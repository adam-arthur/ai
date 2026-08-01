use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};
use thiserror::Error;

use crate::async_trait;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Llama,
}

/// A language model supported by this crate.
///
/// The backend and backend-specific wire name are selected internally.
#[allow(non_camel_case_types)]
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    EnumIter,
    EnumString,
    Hash,
    IntoStaticStr,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
)]
#[strum(parse_err_ty = ParseModelIdError, parse_err_fn = ParseModelIdError::new)]
pub enum ModelId {
    GEMMA_4_E2B_Q4,
    GEMMA_4_E4B_Q4,
    GEMMA_4_12B_Q4,
    GEMMA_4_26B_A4B_Q4,
    GEMMA_4_31B_Q4,
}

impl ModelId {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn iter() -> impl Iterator<Item = Self> {
        <Self as IntoEnumIterator>::iter()
    }

    pub(crate) const fn backend(self) -> Backend {
        match self {
            Self::GEMMA_4_E2B_Q4
            | Self::GEMMA_4_E4B_Q4
            | Self::GEMMA_4_12B_Q4
            | Self::GEMMA_4_26B_A4B_Q4
            | Self::GEMMA_4_31B_Q4 => Backend::Llama,
        }
    }
}

impl From<ModelId> for String {
    fn from(model: ModelId) -> Self {
        model.as_str().to_owned()
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("unsupported model `{0}`")]
pub struct ParseModelIdError(String);

impl ParseModelIdError {
    fn new(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// A provider-neutral interface for querying a language model.
#[async_trait]
pub trait Model: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError>;
}

#[derive(bon::Builder, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    #[builder(start_fn)]
    pub model: ModelId,
    #[builder(start_fn, into)]
    pub messages: Vec<ModelMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub response_format: Option<ModelResponseFormat>,
}

/// A constraint on the model's generated response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ModelResponseFormat {
    JsonObject,
    JsonSchema { name: String, schema: Value },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: ModelRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInput>,
}

impl ModelMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::System,
            content: content.into(),
            image: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            content: content.into(),
            image: None,
        }
    }

    pub fn user_with_image(content: impl Into<String>, image: ImageInput) -> Self {
        Self {
            role: ModelRole::User,
            content: content.into(),
            image: Some(image),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Assistant,
            content: content.into(),
            image: None,
        }
    }
}

/// Raw image bytes and their media type, supplied as model input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInput {
    media_type: String,
    data: Vec<u8>,
}

impl ImageInput {
    pub fn new(media_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelResponse {
    pub content: String,
    pub usage: Option<ModelUsage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct ModelError {
    pub message: String,
}

impl ModelError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_model_round_trips() {
        let wire_names = [
            "GEMMA_4_E2B_Q4",
            "GEMMA_4_E4B_Q4",
            "GEMMA_4_12B_Q4",
            "GEMMA_4_26B_A4B_Q4",
            "GEMMA_4_31B_Q4",
        ];

        for (model, wire_name) in ModelId::iter().zip(wire_names) {
            assert_eq!(model.as_str(), wire_name);
            assert_eq!(model.as_str().parse(), Ok(model));
            assert_eq!(
                serde_json::to_string(&model).unwrap(),
                format!(r#""{model}""#)
            );
        }

        assert_eq!(
            "unsupported model `unknown`",
            "unknown".parse::<ModelId>().unwrap_err().to_string()
        );
    }
}
