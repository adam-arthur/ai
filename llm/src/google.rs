//! Google Gemini API transport.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{ModelRequest, ModelResponse, ModelRole, ModelUsage};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Clone, Debug)]
pub(crate) struct GeminiClient {
    api_key: String,
    base_url: String,
    http: Client,
}

impl GeminiClient {
    pub(crate) fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            http: Client::new(),
        }
    }

    pub(crate) async fn complete(
        &self,
        request: ModelRequest,
    ) -> Result<ModelResponse, GeminiError> {
        let mut system = Vec::new();
        let mut contents = Vec::new();
        for message in request.messages {
            if message.role == ModelRole::System {
                system.push(message.content);
                continue;
            }
            let role = match message.role {
                ModelRole::System => unreachable!(),
                ModelRole::User => "user",
                ModelRole::Assistant => "model",
            };
            let mut parts = vec![json!({"text": message.content})];
            if let Some(image) = message.image {
                parts.push(json!({
                    "inlineData": {
                        "mimeType": image.media_type(),
                        "data": STANDARD.encode(image.data())
                    }
                }));
            }
            contents.push(json!({"role": role, "parts": parts}));
        }

        let mut body = json!({
            "contents": contents,
            "generationConfig": {"thinkingConfig": {"thinkingLevel": "LOW"}}
        });
        let object = body.as_object_mut().expect("request body is an object");
        if !system.is_empty() {
            object.insert(
                "systemInstruction".into(),
                json!({"parts": [{"text": system.join("\n\n")}]}),
            );
        }
        let generation = object["generationConfig"]
            .as_object_mut()
            .expect("generation config is an object");
        if let Some(max_output_tokens) = request.max_tokens {
            generation.insert("maxOutputTokens".into(), json!(max_output_tokens));
        }
        if let Some(temperature) = request.temperature {
            generation.insert("temperature".into(), json!(temperature));
        }
        if let Some(schema) = request.response_schema {
            generation.insert("responseMimeType".into(), json!("application/json"));
            generation.insert("responseJsonSchema".into(), schema.schema().clone().into());
        }

        let response = self
            .http
            .post(format!(
                "{}/models/{}:generateContent",
                self.base_url,
                request.model.as_str()
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
            return Err(GeminiError::Api { status, message });
        }
        let body: Value = serde_json::from_slice(&bytes)?;
        let content = extract_text(&body).ok_or(GeminiError::MissingText)?;
        let usage = body.get("usageMetadata").map(|usage| ModelUsage {
            prompt_tokens: usage["promptTokenCount"].as_u64().unwrap_or(0),
            completion_tokens: usage["candidatesTokenCount"].as_u64().unwrap_or(0),
            total_tokens: usage["totalTokenCount"].as_u64().unwrap_or(0),
        });
        Ok(ModelResponse { content, usage })
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
pub(crate) enum GeminiError {
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
        let body = json!({"candidates": [{"content": {"parts": [{"text": "hello"}]}}]});
        assert_eq!(extract_text(&body).as_deref(), Some("hello"));
    }
}
