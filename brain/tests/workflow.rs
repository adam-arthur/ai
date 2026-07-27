use std::{collections::VecDeque, sync::Mutex};

use brain::{AgentRuntime, FlowError, RunConfig, RuntimeError, RuntimeRequest, RuntimeResponse, async_trait, complete, fail, flow, next, step};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ResearchInput {
    topic: String,
}

#[derive(Deserialize, JsonSchema)]
struct ResearchResult {
    finding: String,
    needs_analysis: bool,
}

#[derive(Serialize)]
struct AnalysisInput {
    finding: String,
}

#[derive(Deserialize, JsonSchema)]
struct AnalysisResult {
    report: String,
}

struct QueueRuntime {
    responses: Mutex<VecDeque<Result<RuntimeResponse, RuntimeError>>>,
    requests: Mutex<Vec<RuntimeRequest>>,
}

impl QueueRuntime {
    fn new(responses: impl IntoIterator<Item = Result<RuntimeResponse, RuntimeError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AgentRuntime for QueueRuntime {
    async fn invoke(&self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError> {
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("a mock response must be available")
    }
}

#[tokio::test]
async fn runs_heterogeneous_nodes_and_completes_with_a_typed_value() {
    let temporary = tempfile::tempdir().unwrap();
    let debug = temporary.path().join("debug");
    let runtime = QueueRuntime::new([
        Ok(RuntimeResponse::new(
            r#"{"finding":"typed flows are useful","needs_analysis":true}"#,
        )),
        Ok(RuntimeResponse::new(r#"{"report":"ship the experiment"}"#)),
    ]);
    let research = step::<ResearchInput, ResearchResult>("research");
    let analyze = step::<AnalysisInput, AnalysisResult>("analyze");
    let run = flow::<String>("investigate")
        .begins_with(
            research,
            ResearchInput {
                topic: "agent workflows".into(),
            },
        )
        .node(research)
        .prompt("Research the topic.")
        .then(move |result| {
            if result.needs_analysis {
                next(
                    analyze,
                    AnalysisInput {
                        finding: result.finding,
                    },
                )
            } else {
                complete(result.finding)
            }
        })
        .node(analyze)
        .prompt("Analyze the finding.")
        .then(|result| complete(result.report))
        .build()
        .run_with(
            &runtime,
            RunConfig::new()
                .working_directory(temporary.path())
                .debug_directory(&debug),
        )
        .await
        .unwrap();

    assert_eq!(run.output, "ship the experiment");
    assert_eq!(run.invocations, 2);
    assert!(debug.join("001-research/prompt.md").is_file());
    assert!(debug.join("001-research/response.json").is_file());
    assert!(debug.join("002-analyze/transition.json").is_file());

    let requests = runtime.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].prompt.contains("agent workflows"));
    assert_eq!(requests[0].output_schema["required"][0], "finding");
}

#[derive(Serialize)]
struct AttemptInput {
    attempt: usize,
}

#[derive(Deserialize, JsonSchema)]
struct AttemptOutput {
    answer: String,
}

#[tokio::test]
async fn a_catch_handler_can_route_to_the_same_node_as_an_ordinary_next_transition() {
    let temporary = tempfile::tempdir().unwrap();
    let debug = temporary.path().join("debug");
    let runtime = QueueRuntime::new([
        Ok(RuntimeResponse::new("not json")),
        Ok(RuntimeResponse::new(r#"{"answer":"recovered"}"#)),
    ]);
    let research = step::<AttemptInput, AttemptOutput>("research");
    let run = flow::<String>("retry-by-routing")
        .begins_with(research, AttemptInput { attempt: 1 })
        .node(research)
        .prompt("Return an answer.")
        .catch(move |failure| {
            if failure.error().is_invalid_output() {
                let mut input = failure.into_input();
                input.attempt += 1;
                next(research, input)
            } else {
                fail(failure)
            }
        })
        .then(|result| complete(result.answer))
        .build()
        .run_with(&runtime, RunConfig::new().debug_directory(&debug))
        .await
        .unwrap();

    assert_eq!(run.output, "recovered");
    assert_eq!(run.invocations, 2);
    assert!(debug.join("001-research/response.raw.txt").is_file());
    assert!(debug.join("002-research/response.json").is_file());
}

#[tokio::test]
async fn runtime_failures_reach_catch_with_the_original_input() {
    let temporary = tempfile::tempdir().unwrap();
    let runtime = QueueRuntime::new([Err(RuntimeError::new("model unavailable"))]);
    let research = step::<AttemptInput, AttemptOutput>("research");

    let error = flow::<String>("failure")
        .begins_with(research, AttemptInput { attempt: 7 })
        .node(research)
        .prompt("Return an answer.")
        .catch(|failure| {
            assert_eq!(failure.input().attempt, 7);
            assert_eq!(failure.invocation(), 1);
            assert!(failure.error().is_runtime());
            fail(failure)
        })
        .then(|result| complete(result.answer))
        .build()
        .run_with(
            &runtime,
            RunConfig::new().debug_directory(temporary.path().join("debug")),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, FlowError::Failed(_)));
    assert!(error.to_string().contains("model unavailable"));
}

#[tokio::test]
async fn failures_stop_the_flow_without_a_catch_handler() {
    let temporary = tempfile::tempdir().unwrap();
    let runtime = QueueRuntime::new([Err(RuntimeError::new("model unavailable"))]);
    let research = step::<AttemptInput, AttemptOutput>("research");

    let error = flow::<String>("failure")
        .begins_with(research, AttemptInput { attempt: 1 })
        .node(research)
        .prompt("Return an answer.")
        .then(|_| panic!("the success handler must not run"))
        .build()
        .run_with(
            &runtime,
            RunConfig::new().debug_directory(temporary.path().join("debug")),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, FlowError::Failed(_)));
    assert!(error.to_string().contains("model unavailable"));
}

#[tokio::test]
async fn rejects_missing_handlers_before_invoking_the_runtime() {
    let temporary = tempfile::tempdir().unwrap();
    let runtime = QueueRuntime::new([]);
    let research = step::<AttemptInput, AttemptOutput>("research");

    let error = flow::<String>("invalid")
        .begins_with(research, AttemptInput { attempt: 1 })
        .build()
        .run_with(
            &runtime,
            RunConfig::new().debug_directory(temporary.path().join("debug")),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, FlowError::InvalidDefinition(_)));
    assert!(error.to_string().contains("no node definition"));
    assert!(runtime.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_a_different_step_with_the_same_name_as_the_registered_node() {
    let temporary = tempfile::tempdir().unwrap();
    let runtime = QueueRuntime::new([]);
    let initial = step::<AttemptInput, AttemptOutput>("research");
    let registered = step::<AttemptInput, AttemptOutput>("research");

    let error = flow::<String>("invalid")
        .begins_with(initial, AttemptInput { attempt: 1 })
        .node(registered)
        .prompt("Return an answer.")
        .then(|result| complete(result.answer))
        .build()
        .run_with(
            &runtime,
            RunConfig::new().debug_directory(temporary.path().join("debug")),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, FlowError::InvalidDefinition(_)));
    assert!(error.to_string().contains("registered step handle"));
    assert!(runtime.requests.lock().unwrap().is_empty());
}
