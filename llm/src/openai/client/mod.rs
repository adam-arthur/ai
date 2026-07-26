use reqwest::{Client, Url};

use super::{ChatCompletion, ChatRequest};

mod error;

pub use error::OpenAiClientError;

/// HTTP client for an OpenAI-compatible chat completion API.
#[derive(Clone, Debug)]
pub struct OpenAiClient {
    http: Client,
    base_url: Url,
}

impl OpenAiClient {
    #[cfg(test)]
    fn new(server: impl AsRef<str>) -> Result<Self, OpenAiClientError> {
        let server = server.as_ref().trim_end_matches('/');
        let base_url = Url::parse(&format!("{server}/"))?;
        Ok(Self::from_url(base_url))
    }

    pub fn from_url(base_url: Url) -> Self {
        Self {
            http: Client::new(),
            base_url,
        }
    }

    #[cfg(test)]
    pub(crate) fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Sends a structured request to `/v1/chat/completions`.
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatCompletion, OpenAiClientError> {
        let url = self.base_url.join("v1/chat/completions")?;
        let response = self.http.post(url).json(request).send().await?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            return Err(OpenAiClientError::Server {
                status: status.as_u16(),
                body,
            });
        }

        Ok(serde_json::from_str(&body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_normalizes_the_server_url() {
        let client = OpenAiClient::new("http://localhost:8080").unwrap();
        assert_eq!(client.base_url().as_str(), "http://localhost:8080/");
    }
}
