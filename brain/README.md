# brain

`brain` is an experimental Rust library for readable, typed agent workflows.
It invokes one agent node at a time, decodes the node's JSON result into a Rust
type, and lets ordinary Rust code choose the next node or complete the flow.

The project is a small workspace:

- `brain` contains the runtime-neutral flow engine.
- `brain-codex` invokes a locally installed Codex CLI through `codex exec`.

## Example

```rust,no_run
use brain::{Access, Internet, RunConfig, complete, flow, next, step};
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

# async fn example() -> Result<(), brain::FlowError> {
let research = step::<ResearchInput, ResearchResult>("research");
let analyze = step::<AnalysisInput, AnalysisResult>("analyze");
let run = flow::<String>("investigate")
    .begins_with(research, ResearchInput {
        topic: "typed agent workflows".into(),
    })
    .node(research)
    .prompt("Research the supplied topic and return the most important finding.")
    .access(Access::ReadOnly)
    .internet(Internet::Enabled)
    .then(move |result| {
        if result.needs_analysis {
            next(analyze, AnalysisInput {
                finding: result.finding,
            })
        } else {
            complete(result.finding)
        }
    })
    .node(analyze)
    .prompt("Analyze the supplied finding and produce a concise final report.")
    .access(Access::ReadOnly)
    .then(|result| complete(result.report))
    .build()
    .run_with(
        &CodexRuntime::new(),
        RunConfig::new()
            .working_directory(".")
            .debug_directory("debug"),
    )
    .await?;

println!("{}", run.output);
# Ok(())
# }
```

Every node has one `.then` function that receives its typed output. A failure
stops the flow automatically unless the node has an optional `.catch` function.
The `StepFailure` passed to that function still owns the original input, so
sending it back to the same node is an ordinary transition rather than a
special retry:

```rust,ignore
.node(research)
.prompt("Research the topic.")
.catch(move |failure| {
    if failure.error().is_invalid_output() {
        next(research, failure.into_input())
    } else {
        fail(failure)
    }
})
.then(complete)
```

## Execution semantics

- A flow's `.begins_with(step, input)` selects one typed initial invocation.
- Each invocation produces one typed JSON result or one failure.
- Its `.then` function returns `next(...)`, `complete(...)`, or `fail(...)`.
- `next(...)` selects exactly one invocation. There is no fan-out or join.
- Routing to an earlier node is allowed; `brain` does not detect or limit loops.
- There are no automatic retries, timeouts, cancellation, budgets, concurrency,
  checkpoint recovery, or human-input transitions.
- Every invocation starts a fresh runtime session. The Codex adapter uses
  `--ephemeral` and never resumes an earlier thread.

## Debug traces

`RunConfig::debug_directory` receives one directory per invocation:

```text
debug/
├── 001-research/
├── 002-analyze/
└── 003-research/
```

The directories contain the input, assembled prompt, output schema, raw and
decoded response when available, runtime events, stdout, stderr, and selected
transition. Existing numbered directories are preserved and numbering continues
from the highest prefix. The implementation intentionally assumes only one flow
runs against a debug directory at a time.

Debug traces are observability, not node outputs. Nodes expose only their typed
JSON result. `brain` has no API for returning artifacts, file mutations,
workspace deltas, or patches.

## Access settings

`Access` and `Internet` are small, best-effort runtime settings. The Codex
adapter maps them to the corresponding CLI options. For Codex,
`Internet::Enabled` enables live web search rather than general network access
for spawned commands. These settings are not a security boundary, and `brain`
does not isolate workspaces, scrub environment variables, manage secrets, or
compile tool policies.

Arbitrary tool allowlists and richer per-node model settings are intentionally
deferred until their runtime semantics are clear.
