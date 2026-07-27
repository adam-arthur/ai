use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::async_trait;

/// Best-effort filesystem access requested for one node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    #[default]
    ReadOnly,
    WorkspaceWrite,
    Full,
}

/// Best-effort internet access requested for one node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Internet {
    #[default]
    Disabled,
    Enabled,
}

/// A runtime-neutral request to invoke one agent node.
#[derive(Clone, Debug)]
pub struct RuntimeRequest {
    pub flow_name: String,
    pub node_name: String,
    pub invocation: usize,
    pub prompt: String,
    pub output_schema: Value,
    pub working_directory: PathBuf,
    pub access: Access,
    pub internet: Internet,
}

/// The observable result of a successful runtime invocation.
#[derive(Clone, Debug, Default)]
pub struct RuntimeResponse {
    pub output: String,
    pub events: Vec<Value>,
    pub stdout: String,
    pub stderr: String,
}

impl RuntimeResponse {
    pub fn new(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            ..Self::default()
        }
    }
}

/// A failed runtime invocation together with any observable diagnostics.
#[derive(Clone, Debug, Error)]
#[error("{message}")]
pub struct RuntimeError {
    pub message: String,
    pub events: Vec<Value>,
    pub stdout: String,
    pub stderr: String,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            events: Vec::new(),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    pub fn with_diagnostics(
        mut self, events: Vec<Value>, stdout: impl Into<String>, stderr: impl Into<String>,
    ) -> Self {
        self.events = events;
        self.stdout = stdout.into();
        self.stderr = stderr.into();
        self
    }
}

/// Executes one fully assembled agent invocation.
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    async fn invoke(&self, request: RuntimeRequest) -> Result<RuntimeResponse, RuntimeError>;
}
