use std::{collections::VecDeque, fs, sync::Mutex};

use harness::{Agent, AgentAction, AgentDecision, AgentEvent, AgentModel, AgentModelError, AgentRequest, AgentToolCall, PlanDirective, WorkspaceTools, async_trait};
use serde_json::json;
use tempfile::tempdir;

struct ScriptedModel {
    decisions: Mutex<VecDeque<AgentDecision>>,
}

#[async_trait]
impl AgentModel for ScriptedModel {
    async fn decide(&self, _request: AgentRequest) -> Result<AgentDecision, AgentModelError> {
        self.decisions
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentModelError::new("script exhausted"))
    }
}

#[tokio::test]
async fn agent_can_inspect_a_workspace_end_to_end() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("answer.txt"), "The answer is 42.\n").unwrap();

    let model = ScriptedModel {
        decisions: Mutex::new(
            vec![
                AgentDecision {
                    plan: PlanDirective::Create {
                        steps: vec!["Read the answer file".into()],
                    },
                    action: AgentAction::ToolCalls {
                        calls: vec![AgentToolCall {
                            name: "read_file".into(),
                            arguments: json!({ "path": "answer.txt" }),
                        }],
                    },
                },
                AgentDecision {
                    plan: PlanDirective::Keep,
                    action: AgentAction::Finish {
                        summary: "The file says the answer is 42".into(),
                        output: "42".into(),
                    },
                },
            ]
            .into(),
        ),
    };
    let workspace = WorkspaceTools::new(directory.path()).unwrap();
    let mut agent = Agent::builder(model).build();
    workspace.register(agent.tools_mut()).unwrap();

    let run = agent.run("What is the answer?").await.unwrap();

    assert_eq!(run.output, "42");
    assert_eq!(run.turns, 2);
    assert!(run.events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolReturned { name, result, .. }
            if name == "read_file" && result["content"] == "The answer is 42.\n"
    )));
}
