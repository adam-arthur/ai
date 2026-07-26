# harness

`harness` is a small Rust framework for plan-driven, tool-using agents.
It separates agent orchestration from model transport and host-side tool
execution.

## Design

On each turn, the model returns a plan directive together with an action. An
action either finishes the run or requests a non-empty batch of tool calls. The
harness owns the resulting plan state and sends it back on subsequent turns,
while tool calls and results are retained as correlated event history. Batches
execute sequentially in request order and are limited to eight calls by default.

The workspace keeps model access, tool execution, and wakeup behavior decoupled
so each can be used or extended independently.

## Start here

- [`Cargo.toml`](Cargo.toml) is the authoritative workspace manifest.
- [`examples/basic.rs`](examples/basic.rs) demonstrates a complete agent with a
  mock model and tool.
- [`crates/llm/examples/llama.rs`](crates/llm/examples/llama.rs) demonstrates
  direct model access.
- [`src/bin/harness.rs`](src/bin/harness.rs) is the workspace-inspection CLI.
