# brain-js-codex

`brain-js-codex` implements `AgentRuntime` with the local Codex CLI. Each node
runs as a fresh `codex exec --ephemeral` invocation with JSONL events and a JSON
Schema-constrained final response.

```ts
import { CodexRuntime } from "brain-js-codex";

const runtime = new CodexRuntime();
```

The executable defaults to `codex` on `PATH` and can be replaced for a custom
installation:

```ts
const runtime = new CodexRuntime().executable("/opt/bin/codex");
```

This adapter is intentionally minimal and its access options are best effort,
not a security boundary. `Internet.Enabled` exposes Codex's live web-search
tool; it does not separately configure network access for arbitrary commands.

