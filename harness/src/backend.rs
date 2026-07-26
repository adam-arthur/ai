use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{AgentEvent, Plan, ToolDefinition, async_trait};

const AGENT_PROTOCOL_PROMPT: &str = r"You drive a plan-based tool-using agent.

Return one decision containing both a plan directive and an action.
Create a concise plan on the first decision. On later decisions, keep the current step while continuing work on it, or advance it only when it is complete.
Use the event history to account for previous tool calls and results. Tool failures are observations; adjust the approach or arguments and continue.
Every decision must either call one or more tools or finish with a user-facing answer. Include multiple tool calls when they can be made independently from the information already available. When advancing, every call in the action applies to the newly current step.
Finish only when the goal and final pending plan step are complete.";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentRequest {
    pub goal: String,
    pub plan: Option<Plan>,
    pub tools: Vec<ToolDefinition>,
    pub events: Vec<AgentEvent>,
    pub max_tool_calls_per_turn: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentDecision {
    pub plan: PlanDirective,
    pub action: AgentAction,
}

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanDirective {
    Create {
        #[schemars(length(min = 1), inner(length(min = 1)))]
        steps: Vec<String>,
    },
    Keep,
    Advance {
        #[schemars(length(min = 1))]
        summary: String,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentAction {
    ToolCalls {
        #[schemars(length(min = 1))]
        calls: Vec<AgentToolCall>,
    },
    Finish {
        #[schemars(length(min = 1))]
        summary: String,
        #[schemars(length(min = 1))]
        output: String,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentToolCall {
    pub name: String,
    pub arguments: Value,
}

/// The harness-specific boundary that chooses the agent's next typed decision.
#[async_trait]
pub trait AgentModel: Send + Sync {
    async fn decide(&self, request: AgentRequest) -> Result<AgentDecision, AgentModelError>;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct AgentModelError {
    pub message: String,
}

impl AgentModelError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Adapts a provider-neutral language model to the harness's JSON protocol.
#[derive(bon::Builder)]
pub struct StructuredAgentModel<M> {
    #[builder(start_fn)]
    model: M,
    #[builder(start_fn)]
    model_id: llm::ModelId,
    #[builder(required, default = Some(0.0), with = Some)]
    temperature: Option<f32>,
    max_tokens: Option<u32>,
}

impl<M> StructuredAgentModel<M> {
    pub fn inner(&self) -> &M {
        &self.model
    }
}

#[async_trait]
impl<M> AgentModel for StructuredAgentModel<M>
where
    M: llm::Model,
{
    async fn decide(&self, request: AgentRequest) -> Result<AgentDecision, AgentModelError> {
        if request.max_tool_calls_per_turn == 0 {
            return Err(AgentModelError::new("max_tool_calls_per_turn must be at least 1"));
        }
        let schema = decision_schema(&request);
        let payload = serde_json::to_string_pretty(&request)
            .map_err(|error| AgentModelError::new(format!("failed to encode agent request: {error}")))?;
        let model_request = llm::ModelRequest::builder(
            self.model_id,
            [
                llm::ModelMessage::system(AGENT_PROTOCOL_PROMPT),
                llm::ModelMessage::user(format!("Agent request:\n{payload}")),
            ],
        )
        .maybe_temperature(self.temperature)
        .maybe_max_tokens(self.max_tokens)
        .response_format(llm::ModelResponseFormat::JsonSchema {
            name: "agent_decision".into(),
            schema,
        })
        .build();

        let response = self
            .model
            .complete(model_request)
            .await
            .map_err(|error| AgentModelError::new(format!("model query failed: {error}")))?;
        serde_json::from_str(response.content.trim()).map_err(|error| {
            AgentModelError::new(format!(
                "model returned an invalid agent decision: {error}; raw response: {}",
                response.content
            ))
        })
    }
}

fn decision_schema(request: &AgentRequest) -> Value {
    let mut alternatives = Vec::new();
    match &request.plan {
        None => {
            if !request.tools.is_empty() {
                alternatives.push(decision_alternative(
                    create_plan_schema(None),
                    tool_calls_action_schema(&request.tools, request.max_tool_calls_per_turn),
                ));
            }
            alternatives.push(decision_alternative(
                create_plan_schema(Some(1)),
                finish_action_schema(),
            ));
        },
        Some(plan) => {
            let remaining = plan.remaining_steps();
            if !request.tools.is_empty() {
                let action = || tool_calls_action_schema(&request.tools, request.max_tool_calls_per_turn);
                alternatives.push(decision_alternative(keep_plan_schema(), action()));
                if remaining > 1 {
                    alternatives.push(decision_alternative(advance_plan_schema(), action()));
                }
            }
            match remaining {
                1 => alternatives.push(decision_alternative(keep_plan_schema(), finish_action_schema())),
                2 => alternatives.push(decision_alternative(advance_plan_schema(), finish_action_schema())),
                _ => {},
            }
        },
    }
    let mut schema = typed_schema::<AgentDecision>();
    schema.as_object_mut().expect("object schema").clear();
    schema["oneOf"] = alternatives.into();
    schema
}

fn decision_alternative(plan: Value, action: Value) -> Value {
    let mut schema = typed_schema::<AgentDecision>();
    schema["properties"]["plan"] = plan;
    schema["properties"]["action"] = action;
    schema
}

fn create_plan_schema(max_items: Option<usize>) -> Value {
    let mut schema = enum_variant_schema::<PlanDirective>("create");
    if let Some(max_items) = max_items {
        schema["properties"]["steps"]["maxItems"] = max_items.into();
    }
    schema
}

fn keep_plan_schema() -> Value {
    enum_variant_schema::<PlanDirective>("keep")
}

fn advance_plan_schema() -> Value {
    enum_variant_schema::<PlanDirective>("advance")
}

fn tool_calls_action_schema(tools: &[ToolDefinition], max_items: usize) -> Value {
    let alternatives = tools.iter().map(tool_call_schema).collect::<Vec<_>>();
    let mut schema = enum_variant_schema::<AgentAction>("tool_calls");
    schema["properties"]["calls"]["maxItems"] = max_items.into();
    schema["properties"]["calls"]["items"] = one_of_schema(alternatives);
    schema
}

fn tool_call_schema(tool: &ToolDefinition) -> Value {
    let mut schema = typed_schema::<AgentToolCall>();
    schema["description"] = tool.description.clone().into();
    schema["properties"]["name"] = const_schema(tool.name.clone().into());
    schema["properties"]["arguments"] = tool.input_schema.clone();
    schema
}

fn finish_action_schema() -> Value {
    enum_variant_schema::<AgentAction>("finish")
}

fn typed_schema<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::default()
        .with(|settings| {
            settings.inline_subschemas = true;
            settings.meta_schema = None;
        })
        .into_generator();
    serde_json::to_value(generator.into_root_schema_for::<T>()).expect("JSON Schema must serialize")
}

fn enum_variant_schema<T: JsonSchema>(tag: &str) -> Value {
    typed_schema::<T>()["oneOf"]
        .as_array()
        .expect("tagged enum schema must contain oneOf")
        .iter()
        .find(|schema| schema["properties"]["type"]["const"] == tag)
        .unwrap_or_else(|| panic!("tagged enum schema must contain the `{tag}` variant"))
        .clone()
}

fn one_of_schema(alternatives: Vec<Value>) -> Value {
    let mut schema = typed_schema::<Value>();
    schema.as_object_mut().expect("object schema").clear();
    schema["oneOf"] = alternatives.into();
    schema
}

fn const_schema(value: Value) -> Value {
    let mut schema = typed_schema::<Value>();
    schema.as_object_mut().expect("object schema").clear();
    schema["const"] = value;
    schema
}
