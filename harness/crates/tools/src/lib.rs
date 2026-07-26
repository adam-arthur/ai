//! Model-visible tool definitions and host-side tool execution primitives.
//!
//! This crate does not depend on a model transport or an agent runtime. A
//! runtime can advertise [`ToolDefinition`] values to a model and use a
//! [`ToolRegistry`] to resolve the tool calls the model returns.

#![forbid(unsafe_code)]

use std::{collections::BTreeMap, sync::Arc};

pub use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

mod workspace;

pub use workspace::{WorkspaceTools, WorkspaceToolsError};

/// Model-visible metadata for a tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// A JSON Schema describing the accepted arguments.
    pub input_schema: Value,
}

/// An object-safe host-side tool implementation.
#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn call(&self, arguments: Value) -> Result<Value, ToolError>;
}

/// A tool whose JSON boundary is derived from strongly typed Rust input and output.
///
/// Implementors only describe their identity and domain operation. The blanket
/// [`Tool`] implementation derives the input schema, decodes arguments, and
/// encodes the result.
#[async_trait]
pub trait TypedTool: Send + Sync {
    type Input: DeserializeOwned + JsonSchema + Send;
    type Output: Serialize + Send;

    fn name(&self) -> &'static str;

    fn description(&self) -> &'static str;

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError>;
}

#[async_trait]
impl<T> Tool for T
where
    T: TypedTool,
{
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: schema_for!(T::Input).into(),
        }
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolError> {
        let input = serde_json::from_value(arguments)
            .map_err(|error| ToolError::new(format!("invalid tool arguments: {error}")))?;
        let output = self.invoke(input).await?;
        serde_json::to_value(output).map_err(|error| ToolError::new(format!("failed to encode tool output: {error}")))
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct ToolError {
    pub message: String,
}

impl ToolError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Clone)]
struct ToolEntry {
    definition: ToolDefinition,
    implementation: Arc<dyn Tool>,
}

/// A collection of tools indexed by their stable, model-visible names.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolEntry>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(&mut self, tool: T) -> Result<(), ToolRegistryError>
    where
        T: Tool + 'static,
    {
        self.register_arc(Arc::new(tool))
    }

    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) -> Result<(), ToolRegistryError> {
        let definition = tool.definition();
        if definition.name.trim().is_empty() {
            return Err(ToolRegistryError::EmptyName);
        }
        if self.tools.contains_key(&definition.name) {
            return Err(ToolRegistryError::Duplicate(definition.name));
        }
        self.tools.insert(
            definition.name.clone(),
            ToolEntry {
                definition,
                implementation: tool,
            },
        );
        Ok(())
    }

    /// Returns the definitions captured when each tool was registered.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|entry| entry.definition.clone()).collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).map(|entry| Arc::clone(&entry.implementation))
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolRegistryError {
    #[error("tool names cannot be empty")]
    EmptyName,
    #[error("a tool named `{0}` is already registered")]
    Duplicate(String),
}
