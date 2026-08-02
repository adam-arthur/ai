use async_trait::async_trait;
use llm_core::{LlmResult, TextGenerator, TextRequest};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Clone)]
pub struct GeminiClient {
    api_key: String,
    base_url: String,
    client: Client,
}

impl GeminiClient {
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            client: Client::new(),
        }
    }
}

#[async_trait]
impl TextGenerator for GeminiClient {
    async fn generate(&self, request: TextRequest) -> LlmResult<String> {
        let mut body = json!({
            "contents": [{"role": "user", "parts": [{"text": request.prompt}]}],
            "generationConfig": {"thinkingConfig": {"thinkingLevel": "LOW"}}
        });
        let object = body.as_object_mut().expect("request body is an object");
        if let Some(system_prompt) = request.system_prompt {
            object.insert(
                "systemInstruction".into(),
                json!({"parts": [{"text": system_prompt}]}),
            );
        }
        let generation = object
            .get_mut("generationConfig")
            .and_then(Value::as_object_mut)
            .expect("generation config is an object");
        if let Some(max_output_tokens) = request.max_output_tokens {
            generation.insert("maxOutputTokens".into(), json!(max_output_tokens));
        }
        if let Some(temperature) = request.temperature {
            generation.insert("temperature".into(), json!(temperature));
        }
        if let Some(schema) = request.json_schema {
            generation.insert("responseMimeType".into(), json!("application/json"));
            generation.insert("responseJsonSchema".into(), schema);
        }

        let response = self
            .client
            .post(format!(
                "{}/models/{}:generateContent",
                self.base_url, request.model
            ))
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let message = serde_json::from_slice::<GeminiErrorBody>(&bytes)
                .ok()
                .map_or_else(
                    || String::from_utf8_lossy(&bytes).into_owned(),
                    |body| body.error.message,
                );
            return Err(GeminiError::Api { status, message }.into());
        }
        let body: Value = serde_json::from_slice(&bytes)?;
        extract_text(&body)
            .ok_or(GeminiError::MissingText)
            .map_err(Into::into)
    }
}

fn extract_text(body: &Value) -> Option<String> {
    let text = body
        .get("candidates")?
        .as_array()?
        .iter()
        .filter_map(|candidate| {
            candidate
                .pointer("/content/parts")
                .and_then(Value::as_array)
        })
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

#[derive(Debug, Error)]
enum GeminiError {
    #[error("Gemini API request failed ({status}): {message}")]
    Api { status: StatusCode, message: String },
    #[error("Gemini response did not contain text")]
    MissingText,
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct GeminiErrorBody {
    error: GeminiErrorMessage,
}

#[derive(Deserialize)]
struct GeminiErrorMessage {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_candidate_text() {
        let body = json!({"candidates": [{"content": {"parts": [{"text": "좋아요"}]}}]});
        assert_eq!(extract_text(&body).as_deref(), Some("좋아요"));
    }
}
