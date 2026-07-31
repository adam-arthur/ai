# brain-gleam

`brain-gleam` is an experimental Gleam port of `brain` for readable, typed
agent workflows. It invokes one agent node at a time, decodes each node's JSON
result into a Gleam type, and lets ordinary Gleam code choose the next node or
complete the flow.

The user-facing flow graph is implemented. The Codex adapter is deliberately a
stub for now: `brain/codex` exposes the backend boundary but reports a runtime
error instead of launching `codex exec`.

## Example

```gleam
import brain
import brain/codex
import gleam/dynamic/decode
import gleam/json

type ResearchInput {
  ResearchInput(topic: String)
}

type ResearchResult {
  ResearchResult(finding: String, needs_analysis: Bool)
}

fn encode_research_input(input: ResearchInput) -> json.Json {
  json.object([#("topic", json.string(input.topic))])
}

fn decode_research_result() -> decode.Decoder(ResearchResult) {
  use finding <- decode.field("finding", decode.string)
  use needs_analysis <- decode.field("needs_analysis", decode.bool)
  decode.success(ResearchResult(finding:, needs_analysis:))
}

fn research_schema() -> json.Json {
  json.object([
    #("type", json.string("object")),
    #("properties", json.object([
      #("finding", json.object([#("type", json.string("string"))])),
      #("needs_analysis", json.object([#("type", json.string("boolean"))])),
    ])),
    #("required", json.array(["finding", "needs_analysis"], of: json.string)),
    #("additionalProperties", json.bool(False)),
  ])
}

pub fn main() {
  let research = brain.step(
    "research",
    encode_research_input,
    decode_research_result(),
    research_schema(),
  )

  let graph =
    brain.flow("investigate")
    |> brain.begins_with(research, ResearchInput("typed agent workflows"))
    |> brain.node(research)
    |> brain.prompt("Research the topic and return one important finding.")
    |> brain.internet(brain.Enabled)
    |> brain.then(fn(result) { brain.complete(result.finding) })
    |> brain.build

  // The runtime is injected. This currently returns `Error(Failed(...))`
  // because the Codex implementation is a stub.
  brain.run(graph, codex.new() |> codex.as_runtime)
}
```

Gleam does not derive JSON codecs or JSON Schema, so `step` receives an input
encoder, output decoder, and output schema explicitly. The step remains typed
as `Step(input, output)`, which means `begins_with`, `next`, `then`, and `catch`
all check their values at compile time.

## Execution semantics

- `begins_with(step, input)` selects one typed initial invocation.
- Each invocation produces one decoded result or one failure.
- `then` returns `next`, `complete`, or `fail`.
- `next` selects exactly one invocation; there is no fan-out or join.
- `catch` can route the original typed input to any step, including the same
  step.
- Routing to an earlier node is allowed; loops are not detected or limited.
- There are no automatic retries, timeouts, cancellation, budgets,
  concurrency, checkpoint recovery, or human-input transitions.
- Runtime calls are synchronous. A future backend can use an actor internally
  without changing the graph API.

Debug trace persistence and the real Codex process adapter are intentionally
deferred with the backend. `RunConfig` currently carries only the working
directory supplied to runtime requests.

## Development

```sh
gleam format --check src test
gleam test
```
