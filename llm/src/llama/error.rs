use thiserror::Error;

use crate::openai_compatible::OpenAiCompatibleError;

use super::LlamaConfigError;

#[derive(Debug, Error)]
pub enum LlamaClientError {
    #[error(transparent)]
    Config(#[from] LlamaConfigError),
    #[error(transparent)]
    Client(#[from] OpenAiCompatibleError),
    #[error("model backend returned no choices")]
    EmptyResponse,
}
