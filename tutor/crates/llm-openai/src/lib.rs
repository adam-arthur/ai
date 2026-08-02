use async_trait::async_trait;
use llm_core::{
    Audio, LlmResult, SpeechSynthesisRequest, SpeechSynthesizer, SpeechTranscriber, TextGenerator,
    TextRequest, TranscriptionRequest,
};
use reqwest::{Client, StatusCode, multipart};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Clone)]
pub struct OpenAiClient {
    api_key: String,
    base_url: String,
    client: Client,
}

impl OpenAiClient {
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            client: Client::new(),
        }
    }

    #[cfg(test)]
    fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
            client: Client::new(),
        }
    }

    async fn response_json(&self, response: reqwest::Response) -> Result<Value, OpenAiError> {
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            let message = serde_json::from_slice::<OpenAiErrorBody>(&body)
                .ok()
                .map_or_else(
                    || String::from_utf8_lossy(&body).into_owned(),
                    |body| body.error.message,
                );
            return Err(OpenAiError::Api { status, message });
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

#[async_trait]
impl TextGenerator for OpenAiClient {
    async fn generate(&self, request: TextRequest) -> LlmResult<String> {
        let mut body = json!({
            "model": request.model,
            "input": request.prompt,
            "store": false,
            "reasoning": { "effort": "low" }
        });
        let object = body.as_object_mut().expect("request body is an object");
        if let Some(instructions) = request.system_prompt {
            object.insert("instructions".into(), json!(instructions));
        }
        if let Some(max_output_tokens) = request.max_output_tokens {
            object.insert("max_output_tokens".into(), json!(max_output_tokens));
        }
        if let Some(temperature) = request.temperature {
            object.insert("temperature".into(), json!(temperature));
        }
        if let Some(schema) = request.json_schema {
            object.insert(
                "text".into(),
                json!({
                    "format": {
                        "type": "json_schema",
                        "name": "response",
                        "schema": schema,
                        "strict": true
                    }
                }),
            );
        }

        let response = self
            .client
            .post(format!("{}/responses", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let body = self.response_json(response).await?;
        extract_response_text(&body)
            .ok_or(OpenAiError::MissingText)
            .map_err(Into::into)
    }
}

#[async_trait]
impl SpeechTranscriber for OpenAiClient {
    async fn transcribe(&self, request: TranscriptionRequest) -> LlmResult<String> {
        let (file_name, mime_type) = audio_file_metadata(&request.audio.mime_type)?;
        let file = multipart::Part::bytes(request.audio.data)
            .file_name(file_name)
            .mime_str(mime_type)?;
        let mut form = multipart::Form::new()
            .text("model", request.model)
            .text("response_format", "json")
            .part("file", file);
        if let Some(prompt) = request.prompt {
            form = form.text("prompt", prompt);
        }
        if let Some(language) = request.language_code {
            form = form.text("language", language);
        }
        let response = self
            .client
            .post(format!("{}/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;
        let body = self.response_json(response).await?;
        body.get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(OpenAiError::MissingText)
            .map_err(Into::into)
    }
}

#[async_trait]
impl SpeechSynthesizer for OpenAiClient {
    async fn synthesize(&self, request: SpeechSynthesisRequest) -> LlmResult<Audio> {
        let mut body = json!({
            "model": request.model,
            "input": request.text,
            "voice": request.voice,
            "response_format": "pcm"
        });
        if let Some(instructions) = request.instructions {
            body.as_object_mut()
                .expect("request body is an object")
                .insert("instructions".into(), json!(instructions));
        }
        let response = self
            .client
            .post(format!("{}/audio/speech", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let message = serde_json::from_slice::<OpenAiErrorBody>(&bytes)
                .ok()
                .map_or_else(
                    || String::from_utf8_lossy(&bytes).into_owned(),
                    |body| body.error.message,
                );
            return Err(OpenAiError::Api { status, message }.into());
        }
        Ok(Audio {
            data: bytes.to_vec(),
            mime_type: "audio/pcm;rate=24000".to_owned(),
        })
    }
}

fn extract_response_text(body: &Value) -> Option<String> {
    if let Some(text) = body.get("output_text").and_then(Value::as_str) {
        if !text.is_empty() {
            return Some(text.to_owned());
        }
    }
    let text = body
        .get("output")?
        .as_array()?
        .iter()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

fn audio_file_metadata(mime_type: &str) -> Result<(&'static str, &'static str), OpenAiError> {
    match mime_type.split(';').next().unwrap_or(mime_type) {
        "audio/flac" | "audio/x-flac" => Ok(("audio.flac", "audio/flac")),
        "audio/m4a" | "audio/mp4" | "audio/x-m4a" => Ok(("audio.m4a", "audio/mp4")),
        "audio/mpeg" | "audio/mp3" | "audio/mpga" => Ok(("audio.mp3", "audio/mpeg")),
        "audio/ogg" => Ok(("audio.ogg", "audio/ogg")),
        "audio/wav" | "audio/wave" | "audio/x-wav" => Ok(("audio.wav", "audio/wav")),
        "audio/webm" => Ok(("audio.webm", "audio/webm")),
        other => Err(OpenAiError::UnsupportedAudio(other.to_owned())),
    }
}

#[derive(Debug, Error)]
enum OpenAiError {
    #[error("OpenAI API request failed ({status}): {message}")]
    Api { status: StatusCode, message: String },
    #[error("OpenAI response did not contain text")]
    MissingText,
    #[error("unsupported transcription audio type: {0}")]
    UnsupportedAudio(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    error: OpenAiErrorMessage,
}

#[derive(Deserialize)]
struct OpenAiErrorMessage {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_nested_responses_api_text() {
        let body =
            json!({"output": [{"content": [{"type": "output_text", "text": "안녕하세요"}]}]});
        assert_eq!(extract_response_text(&body).as_deref(), Some("안녕하세요"));
    }

    #[test]
    fn maps_browser_audio_file_types() {
        assert_eq!(
            audio_file_metadata("audio/webm;codecs=opus").unwrap().0,
            "audio.webm"
        );
    }

    #[test]
    fn constructs_test_client() {
        let client = OpenAiClient::with_base_url("key", "http://localhost");
        assert_eq!(client.base_url, "http://localhost");
    }
}
