use std::sync::atomic::{AtomicUsize, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tools::{Tool, ToolDefinition, ToolError, ToolRegistry, ToolRegistryError, TypedTool, async_trait};

struct NamedTool(&'static str);

#[async_trait]
impl Tool for NamedTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.0.into(),
            description: format!("The {} tool", self.0),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolError> {
        Ok(arguments)
    }
}

#[test]
fn rejects_empty_and_duplicate_names() {
    let mut registry = ToolRegistry::new();

    assert_eq!(registry.register(NamedTool("  ")), Err(ToolRegistryError::EmptyName));
    registry.register(NamedTool("echo")).unwrap();
    assert_eq!(
        registry.register(NamedTool("echo")),
        Err(ToolRegistryError::Duplicate("echo".into()))
    );
}

#[test]
fn definitions_are_ordered_by_name() {
    let mut registry = ToolRegistry::new();
    registry.register(NamedTool("zebra")).unwrap();
    registry.register(NamedTool("alpha")).unwrap();

    let names: Vec<_> = registry
        .definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect();

    assert_eq!(names, ["alpha", "zebra"]);
}

struct ChangingDefinition(AtomicUsize);

#[async_trait]
impl Tool for ChangingDefinition {
    fn definition(&self) -> ToolDefinition {
        let version = self.0.fetch_add(1, Ordering::Relaxed);
        ToolDefinition {
            name: format!("tool-{version}"),
            description: format!("version {version}"),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(&self, arguments: Value) -> Result<Value, ToolError> {
        Ok(arguments)
    }
}

#[test]
fn definition_is_captured_at_registration() {
    let mut registry = ToolRegistry::new();
    registry.register(ChangingDefinition(AtomicUsize::new(0))).unwrap();

    assert_eq!(registry.definitions()[0].name, "tool-0");
    assert_eq!(registry.definitions()[0].description, "version 0");
    assert!(registry.get("tool-0").is_some());
    assert!(registry.get("tool-1").is_none());
}

struct TypedEcho;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EchoInput {
    message: String,
}

#[derive(Serialize)]
struct EchoOutput {
    message: String,
}

#[async_trait]
impl TypedTool for TypedEcho {
    type Input = EchoInput;
    type Output = EchoOutput;

    fn name(&self) -> &'static str {
        "typed_echo"
    }

    fn description(&self) -> &'static str {
        "Echo typed input"
    }

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output, ToolError> {
        Ok(EchoOutput { message: input.message })
    }
}

#[tokio::test]
async fn typed_tools_derive_their_schema_and_json_boundary() {
    let mut registry = ToolRegistry::new();
    registry.register(TypedEcho).unwrap();

    let definition = &registry.definitions()[0];
    assert_eq!(definition.input_schema["required"], json!(["message"]));
    assert_eq!(definition.input_schema["additionalProperties"], false);

    let tool = registry.get("typed_echo").unwrap();
    let output = tool.call(json!({ "message": "hello" })).await.unwrap();
    assert_eq!(output, json!({ "message": "hello" }));

    let error = tool
        .call(json!({ "message": "hello", "extra": true }))
        .await
        .unwrap_err();
    assert!(error.message.contains("unknown field `extra`"));
}
