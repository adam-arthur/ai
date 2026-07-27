# brain

`brain` is an experimental Rust library for readable, typed agent workflows.
It invokes one agent node at a time, decodes the node's JSON result into a Rust
type, and lets ordinary Rust code choose the next node or complete the flow.

The project is a small workspace:

- `brain` contains the runtime-neutral flow engine.
- `brain-codex` invokes a locally installed Codex CLI through `codex exec`.

## Example

```rust,no_run
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
let research = node::<ResearchInput, ResearchResult>("research")
    .prompt("Research the supplied topic and return the most important finding.")
    .access(Access::ReadOnly)
    .internet(Internet::Enabled);
let analyze = node::<AnalysisInput, AnalysisResult>("analyze")
    .prompt("Analyze the supplied finding and produce a concise final report.")
    .access(Access::ReadOnly);
let run = flow::<String>("investigate")
    .begins_with(research.with(ResearchInput {
        topic: "typed agent workflows".into(),
    }))
    .after(research, move |outcome| match outcome {
        Ok(result) if result.needs_analysis => {
            next(analyze.with(AnalysisInput {
                finding: result.finding,
            }))
        }
        Ok(result) => complete(result.finding),
        Err(failure) => fail(failure.into_error()),
    })
    .after(analyze, |outcome| match outcome {
        Ok(result) => complete(result.report),
        Err(failure) => fail(failure.into_error()),
    })
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

Every node has one `.after` function. It receives either the typed output or a
`NodeFailure` that still owns the original input. Sending that input back to the
same node is an ordinary transition rather than a special retry:

```rust,ignore
.after(research, move |outcome| match outcome {
    Ok(result) => complete(result),
    Err(failure) if failure.error().is_invalid_output() => {
        next(research_again.with(failure.into_input()))
    }
    Err(failure) => fail(failure.into_error()),
})
```

## Execution semantics

- A flow begins with one typed node invocation.
- Each invocation produces one typed JSON result or one failure.
- Its `.after` function returns `next(...)`, `complete(...)`, or `fail(...)`.
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
