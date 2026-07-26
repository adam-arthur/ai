use std::sync::{Arc, Mutex};

use harness::{AgentAction, AgentDecision, AgentModel, AgentRequest, AgentToolCall, Plan, PlanDirective, StructuredAgentModel, ToolDefinition};
use llm::{Model, ModelError, ModelId, ModelRequest, ModelResponse, ModelResponseFormat, async_trait};
use serde_json::json;

struct RecordingModel {
    content: String,
    requests: Arc<Mutex<Vec<ModelRequest>>>,
}

#[async_trait]
impl Model for RecordingModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        self.requests.lock().unwrap().push(request);
        Ok(ModelResponse {
            content: self.content.clone(),
            usage: None,
        })
    }
}

fn tools() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "add".into(),
        description: "Add two integers".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "a": { "type": "integer" },
                "b": { "type": "integer" }
            },
            "required": ["a", "b"],
            "additionalProperties": false
        }),
    }]
}

fn request(plan: Option<Plan>) -> AgentRequest {
    AgentRequest {
        goal: "Add two numbers".into(),
        plan,
        tools: tools(),
        events: Vec::new(),
        max_tool_calls_per_turn: 8,
    }
}

fn recording_model(content: &str, requests: &Arc<Mutex<Vec<ModelRequest>>>) -> RecordingModel {
    RecordingModel {
        content: content.into(),
        requests: Arc::clone(requests),
    }
}

fn response_schema(request: &ModelRequest) -> &serde_json::Value {
    match request.response_format.as_ref().unwrap() {
        ModelResponseFormat::JsonSchema { schema, .. } => schema,
        ModelResponseFormat::JsonObject => panic!("expected a JSON Schema constraint"),
    }
}

#[tokio::test]
async fn structured_model_builds_a_request_and_decodes_a_decision() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model(
        r#"{"plan":{"type":"create","steps":["Calculate"]},"action":{"type":"tool_calls","calls":[{"name":"add","arguments":{"a":20,"b":22}},{"name":"add","arguments":{"a":1,"b":2}}]}}"#,
        &requests,
    );
    let agent_model = StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .max_tokens(512)
        .build();

    let response = agent_model.decide(request(None)).await.unwrap();

    assert_eq!(
        response,
        AgentDecision {
            plan: PlanDirective::Create {
                steps: vec!["Calculate".into()]
            },
            action: AgentAction::ToolCalls {
                calls: vec![
                    AgentToolCall {
                        name: "add".into(),
                        arguments: json!({ "a": 20, "b": 22 }),
                    },
                    AgentToolCall {
                        name: "add".into(),
                        arguments: json!({ "a": 1, "b": 2 }),
                    },
                ]
            }
        }
    );
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0].model, ModelId::GEMMA_4_E2B_Q4);
    assert_eq!(requests[0].temperature, Some(0.0));
    assert_eq!(requests[0].max_tokens, Some(512));
    assert!(
        requests[0].messages[0]
            .content
            .contains("both a plan directive and an action")
    );
    assert!(requests[0].messages[1].content.contains("\"plan\": null"));
}

#[tokio::test]
async fn initial_schema_allows_planning_with_a_tool_or_direct_finish() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model(
        r#"{"plan":{"type":"create","steps":["Calculate"]},"action":{"type":"finish","summary":"done","output":"42"}}"#,
        &requests,
    );

    StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .build()
        .decide(request(None))
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    let alternatives = response_schema(&requests[0])["oneOf"].as_array().unwrap();
    assert_eq!(alternatives.len(), 2);
    let tool = alternatives
        .iter()
        .find(|schema| schema["properties"]["action"]["properties"]["type"]["const"] == "tool_calls")
        .unwrap();
    let finish = alternatives
        .iter()
        .find(|schema| schema["properties"]["action"]["properties"]["type"]["const"] == "finish")
        .unwrap();
    assert_eq!(tool["properties"]["plan"]["properties"]["type"]["const"], "create");
    let calls = &tool["properties"]["action"]["properties"]["calls"];
    assert_eq!(calls["minItems"], 1);
    assert_eq!(calls["maxItems"], 8);
    assert_eq!(calls["items"]["oneOf"][0]["properties"]["name"]["const"], "add");
    assert!(
        tool["properties"]["plan"]["properties"]["steps"]
            .get("maxItems")
            .is_none()
    );
    assert_eq!(finish["properties"]["plan"]["properties"]["steps"]["maxItems"], 1);
}

#[tokio::test]
async fn final_step_schema_keeps_the_plan_and_calls_a_tool_or_finishes() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model(
        r#"{"plan":{"type":"keep"},"action":{"type":"finish","summary":"done","output":"42"}}"#,
        &requests,
    );

    StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .build()
        .decide(request(Some(Plan::new(["Calculate"]).unwrap())))
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    let alternatives = response_schema(&requests[0])["oneOf"].as_array().unwrap();
    let pairs = alternatives
        .iter()
        .map(|schema| {
            (
                schema["properties"]["plan"]["properties"]["type"]["const"]
                    .as_str()
                    .unwrap(),
                schema["properties"]["action"]["properties"]["type"]["const"]
                    .as_str()
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(pairs, [("keep", "tool_calls"), ("keep", "finish")]);
}

#[tokio::test]
async fn intermediate_schema_can_advance_and_act_in_the_same_decision() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model(
        r#"{"plan":{"type":"advance","summary":"first done"},"action":{"type":"tool_calls","calls":[{"name":"add","arguments":{"a":20,"b":22}}]}}"#,
        &requests,
    );

    StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .build()
        .decide(request(Some(Plan::new(["First", "Second", "Third"]).unwrap())))
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    let alternatives = response_schema(&requests[0])["oneOf"].as_array().unwrap();
    let pairs = alternatives
        .iter()
        .map(|schema| {
            (
                schema["properties"]["plan"]["properties"]["type"]["const"]
                    .as_str()
                    .unwrap(),
                schema["properties"]["action"]["properties"]["type"]["const"]
                    .as_str()
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(pairs, [("keep", "tool_calls"), ("advance", "tool_calls")]);
}

#[tokio::test]
async fn penultimate_step_can_advance_and_finish() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model(
        r#"{"plan":{"type":"advance","summary":"first done"},"action":{"type":"finish","summary":"second done","output":"42"}}"#,
        &requests,
    );

    StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .build()
        .decide(request(Some(Plan::new(["First", "Second"]).unwrap())))
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    let alternatives = response_schema(&requests[0])["oneOf"].as_array().unwrap();
    assert!(alternatives.iter().any(|schema| {
        schema["properties"]["plan"]["properties"]["type"]["const"] == "advance"
            && schema["properties"]["action"]["properties"]["type"]["const"] == "finish"
    }));
}

#[tokio::test]
async fn schema_couples_each_tool_name_to_its_arguments() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model(
        r#"{"plan":{"type":"keep"},"action":{"type":"finish","summary":"done","output":"done"}}"#,
        &requests,
    );
    let mut agent_request = request(Some(Plan::new(["Inspect"]).unwrap()));
    agent_request.tools.push(ToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
            "additionalProperties": false
        }),
    });

    StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .build()
        .decide(agent_request)
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    let alternatives = response_schema(&requests[0])["oneOf"].as_array().unwrap();
    let tool_actions = alternatives
        .iter()
        .filter_map(|schema| {
            let action = &schema["properties"]["action"];
            (action["properties"]["type"]["const"] == "tool_calls").then_some(action)
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_actions.len(), 1);
    let call_alternatives = tool_actions[0]["properties"]["calls"]["items"]["oneOf"]
        .as_array()
        .unwrap();
    let read_file = call_alternatives
        .iter()
        .find(|call| call["properties"]["name"]["const"] == "read_file")
        .unwrap();
    assert_eq!(read_file["properties"]["arguments"]["required"], json!(["path"]));
}

#[tokio::test]
async fn structured_model_reports_the_raw_invalid_response() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let model = recording_model("certainly, here is the plan", &requests);
    let error = StructuredAgentModel::builder(model, ModelId::GEMMA_4_E2B_Q4)
        .build()
        .decide(request(None))
        .await
        .unwrap_err();

    assert!(error.message.contains("raw response: certainly, here is the plan"));
}
