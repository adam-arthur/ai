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
receives Zod input and output schemas. They infer the node's types, validate its
values, and provide the JSON Schema sent to an agent runtime.

## Example

```ts
import { z } from "zod";

import { complete, fail, flow, next, node } from "brain-js";
import { CodexRuntime } from "brain-js-codex";

const research = node({
  name: "research",
  input: z.object({ topic: z.string() }),
  output: z.object({
    finding: z.string(),
    needsAnalysis: z.boolean(),
  }),
  prompt: "Research the supplied topic and return the most important finding.",
  internet: true,
});

const analyze = node({
  name: "analyze",
  input: z.object({ finding: z.string() }),
  output: z.object({ report: z.string() }),
  prompt: "Analyze the supplied finding and produce a concise final report.",
});

const run = await flow<string>("investigate")
  .startWith(research.withInput({ topic: "typed agent workflows" }))
  .on(research, (result) => {
    if (!result.ok) return fail(result.error);
    if (!result.value.needsAnalysis) return complete(result.value.finding);
    return next(analyze.withInput({ finding: result.value.finding }));
  })
  .on(analyze, (result) =>
    result.ok ? complete(result.value.report) : fail(result.error),
  )
  .run(new CodexRuntime(), {
    workingDirectory: ".",
    debugDirectory: "debug",
  });

console.log(run.output);
```

Every node has one `.on` handler. It receives a discriminated result containing
either the typed output or the error, original input, and invocation number.
Sending that input back to the same node is an ordinary transition:

```ts
.on(research, (result) => {
  if (result.ok) return complete(result.value);
  if (result.error.kind === "invalid_output") {
    return next(research.withInput(result.input));
  }
  return fail(result.error);
})
```

## Execution semantics

- A flow begins with one typed node invocation.
- Each invocation produces one schema-validated JSON result or one failure.
- Its `.on` handler returns `next(...)`, `complete(...)`, or `fail(...)`.
- `next(...)` selects exactly one invocation. There is no fan-out or join.
- Routing to an earlier node is allowed; `brain-js` does not detect or limit
  loops.
- There are no automatic retries, timeouts, cancellation, budgets,
  concurrency, checkpoint recovery, or human-input transitions.
- Every invocation starts a fresh runtime session. The Codex adapter uses
  `--ephemeral` and never resumes an earlier thread.

## Debug traces

The configured `debugDirectory` receives one directory per invocation:

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

Node `access` and `internet` options are small, best-effort runtime settings.
The Codex adapter maps them to corresponding CLI options. `internet: true`
enables live web search rather than general network access for spawned
commands. These settings are not a security boundary, and `brain-js` does not
isolate workspaces, scrub environment variables, manage secrets, or compile
tool policies.

## Development

From this directory:

```sh
npm install
npm test
```

Node.js runs the TypeScript sources and tests directly using native type
stripping; there is no compilation step.

The complete Codex example is in
`workspaces/codex/examples/investigate.ts`.
