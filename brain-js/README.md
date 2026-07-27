# brain-js

`brain-js` is an experimental TypeScript library for readable, typed agent
workflows. It invokes one agent node at a time, validates the node's JSON result
with a Zod schema, and lets ordinary TypeScript choose the next node or complete
the flow.

The project maps the Rust crates in `brain` to npm workspaces:

- `workspaces/brain` contains the runtime-neutral `brain-js` flow engine.
- `workspaces/codex` contains the `brain-js-codex` adapter for a locally
  installed Codex CLI.

Unlike Rust, TypeScript types do not exist at runtime. Each node therefore
receives a Zod schema explicitly; the schema provides both output validation and
the JSON Schema sent to an agent runtime.

## Example

```ts
import { z } from "zod";

import {
  Access,
  Internet,
  RunConfig,
  complete,
  fail,
  flow,
  next,
  node,
} from "brain-js";
import { CodexRuntime } from "brain-js-codex";

interface ResearchInput {
  topic: string;
}

const researchResult = z.object({
  finding: z.string(),
  needsAnalysis: z.boolean(),
});

interface AnalysisInput {
  finding: string;
}

const analysisResult = z.object({ report: z.string() });

const research = node<ResearchInput, z.infer<typeof researchResult>>(
  "research",
  researchResult,
)
  .prompt("Research the supplied topic and return the most important finding.")
  .access(Access.ReadOnly)
  .internet(Internet.Enabled);
const analyze = node<AnalysisInput, z.infer<typeof analysisResult>>(
  "analyze",
  analysisResult,
)
  .prompt("Analyze the supplied finding and produce a concise final report.")
  .access(Access.ReadOnly);

const run = await flow<string>("investigate")
  .beginsWith(research.with({ topic: "typed agent workflows" }))
  .after(research, (outcome) => {
    if (!outcome.ok) return fail(outcome.failure.intoError());
    if (!outcome.value.needsAnalysis) return complete(outcome.value.finding);
    return next(analyze.with({ finding: outcome.value.finding }));
  })
  .after(analyze, (outcome) =>
    outcome.ok
      ? complete(outcome.value.report)
      : fail(outcome.failure.intoError()),
  )
  .runWith(
    new CodexRuntime(),
    new RunConfig()
      .workingDirectory(".")
      .debugDirectory("debug"),
  );

console.log(run.output);
```

Every node has one `.after` function. It receives a discriminated outcome that
contains either the typed output or a `NodeFailure` retaining the original
input. Sending that input back to the same node is an ordinary transition:

```ts
.after(research, (outcome) => {
  if (outcome.ok) return complete(outcome.value);
  if (outcome.failure.error().isInvalidOutput()) {
    return next(research.with(outcome.failure.intoInput()));
  }
  return fail(outcome.failure.intoError());
})
```

## Execution semantics

- A flow begins with one typed node invocation.
- Each invocation produces one schema-validated JSON result or one failure.
- Its `.after` function returns `next(...)`, `complete(...)`, or `fail(...)`.
- `next(...)` selects exactly one invocation. There is no fan-out or join.
- Routing to an earlier node is allowed; `brain-js` does not detect or limit
  loops.
- There are no automatic retries, timeouts, cancellation, budgets,
  concurrency, checkpoint recovery, or human-input transitions.
- Every invocation starts a fresh runtime session. The Codex adapter uses
  `--ephemeral` and never resumes an earlier thread.

## Debug traces

`RunConfig.debugDirectory(...)` receives one directory per invocation:

```text
debug/
├── 001-research/
├── 002-analyze/
└── 003-research/
```

The directories contain the input, assembled prompt, output schema, raw and
decoded response when available, runtime events, stdout, stderr, and selected
transition. Existing numbered directories are preserved and numbering
continues from the highest prefix. Only one flow should write to a debug
directory at a time.

Debug traces are observability, not node outputs. Nodes expose only their typed
JSON result. `brain-js` has no API for returning artifacts, file mutations,
workspace deltas, or patches.

## Access settings

`Access` and `Internet` are small, best-effort runtime settings. The Codex
adapter maps them to corresponding CLI options. `Internet.Enabled` enables live
web search rather than general network access for spawned commands. These
settings are not a security boundary, and `brain-js` does not isolate
workspaces, scrub environment variables, manage secrets, or compile tool
policies.

## Development

From this directory:

```sh
npm install
npm test
```

The complete Codex example is in
`workspaces/codex/examples/investigate.ts`.

