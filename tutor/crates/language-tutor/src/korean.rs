use std::{fmt::Write as _, sync::Arc};

use llm_core::{
    Audio, SpeechSynthesisRequest, SpeechSynthesizer, SpeechTranscriber, TextGenerator,
    TextRequest, TranscriptionRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
pub enum KoreanTutorLevel {
    A1,
    A2,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
pub enum TextModel {
    #[serde(rename = "gemini-3.1-flash-lite")]
    #[ts(rename = "gemini-3.1-flash-lite")]
    Gemini31FlashLite,
    #[serde(rename = "gemini-3.5-flash")]
    #[ts(rename = "gemini-3.5-flash")]
    Gemini35Flash,
    #[serde(rename = "gpt-5.5")]
    #[ts(rename = "gpt-5.5")]
    Gpt55,
}

impl TextModel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gemini31FlashLite => "gemini-3.1-flash-lite",
            Self::Gemini35Flash => "gemini-3.5-flash",
            Self::Gpt55 => "gpt-5.5",
        }
    }

    const fn is_openai(self) -> bool {
        matches!(self, Self::Gpt55)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
pub enum TranscriptionModel {
    #[serde(rename = "gpt-4o-mini-transcribe")]
    #[ts(rename = "gpt-4o-mini-transcribe")]
    Gpt4oMiniTranscribe,
    #[serde(rename = "gpt-4o-transcribe")]
    #[ts(rename = "gpt-4o-transcribe")]
    Gpt4oTranscribe,
}

impl TranscriptionModel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gpt4oMiniTranscribe => "gpt-4o-mini-transcribe",
            Self::Gpt4oTranscribe => "gpt-4o-transcribe",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
pub enum SpeechSynthesisModel {
    #[serde(rename = "tts-1")]
    #[ts(rename = "tts-1")]
    Tts1,
}

impl SpeechSynthesisModel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tts1 => "tts-1",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ModelConfiguration {
    pub mistake_detection: TextModel,
    pub reply: TextModel,
    pub speech_synthesis: SpeechSynthesisModel,
    pub transcription: TranscriptionModel,
}

impl Default for ModelConfiguration {
    fn default() -> Self {
        Self {
            mistake_detection: TextModel::Gemini31FlashLite,
            reply: TextModel::Gemini31FlashLite,
            speech_synthesis: SpeechSynthesisModel::Tts1,
            transcription: TranscriptionModel::Gpt4oMiniTranscribe,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename_all = "lowercase")]
pub enum KoreanTutorMistakeKind {
    Grammar,
    Vocabulary,
    Politeness,
    Naturalness,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct KoreanTutorMistake {
    pub kind: KoreanTutorMistakeKind,
    pub original: String,
    pub correction: String,
    pub explanation: String,
}

#[derive(Clone, Debug)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub text: String,
}

#[derive(Clone, Copy, Debug)]
pub enum ConversationRole {
    Learner,
    Tutor,
}

#[derive(Debug)]
pub struct TurnResult {
    pub transcription: String,
    pub mistakes: Vec<KoreanTutorMistake>,
    pub response_text: String,
    pub response_audio: Audio,
}

pub struct KoreanTutor {
    google_text: Arc<dyn TextGenerator>,
    openai_text: Arc<dyn TextGenerator>,
    transcriber: Arc<dyn SpeechTranscriber>,
    synthesizer: Arc<dyn SpeechSynthesizer>,
}

impl KoreanTutor {
    #[must_use]
    pub fn new(
        google_text: Arc<dyn TextGenerator>,
        openai_text: Arc<dyn TextGenerator>,
        transcriber: Arc<dyn SpeechTranscriber>,
        synthesizer: Arc<dyn SpeechSynthesizer>,
    ) -> Self {
        Self {
            google_text,
            openai_text,
            transcriber,
            synthesizer,
        }
    }

    /// Processes one complete learner audio turn.
    ///
    /// # Errors
    ///
    /// Returns an error when a provider request fails, mistake JSON is invalid, or the tutor
    /// produces no response text.
    pub async fn process_turn(
        &self,
        level: KoreanTutorLevel,
        models: ModelConfiguration,
        conversation: &[ConversationMessage],
        audio: Audio,
    ) -> Result<TurnResult, TutorError> {
        let transcription = self
            .transcriber
            .transcribe(TranscriptionRequest {
                model: models.transcription.as_str().to_owned(),
                audio,
                prompt: Some(
                    "The audio may contain Korean learner speech, Hangul, romanized Korean, and occasional English."
                        .to_owned(),
                ),
                language_code: None,
            })
            .await
            .map_err(TutorError::Provider)?
            .trim()
            .to_owned();

        let previous_tutor_text = conversation
            .iter()
            .rev()
            .find(|message| matches!(message.role, ConversationRole::Tutor))
            .map(|message| message.text.as_str());
        let mistakes = self
            .detect_mistakes(
                level,
                models.mistake_detection,
                &transcription,
                previous_tutor_text,
            )
            .await?;
        let prompt = response_prompt(conversation, &transcription, &mistakes);
        let response_text = self
            .text_generator(models.reply)
            .generate(TextRequest {
                model: models.reply.as_str().to_owned(),
                system_prompt: Some(system_prompt(level).to_owned()),
                prompt,
                json_schema: None,
                max_output_tokens: Some(350),
                temperature: None,
            })
            .await
            .map_err(TutorError::Provider)?
            .trim()
            .to_owned();
        if response_text.is_empty() {
            return Err(TutorError::EmptyResponse);
        }
        let response_audio = self
            .synthesizer
            .synthesize(SpeechSynthesisRequest {
                model: models.speech_synthesis.as_str().to_owned(),
                text: response_text.clone(),
                voice: "nova".to_owned(),
                instructions: None,
            })
            .await
            .map_err(TutorError::Provider)?;

        Ok(TurnResult {
            transcription,
            mistakes,
            response_text,
            response_audio,
        })
    }

    async fn detect_mistakes(
        &self,
        level: KoreanTutorLevel,
        model: TextModel,
        transcription: &str,
        previous_tutor_text: Option<&str>,
    ) -> Result<Vec<KoreanTutorMistake>, TutorError> {
        if transcription.trim().is_empty() {
            return Ok(Vec::new());
        }
        let prompt = format!(
            "Learner level: {level:?}\n{}\nLearner transcript:\n{transcription}",
            previous_tutor_text
                .map(|text| format!("\nPrevious tutor message:\n{text}\n"))
                .unwrap_or_default()
        );
        let text = self
            .text_generator(model)
            .generate(TextRequest {
                model: model.as_str().to_owned(),
                system_prompt: Some(MISTAKE_SYSTEM_PROMPT.to_owned()),
                prompt,
                json_schema: Some(mistake_schema()),
                max_output_tokens: Some(300),
                temperature: None,
            })
            .await
            .map_err(TutorError::Provider)?;
        let response: MistakeResponse =
            serde_json::from_str(&text).map_err(TutorError::InvalidMistakes)?;
        Ok(response.mistakes)
    }

    fn text_generator(&self, model: TextModel) -> &Arc<dyn TextGenerator> {
        if model.is_openai() {
            &self.openai_text
        } else {
            &self.google_text
        }
    }
}

#[derive(Deserialize)]
struct MistakeResponse {
    mistakes: Vec<KoreanTutorMistake>,
}

fn mistake_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "mistakes": {
                "type": "array",
                "maxItems": 2,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": ["grammar", "vocabulary", "politeness", "naturalness"]},
                        "original": {"type": "string"},
                        "correction": {"type": "string"},
                        "explanation": {"type": "string"}
                    },
                    "required": ["kind", "original", "correction", "explanation"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["mistakes"],
        "additionalProperties": false
    })
}

fn response_prompt(
    conversation: &[ConversationMessage],
    transcription: &str,
    mistakes: &[KoreanTutorMistake],
) -> String {
    let mut output = String::new();
    if !mistakes.is_empty() {
        output.push_str("Internal tutor note:\nThe learner made these notable Korean mistakes. Briefly correct them if helpful before continuing the conversation:\n");
        for mistake in mistakes {
            let _ = writeln!(
                output,
                "- {} -> {}: {}",
                mistake.original, mistake.correction, mistake.explanation
            );
        }
        output.push('\n');
    }
    let previous = conversation.iter().rev().take(8).collect::<Vec<_>>();
    if !previous.is_empty() {
        output.push_str("Previous conversation:\n");
        for message in previous.into_iter().rev() {
            let role = match message.role {
                ConversationRole::Learner => "Learner",
                ConversationRole::Tutor => "Tutor",
            };
            let _ = writeln!(output, "{role}: {}", message.text);
        }
        output.push('\n');
    }
    let _ = write!(
        output,
        "Learner transcript:\n{}\n\nWrite the tutor's next spoken response.",
        if transcription.is_empty() {
            "[No clear transcription]"
        } else {
            transcription
        }
    );
    output
}

const fn system_prompt(level: KoreanTutorLevel) -> &'static str {
    match level {
        KoreanTutorLevel::A1 => A1_SYSTEM_PROMPT,
        KoreanTutorLevel::A2 => A2_SYSTEM_PROMPT,
    }
}

const A1_SYSTEM_PROMPT: &str = r"ROLE: A1 Korean language tutor for short, warm, and encouraging voice conversations.

CONSTRAINTS:
- Use basic Korean for conversational flow. Use concise English only for corrections, then return to Korean.
- Use present tense, basic particles, and simple polite endings.
- Stay with survival topics and keep the total output brief for speech.

For every turn: briefly correct an important error if needed, respond naturally, then end with exactly one simple Korean question. If the learner struggles, offer two simple Korean options or a fill-in-the-blank prompt.";

const A2_SYSTEM_PROMPT: &str = r"You are a Korean language tutor speaking with an A2 learner.

Teach through a natural voice conversation. Use mostly Korean with brief English explanations only when useful. Discuss familiar daily topics. Ask one clear question at a time and encourage two or three connected sentences. Correct important errors with a natural Korean version and one short explanation. Keep the learner speaking Korean and end most turns with a follow-up question.";

const MISTAKE_SYSTEM_PROMPT: &str = r"You identify Korean language mistakes in a learner's spoken turn.

Identify clear grammar, vocabulary, politeness, or naturalness errors. Do not flag minor spoken quirks. Prioritize at most two issues useful for the learner's level. If the transcript is communicable without obvious errors or too unclear to correct confidently, return no mistakes. Explanations must be concise and in English. Return only the requested JSON.";

#[derive(Debug, Error)]
pub enum TutorError {
    #[error("language model request failed: {0}")]
    Provider(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("mistake detection returned invalid JSON: {0}")]
    InvalidMistakes(#[source] serde_json::Error),
    #[error("the tutor returned an empty response")]
    EmptyResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_models_match_the_reliable_profile() {
        let models = ModelConfiguration::default();
        assert_eq!(models.reply.as_str(), "gemini-3.1-flash-lite");
        assert_eq!(models.transcription.as_str(), "gpt-4o-mini-transcribe");
    }

    #[test]
    fn response_prompt_limits_history() {
        let conversation = (0..10)
            .map(|index| ConversationMessage {
                role: ConversationRole::Learner,
                text: format!("message-{index}"),
            })
            .collect::<Vec<_>>();
        let prompt = response_prompt(&conversation, "안녕하세요", &[]);
        assert!(!prompt.contains("message-0"));
        assert!(prompt.contains("message-9"));
    }
}
