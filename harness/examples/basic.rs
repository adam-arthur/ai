use std::{collections::VecDeque, sync::Mutex};

use harness::{Agent, AgentAction, AgentDecision, AgentModel, AgentModelError, AgentRequest, AgentToolCall, PlanDirective, ToolError, TypedTool, async_trait};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

// A real adapter over llm::Model implements this same harness protocol.
struct MockAgentModel {
    decisions: Mutex<VecDeque<AgentDecision>>,
}

#[async_trait]
impl AgentModel for MockAgentModel {
    async fn decide(&self, _request: AgentRequest) -> Result<AgentDecision, AgentModelError> {
        self.decisions
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentModelError::new("no response available"))
    }
}

struct Echo;

#[derive(Deserialize, Serialize, JsonSchema)]
struct EchoMessage {
    message: String,
}

#[async_trait]
impl TypedTool for Echo {
    type Input = EchoMessage;
    type Output = EchoMessage;

    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Echo a message"
    }

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        Ok(input)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = MockAgentModel {
        decisions: Mutex::new(
            vec![
                AgentDecision {
                    plan: PlanDirective::Create {
                        steps: vec!["Echo the greeting".into()],
                    },
                    action: AgentAction::ToolCalls {
                        calls: vec![AgentToolCall {
                            name: "echo".into(),
                            arguments: json!({ "message": "hello" }),
                        }],
                    },
                },
                AgentDecision {
                    plan: PlanDirective::Keep,
                    action: AgentAction::Finish {
                        summary: "Greeting echoed".into(),
                        output: "Done".into(),
                    },
                },
            ]
            .into(),
        ),
    };

    let mut agent = Agent::builder(model).build();
    agent.tools_mut().register(Echo)?;
    let run = agent.run("Echo hello").await?;
    println!("{}", run.output);
    Ok(())
}
