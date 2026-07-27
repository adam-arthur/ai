# brain-codex

`brain-codex` implements `brain::AgentRuntime` with the local Codex CLI. Each
node runs as a fresh `codex exec --ephemeral` invocation with JSONL events and a
JSON Schema-constrained final response.

```rust
let runtime = brain_codex::CodexRuntime::new();
```

The executable defaults to `codex` on `PATH` and can be replaced for a custom
installation:

```rust
let runtime = brain_codex::CodexRuntime::new().executable("/opt/bin/codex");
```

This adapter is intentionally minimal and its access options are best effort,
not a security boundary. `Internet::Enabled` exposes Codex's live web-search
tool; it does not separately configure network access for arbitrary commands.
