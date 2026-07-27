use std::{fmt, io, path::PathBuf};

use thiserror::Error;

/// The category of a failed node invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvocationErrorKind {
    /// The input could not be serialized for the runtime.
    InvalidInput,
    /// The runtime could not complete the invocation.
    Runtime,
    /// The runtime's final response did not deserialize into the node output.
    InvalidOutput,
}

/// An error produced while invoking or decoding one node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvocationError {
    kind: InvocationErrorKind,
    message: String,
}

impl InvocationError {
    pub(crate) fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            kind: InvocationErrorKind::InvalidInput,
            message: message.into(),
        }
    }

    pub(crate) fn runtime(message: impl Into<String>) -> Self {
        Self {
            kind: InvocationErrorKind::Runtime,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_output(message: impl Into<String>) -> Self {
        Self {
            kind: InvocationErrorKind::InvalidOutput,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> InvocationErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn is_invalid_input(&self) -> bool {
        matches!(self.kind, InvocationErrorKind::InvalidInput)
    }

    pub const fn is_runtime(&self) -> bool {
        matches!(self.kind, InvocationErrorKind::Runtime)
    }

    pub const fn is_invalid_output(&self) -> bool {
        matches!(self.kind, InvocationErrorKind::InvalidOutput)
    }
}

impl fmt::Display for InvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvocationError {}

/// A consumer-selected failure that stops a flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowFailure {
    message: String,
}

impl FlowFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FlowFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FlowFailure {}

impl From<String> for FlowFailure {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for FlowFailure {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl From<InvocationError> for FlowFailure {
    fn from(error: InvocationError) -> Self {
        Self::new(error.to_string())
    }
}

/// An error that prevents a flow from completing.
#[derive(Debug, Error)]
pub enum FlowError {
    #[error("invalid flow definition: {0}")]
    InvalidDefinition(String),
    #[error("flow failed: {0}")]
    Failed(#[source] FlowFailure),
    #[error("failed to access `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("internal flow type mismatch for node `{0}`")]
    TypeMismatch(String),
}

impl FlowError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
