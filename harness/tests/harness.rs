use std::{collections::VecDeque, num::NonZeroUsize, sync::{Arc, Mutex}};

use harness::{Agent, AgentAction, AgentConfig, AgentDecision, AgentError, AgentEvent, AgentModel, AgentModelError, AgentRequest, AgentToolCall, PlanDirective, ToolError, TypedTool, async_trait};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

struct ScriptedAgentModel {
    decisions: Mutex<VecDeque<AgentDecision>>,
    requests: Arc<Mutex<Vec<AgentRequest>>>,
}

impl ScriptedAgentModel {
    fn new(decisions: Vec<AgentDecision>) -> (Self, Arc<Mutex<Vec<AgentRequest>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                decisions: Mutex::new(decisions.into()),
                requests: Arc::clone(&requests),
            },
            requests,
        )
    }
}

#[async_trait]
impl AgentModel for ScriptedAgentModel {
    async fn decide(&self, request: AgentRequest) -> Result<AgentDecision, AgentModelError> {
        self.requests.lock().unwrap().push(request);
        self.decisions
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentModelError::new("script exhausted"))
    }
}

struct Add;
struct Fail;

#[derive(Deserialize, JsonSchema)]
struct AddInput {
    a: i64,
    b: i64,
}

#[derive(Serialize)]
struct AddOutput {
    sum: i64,
}

#[async_trait]
impl TypedTool for Add {
    type Input = AddInput;
    type Output = AddOutput;

    fn name(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> &'static str {
        "Add two integers"
    }

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        Ok(AddOutput { sum: input.a + input.b })
    }
}

#[async_trait]
impl TypedTool for Fail {
    type Input = ();
    type Output = ();

    fn name(&self) -> &'static str {
        "fail"
    }

    fn description(&self) -> &'static str {
        "Always fail"
    }

    async fn invoke(&self, _input: Self::Input) -> Result<Self::Output, ToolError> {
        Err(ToolError::new("expected failure"))
    }
}

#[tokio::test]
async fn agent_plans_and_calls_a_tool_in_the_same_turn() {
    let (model, requests) = ScriptedAgentModel::new(vec![
        AgentDecision {
            plan: PlanDirective::Create {
                steps: vec!["Calculate the answer".into()],
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
                ],
            },
        },
        AgentDecision {
            plan: PlanDirective::Keep,
            action: AgentAction::Finish {
                summary: "The sum is 42".into(),
                output: "42".into(),
            },
        },
    ]);
    let mut agent = Agent::builder(model).build();
    agent.tools_mut().register(Add).unwrap();

    let run = agent.run("What is 20 + 22?").await.unwrap();

    assert_eq!(run.output, "42");
    assert!(run.plan.is_complete());
    assert_eq!(run.turns, 2);
    assert!(run.events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolReturned { name, result, .. }
            if name == "add" && result == &json!({ "sum": 42 })
    )));
    assert!(run.events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolReturned { call_id, result, .. }
            if call_id == "turn-1-call-2" && result == &json!({ "sum": 3 })
    )));

    let requests = requests.lock().unwrap();
    assert!(requests[0].plan.is_none());
    let plan = requests[1].plan.as_ref().unwrap();
    assert_eq!(plan.current().unwrap().description, "Calculate the answer");
    assert!(requests[1].events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolReturned { name, .. } if name == "add"
    )));
}

#[tokio::test]
async fn agent_advances_a_step_and_calls_the_next_tool_in_one_turn() {
    let (model, _) = ScriptedAgentModel::new(vec![
        AgentDecision {
            plan: PlanDirective::Create {
                steps: vec!["Try the operation".into(), "Calculate the fallback".into()],
            },
            action: AgentAction::ToolCalls {
                calls: vec![AgentToolCall {
                    name: "fail".into(),
                    arguments: json!(null),
                }],
            },
        },
        AgentDecision {
            plan: PlanDirective::Advance {
                summary: "The initial operation failed as expected".into(),
            },
            action: AgentAction::ToolCalls {
                calls: vec![AgentToolCall {
                    name: "add".into(),
                    arguments: json!({ "a": 20, "b": 22 }),
                }],
            },
        },
        AgentDecision {
            plan: PlanDirective::Keep,
            action: AgentAction::Finish {
                summary: "Calculated the fallback".into(),
                output: "42".into(),
            },
        },
    ]);
    let mut agent = Agent::builder(model).build();
    agent.tools_mut().register(Fail).unwrap();
    agent.tools_mut().register(Add).unwrap();

    let run = agent.run("Exercise failure recovery").await.unwrap();

    assert_eq!(run.output, "42");
    assert_eq!(run.turns, 3);
    assert!(run.events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolFailed { name, error, .. }
            if name == "fail" && error == "expected failure"
    )));
    assert_eq!(
        run.plan.steps()[0].summary.as_deref(),
        Some("The initial operation failed as expected")
    );
}

#[tokio::test]
async fn agent_can_plan_and_finish_without_a_tool() {
    let (model, _) = ScriptedAgentModel::new(vec![AgentDecision {
        plan: PlanDirective::Create {
            steps: vec!["Answer directly".into()],
        },
        action: AgentAction::Finish {
            summary: "Answered from known information".into(),
            output: "Done".into(),
        },
    }]);

    let run = Agent::builder(model).build().run("Answer directly").await.unwrap();

    assert_eq!(run.output, "Done");
    assert_eq!(run.turns, 1);
}

#[tokio::test]
async fn agent_requires_the_first_decision_to_create_a_plan() {
    let (model, _) = ScriptedAgentModel::new(vec![AgentDecision {
        plan: PlanDirective::Keep,
        action: AgentAction::Finish {
            summary: "No plan".into(),
            output: "invalid".into(),
        },
    }]);

    let error = Agent::builder(model).build().run("Do work").await.unwrap_err();

    assert!(matches!(error, AgentError::PlanNotCreated));
}

#[tokio::test]
async fn agent_rejects_finishing_with_multiple_pending_steps() {
    let (model, _) = ScriptedAgentModel::new(vec![AgentDecision {
        plan: PlanDirective::Create {
            steps: vec!["First".into(), "Second".into()],
        },
        action: AgentAction::Finish {
            summary: "Not all steps are done".into(),
            output: "too early".into(),
        },
    }]);

    let error = Agent::builder(model).build().run("Do both steps").await.unwrap_err();

    assert!(matches!(error, AgentError::PrematureFinish));
}

#[tokio::test]
async fn agent_rejects_an_empty_tool_call_batch() {
    let (model, _) = ScriptedAgentModel::new(vec![AgentDecision {
        plan: PlanDirective::Create {
            steps: vec!["Calculate".into()],
        },
        action: AgentAction::ToolCalls { calls: Vec::new() },
    }]);

    let error = Agent::builder(model).build().run("Calculate").await.unwrap_err();

    assert!(matches!(error, AgentError::EmptyToolCallBatch));
}

#[tokio::test]
async fn agent_enforces_the_tool_call_batch_limit() {
    let (model, requests) = ScriptedAgentModel::new(vec![AgentDecision {
        plan: PlanDirective::Create {
            steps: vec!["Calculate".into()],
        },
        action: AgentAction::ToolCalls {
            calls: vec![
                AgentToolCall {
                    name: "add".into(),
                    arguments: json!({ "a": 1, "b": 2 }),
                },
                AgentToolCall {
                    name: "add".into(),
                    arguments: json!({ "a": 3, "b": 4 }),
                },
            ],
        },
    }]);
    let config = AgentConfig::default().with_max_tool_calls_per_turn(NonZeroUsize::new(1).unwrap());
    let mut agent = Agent::builder(model).config(config).build();
    agent.tools_mut().register(Add).unwrap();

    let error = agent.run("Calculate twice").await.unwrap_err();

    assert!(matches!(
        error,
        AgentError::ToolCallBatchLimitExceeded { count: 2, limit: 1 }
    ));
    assert_eq!(requests.lock().unwrap()[0].max_tool_calls_per_turn, 1);
}
