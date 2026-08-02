use std::{env, error::Error, fmt, sync::OnceLock};

use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::{
    Audio, ImageInput, Model, ModelError, ModelId, ModelMessage, ModelRequest, ModelResponse,
    ModelResponseSchema, SpeechSynthesisRequest, TranscriptionRequest, async_trait,
    google::GeminiClient, llama::LlamaClient, model::Backend, openai::OpenAiClient,
};

static DEFAULT_CLIENT: OnceLock<Client> = OnceLock::new();

/// Asks a model a single user question using backends configured from the
/// process environment.
pub async fn ask<T>(request: AskRequest) -> Result<T, ClientError>
where
    T: JsonSchema + DeserializeOwned,
{
    default_client().ask(request).await
}

/// Completes a provider-neutral model request using process environment configuration.
pub async fn complete(request: ModelRequest) -> Result<ModelResponse, ClientError> {
    default_client().complete_request(request).await
}

/// Transcribes audio using the configured OpenAI API credentials.
pub async fn transcribe(request: TranscriptionRequest) -> Result<String, ClientError> {
    default_client().transcribe(request).await
}

/// Synthesizes speech using the configured OpenAI API credentials.
pub async fn synthesize(request: SpeechSynthesisRequest) -> Result<Audio, ClientError> {
    default_client().synthesize(request).await
}

/// Options for asking a model a single user question.
#[derive(bon::Builder, Clone, Debug, PartialEq)]
pub struct AskRequest {
    model: ModelId,
    #[builder(into)]
    prompt: String,
    image: Option<ImageInput>,
}

impl AskRequest {
    fn into_model_request<T: JsonSchema>(self) -> ModelRequest {
        let message = match self.image {
            Some(image) => ModelMessage::user_with_image(self.prompt, image),
            None => ModelMessage::user(self.prompt),
        };

        ModelRequest::builder(self.model, [message])
            .response_schema(ModelResponseSchema::for_type::<T>())
            .build()
    }
}

fn default_client() -> &'static Client {
    DEFAULT_CLIENT.get_or_init(Client::new)
}

/// Provider-neutral client that routes each model to its backend.
#[derive(Debug)]
pub(crate) struct Client {
    gemini: OnceLock<GeminiClient>,
    llama: OnceLock<LlamaClient>,
    openai: OnceLock<OpenAiClient>,
}

impl Client {
    const fn new() -> Self {
        Self {
            gemini: OnceLock::new(),
            llama: OnceLock::new(),
            openai: OnceLock::new(),
        }
    }

    /// Sends a single user prompt to the selected model backend.
    async fn ask<T>(&self, request: AskRequest) -> Result<T, ClientError>
    where
        T: JsonSchema + DeserializeOwned,
    {
        let response = self
            .complete_request(request.into_model_request::<T>())
            .await?;
        serde_json::from_str(&response.content).map_err(ClientError::new)
    }

    async fn complete_request(&self, request: ModelRequest) -> Result<ModelResponse, ClientError> {
        match request.model.backend() {
            Backend::Gemini => self
                .gemini()?
                .complete(request)
                .await
                .map_err(ClientError::new),
            Backend::Llama => self
                .llama()?
                .complete(request)
                .await
                .map_err(ClientError::new),
            Backend::OpenAi => self
                .openai()?
                .complete(request)
                .await
                .map_err(ClientError::new),
        }
    }

    async fn transcribe(&self, request: TranscriptionRequest) -> Result<String, ClientError> {
        self.openai()?
            .transcribe(request)
            .await
            .map_err(ClientError::new)
    }

    async fn synthesize(&self, request: SpeechSynthesisRequest) -> Result<Audio, ClientError> {
        self.openai()?
            .synthesize(request)
            .await
            .map_err(ClientError::new)
    }

    fn gemini(&self) -> Result<&GeminiClient, ClientError> {
        if let Some(client) = self.gemini.get() {
            return Ok(client);
        }
        let api_key = required_env("GEMINI_API_KEY")?;
        Ok(self.gemini.get_or_init(|| GeminiClient::new(api_key)))
    }

    fn llama(&self) -> Result<&LlamaClient, ClientError> {
        if let Some(client) = self.llama.get() {
            return Ok(client);
        }
        let client = LlamaClient::from_env().map_err(ClientError::new)?;
        Ok(self.llama.get_or_init(|| client))
    }

    fn openai(&self) -> Result<&OpenAiClient, ClientError> {
        if let Some(client) = self.openai.get() {
            return Ok(client);
        }
        let api_key = required_env("OPENAI_API_KEY")?;
        Ok(self.openai.get_or_init(|| OpenAiClient::new(api_key)))
    }
}

fn required_env(name: &'static str) -> Result<String, ClientError> {
    env::var(name).map_err(|source| ClientError {
        message: format!("failed to read `{name}`: {source}"),
        source: Some(Box::new(source)),
    })
}

#[async_trait]
impl Model for Client {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        self.complete_request(request)
            .await
            .map_err(|error| ModelError::new(error.to_string()))
    }
}

/// An error produced while configuring or querying a model.
#[derive(Debug)]
pub struct ClientError {
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl ClientError {
    fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            message: source.to_string(),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, JsonSchema)]
    struct TestResponse {
        description: String,
    }

    #[test]
    fn ask_request_builds_a_multimodal_model_request() {
        let request = AskRequest::builder()
            .model(ModelId::GEMMA_4_E2B_Q4)
            .prompt("Describe this image")
            .image(ImageInput::new("image/png", [1, 2, 3]))
            .build()
            .into_model_request::<TestResponse>();

        assert_eq!(request.model, ModelId::GEMMA_4_E2B_Q4);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content, "Describe this image");
        assert_eq!(
            request.messages[0].image,
            Some(ImageInput::new("image/png", [1, 2, 3]))
        );
        let response_schema = request.response_schema.unwrap();
        assert_eq!(response_schema.name(), "TestResponse");
        assert_eq!(response_schema.schema().as_value()["type"], "object");
        assert!(response_schema.schema().as_value()["properties"]["description"].is_object());

        let decoded: TestResponse =
            serde_json::from_str(r#"{"description":"A test image"}"#).unwrap();
        assert_eq!(decoded.description, "A test image");
    }
}
