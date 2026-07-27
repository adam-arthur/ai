use brain::{FlowError, Internet, complete, flow, next, step};
use brain_codex::CodexRuntime;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ResearchInput {
    topic: String,
}

#[derive(Deserialize, JsonSchema)]
struct ResearchResult {
    finding: String,
}

#[derive(Serialize)]
struct AnalysisInput {
    finding: String,
}

#[derive(Deserialize, JsonSchema)]
struct AnalysisResult {
    report: String,
    follow_up: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), FlowError> {
    let topic = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "typed agent workflows".into());

    let research = step::<ResearchInput, ResearchResult>("research");
    let analyze = step::<AnalysisInput, AnalysisResult>("analyze");

    let run = flow::<String>("investigate")
        .begins_with(research, ResearchInput { topic })
        .node(research)
        .prompt("Research the topic and return one important, well-supported finding.")
        .internet(Internet::Enabled)
        .then(move |result| {
            next(
                analyze,
                AnalysisInput {
                    finding: result.finding,
                },
            )
        })
        .node(analyze)
        .prompt("Analyze the finding. Return a final report, or a focused follow-up topic if more research is needed.")
        .then(move |result| match result.follow_up {
            Some(topic) => next(research, ResearchInput { topic }),
            None => complete(result.report),
        })
        .build()
        .run(&CodexRuntime::new())
        .await?;

    println!("{}", run.output);
    Ok(())
}
