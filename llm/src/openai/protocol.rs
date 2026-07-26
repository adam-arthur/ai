use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[cfg(test)]
impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

/// An OpenAI-compatible chat completion request.
#[derive(bon::Builder, Clone, Debug, PartialEq, Serialize)]
pub struct ChatRequest {
    #[builder(start_fn, into)]
    pub model: String,
    #[builder(start_fn, into)]
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    JsonObject,
    JsonSchema { json_schema: JsonSchemaFormat },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JsonSchemaFormat {
    pub name: String,
    pub strict: bool,
    pub schema: Value,
}

/// The relevant fields returned by an OpenAI-compatible chat completion.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChatCompletion {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ChatMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_uses_the_openai_compatible_shape() {
        let request = ChatRequest::builder(
            "test-model",
            [ChatMessage::system("Be concise"), ChatMessage::user("Hi")],
        )
        .build();

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "model": "test-model",
                "messages": [
                    { "role": "system", "content": "Be concise" },
                    { "role": "user", "content": "Hi" }
                ]
            })
        );
    }

    #[test]
    fn chat_request_serializes_a_json_schema_response_format() {
        let request = ChatRequest::builder("test-model", [ChatMessage::user("Hi")])
            .response_format(ResponseFormat::JsonSchema {
                json_schema: JsonSchemaFormat {
                    name: "test_response".into(),
                    strict: true,
                    schema: serde_json::json!({ "type": "object" }),
                },
            })
            .build();

        let value = serde_json::to_value(request).unwrap();
        assert_eq!(
            value["response_format"],
            serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "test_response",
                    "strict": true,
                    "schema": { "type": "object" }
                }
            })
        );
    }
}
