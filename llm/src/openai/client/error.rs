use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpenAiClientError {
    #[error("invalid OpenAI-compatible server URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("OpenAI-compatible server request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("OpenAI-compatible server returned HTTP {status}: {body}")]
    Server { status: u16, body: String },
    #[error("failed to decode the OpenAI-compatible server response: {0}")]
    Decode(#[from] serde_json::Error),
}
