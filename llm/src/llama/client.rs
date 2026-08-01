use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    ModelRequest, ModelResponse, ModelRole, ModelUsage,
    openai::{
        ChatContent, ChatContentPart, ChatMessage, ChatRequest, ImageUrl, JsonSchemaFormat,
        OpenAiClient, ResponseFormat, Role,
    },
};

use super::{LlamaClientError, LlamaConfig};

/// Typed convenience client for the configured Llama server.
#[derive(Clone, Debug)]
pub struct LlamaClient {
    openai: OpenAiClient,
}

impl LlamaClient {
    pub fn from_env() -> Result<Self, LlamaClientError> {
        Ok(Self::from_config(LlamaConfig::from_env()?))
    }

    #[cfg(test)]
    fn new(server: impl AsRef<str>) -> Result<Self, LlamaClientError> {
        Ok(Self::from_config(LlamaConfig::new(server)?))
    }

    pub fn from_config(config: LlamaConfig) -> Self {
        Self {
            openai: OpenAiClient::from_url(config.server().clone()),
        }
    }

    #[cfg(test)]
    fn base_url(&self) -> &reqwest::Url {
        self.openai.base_url()
    }

    pub async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, LlamaClientError> {
        let chat_request = ChatRequest::builder(
            request.model,
            request
                .messages
                .into_iter()
                .map(|message| {
                    let role = match message.role {
                        ModelRole::System => Role::System,
                        ModelRole::User => Role::User,
                        ModelRole::Assistant => Role::Assistant,
                    };
                    let content = match message.image {
                        Some(image) => ChatContent::Parts(vec![
                            ChatContentPart::Text {
                                text: message.content,
                            },
                            ChatContentPart::ImageUrl {
                                image_url: ImageUrl {
                                    url: format!(
                                        "data:{};base64,{}",
                                        image.media_type(),
                                        STANDARD.encode(image.data())
                                    ),
                                },
                            },
                        ]),
                        None => ChatContent::Text(message.content),
                    };

                    ChatMessage { role, content }
                })
                .collect::<Vec<_>>(),
        )
        .maybe_temperature(request.temperature)
        .maybe_max_tokens(request.max_tokens)
        .maybe_response_format(request.response_schema.map(|response_schema| {
            let (name, schema) = response_schema.into_parts();
            ResponseFormat::JsonSchema {
                json_schema: JsonSchemaFormat {
                    name,
                    strict: true,
                    schema: schema.into(),
                },
            }
        }))
        .build();

        let completion = self.openai.chat(&chat_request).await?;
        let choice = completion
            .choices
            .into_iter()
            .next()
            .ok_or(LlamaClientError::EmptyResponse)?;
        let usage = completion.usage.map(|usage| ModelUsage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        });

        Ok(ModelResponse {
            content: choice.message.content.into_text(),
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_normalizes_the_server_url() {
        let client = LlamaClient::new("http://localhost:8080").unwrap();
        assert_eq!(client.base_url().as_str(), "http://localhost:8080/");
    }
}
