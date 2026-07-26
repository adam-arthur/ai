use std::{error::Error, fmt};

use crate::{Model, ModelError, ModelId, ModelMessage, ModelRequest, ModelResponse, async_trait, llama::LlamaClient, model::Backend};

/// Provider-neutral client that routes each model to its backend.
#[derive(Clone, Debug)]
pub struct Client {
    llama: LlamaClient,
}

impl Client {
    /// Configures all model backends from the process environment.
    pub fn from_env() -> Result<Self, ClientError> {
        Ok(Self {
            llama: LlamaClient::from_env().map_err(ClientError::new)?,
        })
    }

    /// Sends a single user prompt to the backend selected by `model`.
    pub async fn query(&self, model: ModelId, prompt: impl Into<String>) -> Result<String, ClientError> {
        let response = self
            .complete_request(ModelRequest::builder(model, [ModelMessage::user(prompt)]).build())
            .await?;
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
        self.source.as_deref().map(|source| source as &(dyn Error + 'static))
    }
}
