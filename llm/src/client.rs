use std::{error::Error, fmt, sync::OnceLock};

use crate::{
    Model, ModelError, ModelId, ModelMessage, ModelRequest, ModelResponse, async_trait,
    llama::LlamaClient, model::Backend,
};

static DEFAULT_CLIENT: OnceLock<Client> = OnceLock::new();

/// Asks a model a single user question using backends configured from the
/// process environment.
pub async fn ask(model: ModelId, prompt: impl Into<String>) -> Result<String, ClientError> {
    default_client()?.ask(model, prompt).await
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

    /// Sends a single user prompt to the backend selected by `model`.
    async fn ask(&self, model: ModelId, prompt: impl Into<String>) -> Result<String, ClientError> {
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
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}
