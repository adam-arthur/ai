//// Stub Codex runtime for `brain`.
////
//// This module intentionally exposes the future backend boundary without
//// invoking a process yet.

import brain.{type AgentRuntime, type RuntimeRequest, type RuntimeResponse}

/// A placeholder for the future Codex-backed runtime.
pub opaque type CodexRuntime {
  CodexRuntime
}

/// Creates a stub Codex runtime.
pub fn new() -> CodexRuntime {
  CodexRuntime
}

/// Invokes the stub, which always reports that the backend is unavailable.
pub fn invoke(
  _runtime: CodexRuntime,
  _request: RuntimeRequest,
) -> Result(RuntimeResponse, brain.RuntimeError) {
  Error(brain.runtime_error(
    "the Codex backend is not implemented in brain-gleam yet",
  ))
}

/// Adapts a Codex runtime value to `brain.AgentRuntime`.
pub fn as_runtime(runtime: CodexRuntime) -> AgentRuntime {
  fn(request) { invoke(runtime, request) }
}
