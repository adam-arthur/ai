//! Transport types for OpenAI-compatible chat completion servers.

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Debug)]
pub(crate) struct OpenAiCompatibleClient {
    http: Client,
    base_url: Url,
}

impl OpenAiCompatibleClient {
    pub(crate) fn from_url(base_url: Url) -> Self {
        Self {
            http: Client::new(),
            base_url,
        }
    }

    #[cfg(test)]
    pub(crate) fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub(crate) async fn chat(
        &self,
        request: &ChatRequest,
    ) -> Result<ChatCompletion, OpenAiCompatibleError> {
        let url = self.base_url.join("v1/chat/completions")?;
        let response = self.http.post(url).json(request).send().await?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            return Err(OpenAiCompatibleError::Server {
                status: status.as_u16(),
                body,
            });
        }

        Ok(serde_json::from_str(&body)?)
    }
}

#[derive(Debug, Error)]
pub(crate) enum OpenAiCompatibleError {
    #[error("invalid OpenAI-compatible server URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("OpenAI-compatible server request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("OpenAI-compatible server returned HTTP {status}: {body}")]
    Server { status: u16, body: String },
    #[error("failed to decode the OpenAI-compatible server response: {0}")]
    Decode(#[from] serde_json::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatMessage {
    pub role: Role,
    pub content: ChatContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

impl ChatContent {
    pub(crate) fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Parts(parts) => parts
                .into_iter()
                .filter_map(|part| match part {
                    ChatContentPart::Text { text } => Some(text),
                    ChatContentPart::ImageUrl { .. } => None,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ChatContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImageUrl {
    pub url: String,
}

#[derive(bon::Builder, Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ChatRequest {
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
pub(crate) enum ResponseFormat {
    JsonSchema { json_schema: JsonSchemaFormat },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct JsonSchemaFormat {
    pub name: String,
    pub strict: bool,
    pub schema: Value,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct ChatCompletion {
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct Usage {
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
            [ChatMessage {
                role: Role::User,
                content: ChatContent::Text("Hi".into()),
            }],
        )
        .build();

        assert_eq!(
            serde_json::to_value(request).unwrap()["messages"][0],
            serde_json::json!({"role": "user", "content": "Hi"})
        );
    }
}
