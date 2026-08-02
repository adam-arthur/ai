use serde::{Deserialize, Serialize};

/// Encoded or raw audio bytes and their media type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Audio {
    pub data: Vec<u8>,
    pub mime_type: String,
}

impl Audio {
    pub fn new(mime_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptionModelId {
    #[serde(rename = "gpt-4o-mini-transcribe")]
    Gpt4oMiniTranscribe,
    #[serde(rename = "gpt-4o-transcribe")]
    Gpt4oTranscribe,
}

impl TranscriptionModelId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gpt4oMiniTranscribe => "gpt-4o-mini-transcribe",
            Self::Gpt4oTranscribe => "gpt-4o-transcribe",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TranscriptionRequest {
    pub model: TranscriptionModelId,
    pub audio: Audio,
    pub prompt: Option<String>,
    pub language_code: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpeechSynthesisModelId {
    #[serde(rename = "tts-1")]
    Tts1,
}

impl SpeechSynthesisModelId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tts1 => "tts-1",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SpeechSynthesisRequest {
    pub model: SpeechSynthesisModelId,
    pub text: String,
    pub voice: String,
    pub instructions: Option<String>,
}
