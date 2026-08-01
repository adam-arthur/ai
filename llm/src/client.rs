use std::{error::Error, fmt, sync::OnceLock};

use crate::{
    ImageInput, Model, ModelError, ModelId, ModelMessage, ModelRequest, ModelResponse,
    ModelResponseFormat, async_trait, llama::LlamaClient, model::Backend,
};

static DEFAULT_CLIENT: OnceLock<Client> = OnceLock::new();

/// Asks a model a single user question using backends configured from the
/// process environment.
pub async fn ask(request: AskRequest) -> Result<String, ClientError> {
    default_client()?.ask(request).await
}

/// Options for asking a model a single user question.
#[derive(bon::Builder, Clone, Debug, PartialEq)]
pub struct AskRequest {
    model: ModelId,
    #[builder(into)]
    prompt: String,
    image: Option<ImageInput>,
    response_format: Option<ModelResponseFormat>,
}

impl AskRequest {
    fn into_model_request(self) -> ModelRequest {
        let message = match self.image {
            Some(image) => ModelMessage::user_with_image(self.prompt, image),
            None => ModelMessage::user(self.prompt),
        };

        ModelRequest::builder(self.model, [message])
            .maybe_response_format(self.response_format)
            .build()
    }
}

fn default_client() -> Result<&'static Client, ClientError> {
    if let Some(client) = DEFAULT_CLIENT.get() {
        return Ok(client);
    }

    let client = Client::from_env()?;
    Ok(DEFAULT_CLIENT.get_or_init(|| client))
}

/// Provider-neutral client that routes each model to its backend.
#[derive(Clone, Debug)]
pub(crate) struct Client {
    llama: LlamaClient,
}

impl Client {
    /// Configures all model backends from the process environment.
    fn from_env() -> Result<Self, ClientError> {
        Ok(Self {
            llama: LlamaClient::from_env().map_err(ClientError::new)?,
        })
    }

    /// Sends a single user prompt to the selected model backend.
    async fn ask(&self, request: AskRequest) -> Result<String, ClientError> {
        let response = self.complete_request(request.into_model_request()).await?;
        Ok(response.content)
    }

    async fn complete_request(&self, request: ModelRequest) -> Result<ModelResponse, ClientError> {
        match request.model.backend() {
            Backend::Llama => self.llama.complete(request).await.map_err(ClientError::new),
        }
    }
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

    #[test]
    fn ask_request_builds_a_multimodal_model_request() {
        let request = AskRequest::builder()
            .model(ModelId::GEMMA_4_E2B_Q4)
            .prompt("Describe this image")
            .image(ImageInput::new("image/png", [1, 2, 3]))
            .response_format(ModelResponseFormat::JsonObject)
            .build()
            .into_model_request();

        assert_eq!(request.model, ModelId::GEMMA_4_E2B_Q4);
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content, "Describe this image");
        assert_eq!(
            request.messages[0].image,
            Some(ImageInput::new("image/png", [1, 2, 3]))
        );
        assert_eq!(
            request.response_format,
            Some(ModelResponseFormat::JsonObject)
        );
    }
}
