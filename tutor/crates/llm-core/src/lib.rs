use async_trait::async_trait;
use serde_json::Value;

pub type LlmResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug)]
pub struct Audio {
    pub data: Vec<u8>,
    pub mime_type: String,
}

#[derive(Clone, Debug)]
pub struct TextRequest {
    pub model: String,
    pub system_prompt: Option<String>,
    pub prompt: String,
    pub json_schema: Option<Value>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct TranscriptionRequest {
    pub model: String,
    pub audio: Audio,
    pub prompt: Option<String>,
    pub language_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SpeechSynthesisRequest {
    pub model: String,
    pub text: String,
    pub voice: String,
    pub instructions: Option<String>,
}

#[async_trait]
pub trait TextGenerator: Send + Sync {
    async fn generate(&self, request: TextRequest) -> LlmResult<String>;
}

#[async_trait]
pub trait SpeechTranscriber: Send + Sync {
    async fn transcribe(&self, request: TranscriptionRequest) -> LlmResult<String>;
}

#[async_trait]
pub trait SpeechSynthesizer: Send + Sync {
    async fn synthesize(&self, request: SpeechSynthesisRequest) -> LlmResult<Audio>;
}
