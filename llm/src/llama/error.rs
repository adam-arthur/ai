use thiserror::Error;

use crate::openai::OpenAiClientError;

use super::LlamaConfigError;

#[derive(Debug, Error)]
pub enum LlamaClientError {
    #[error(transparent)]
    Config(#[from] LlamaConfigError),
    #[error(transparent)]
    Client(#[from] OpenAiClientError),
    #[error("model backend returned no choices")]
    EmptyResponse,
}
