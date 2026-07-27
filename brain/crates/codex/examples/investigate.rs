use brain::{Access, Internet, RunConfig, complete, fail, flow, next, node};
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let topic = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "typed agent workflows".into());
    let research = node::<ResearchInput, ResearchResult>("research")
        .prompt("Research the topic and return one important, well-supported finding.")
        .access(Access::ReadOnly)
        .internet(Internet::Enabled);
    let analyze = node::<AnalysisInput, AnalysisResult>("analyze")
        .prompt("Analyze the finding. Return a final report, or a focused follow-up topic if more research is needed.")
        .access(Access::ReadOnly);
    let analyze_after_research = analyze.clone();
    let research_after_analysis = research.clone();

    let run = flow::<String>("investigate")
        .begins_with(research.with(ResearchInput { topic }))
        .after(research, move |outcome| match outcome {
            Ok(result) => next(analyze_after_research.with(AnalysisInput {
                finding: result.finding,
            })),
            Err(failure) => fail(failure.into_error()),
        })
        .after(analyze, move |outcome| match outcome {
            Ok(result) => match result.follow_up {
                Some(topic) => next(research_after_analysis.with(ResearchInput { topic })),
                None => complete(result.report),
            },
            Err(failure) => fail(failure.into_error()),
        })
        .run_with(
            &CodexRuntime::new(),
            RunConfig::new().working_directory(".").debug_directory("debug"),
        )
        .await?;

    println!("{}", run.output);
    Ok(())
}
