//! Official OpenAI API transport.

use crate::{
    Audio, ModelRequest, ModelResponse, ModelRole, ModelUsage, SpeechSynthesisRequest,
    TranscriptionRequest,
};
use reqwest::{Client, StatusCode, multipart};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Clone, Debug)]
pub(crate) struct OpenAiClient {
    api_key: String,
    base_url: String,
    http: Client,
}

impl OpenAiClient {
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
    ) -> Result<ModelResponse, OpenAiError> {
        let mut instructions = Vec::new();
        let mut input = Vec::new();
        for message in request.messages {
            if message.role == ModelRole::System {
                instructions.push(message.content);
                continue;
            }
            let role = match message.role {
                ModelRole::System => unreachable!(),
                ModelRole::User => "user",
                ModelRole::Assistant => "assistant",
            };
            let content = match message.image {
                Some(image) => json!([
                    {"type": "input_text", "text": message.content},
                    {
                        "type": "input_image",
                        "image_url": format!(
                            "data:{};base64,{}",
                            image.media_type(),
                            base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                image.data()
                            )
                        )
                    }
                ]),
                None => json!(message.content),
            };
            input.push(json!({"role": role, "content": content}));
        }

        let mut body = json!({
            "model": request.model.as_str(),
            "input": input,
            "store": false,
            "reasoning": {"effort": "low"}
        });
        let object = body.as_object_mut().expect("request body is an object");
        if !instructions.is_empty() {
            object.insert("instructions".into(), json!(instructions.join("\n\n")));
        }
        if let Some(max_output_tokens) = request.max_tokens {
            object.insert("max_output_tokens".into(), json!(max_output_tokens));
        }
        if let Some(temperature) = request.temperature {
            object.insert("temperature".into(), json!(temperature));
        }
        if let Some(schema) = request.response_schema {
            let (name, schema) = schema.into_parts();
            object.insert(
                "text".into(),
                json!({
                    "format": {
                        "type": "json_schema",
                        "name": name,
                        "schema": schema,
                        "strict": true
                    }
                }),
            );
        }

        let response = self
            .http
            .post(format!("{}/responses", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let body = self.response_json(response).await?;
        let content = extract_response_text(&body).ok_or(OpenAiError::MissingText)?;
        let usage = body.get("usage").map(|usage| ModelUsage {
            prompt_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
            completion_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
            total_tokens: usage["total_tokens"].as_u64().unwrap_or(0),
        });
        Ok(ModelResponse { content, usage })
    }

    pub(crate) async fn transcribe(
        &self,
        request: TranscriptionRequest,
    ) -> Result<String, OpenAiError> {
        let (file_name, mime_type) = audio_file_metadata(&request.audio.mime_type)?;
        let file = multipart::Part::bytes(request.audio.data)
            .file_name(file_name)
            .mime_str(mime_type)?;
        let mut form = multipart::Form::new()
            .text("model", request.model.as_str())
            .text("response_format", "json")
            .part("file", file);
        if let Some(prompt) = request.prompt {
            form = form.text("prompt", prompt);
        }
        for language in request.language_codes {
            form = form.text("languages[]", language);
        }
        let response = self
            .http
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
    }

    pub(crate) async fn synthesize(
        &self,
        request: SpeechSynthesisRequest,
    ) -> Result<Audio, OpenAiError> {
        let mut body = json!({
            "model": request.model.as_str(),
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
            .http
            .post(format!("{}/audio/speech", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(Audio::new("audio/pcm;rate=24000", bytes.to_vec()))
    }

    async fn response_json(&self, response: reqwest::Response) -> Result<Value, OpenAiError> {
        let status = response.status();
        let body = response.bytes().await?;
        if !status.is_success() {
            return Err(api_error(status, &body));
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

fn api_error(status: StatusCode, body: &[u8]) -> OpenAiError {
    let message = serde_json::from_slice::<OpenAiErrorBody>(body)
        .ok()
        .map_or_else(
            || String::from_utf8_lossy(body).into_owned(),
            |body| body.error.message,
        );
    OpenAiError::Api { status, message }
}

fn extract_response_text(body: &Value) -> Option<String> {
    if let Some(text) = body.get("output_text").and_then(Value::as_str)
        && !text.is_empty()
    {
        return Some(text.to_owned());
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
pub(crate) enum OpenAiError {
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
        let body = json!({"output": [{"content": [{"type": "output_text", "text": "hello"}]}]});
        assert_eq!(extract_response_text(&body).as_deref(), Some("hello"));
    }

    #[test]
    fn maps_browser_audio_file_types() {
        assert_eq!(
            audio_file_metadata("audio/webm;codecs=opus").unwrap().0,
            "audio.webm"
        );
    }
}
