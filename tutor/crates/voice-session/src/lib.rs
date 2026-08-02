use std::{collections::HashMap, sync::Arc};

use language_tutor::{
    ConversationMessage, KoreanTutor, KoreanTutorLevel, KoreanTutorMistake, ModelConfiguration,
};
use llm_core::Audio;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock, broadcast};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum SessionEvent {
    InputTranscription {
        input_id: Option<String>,
        text: String,
    },
    TurnMistakes {
        input_id: Option<String>,
        mistakes: Vec<KoreanTutorMistake>,
    },
    Text {
        text: String,
    },
    Audio {
        audio: Audio,
    },
    TurnComplete,
    Error {
        message: String,
    },
}

pub struct SessionService {
    tutor: Arc<KoreanTutor>,
    sessions: RwLock<HashMap<Uuid, Arc<Mutex<Session>>>>,
}

impl SessionService {
    #[must_use]
    pub fn new(tutor: Arc<KoreanTutor>) -> Self {
        Self {
            tutor,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create(&self, level: KoreanTutorLevel, models: ModelConfiguration) -> Uuid {
        let id = Uuid::new_v4();
        let (events, _) = broadcast::channel(32);
        self.sessions.write().await.insert(
            id,
            Arc::new(Mutex::new(Session {
                conversation: Vec::new(),
                events,
                level,
                models,
            })),
        );
        id
    }

    /// Subscribes to browser-facing events for a session.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NotFound`] when the session does not exist.
    pub async fn subscribe(
        &self,
        id: Uuid,
    ) -> Result<broadcast::Receiver<SessionEvent>, SessionError> {
        let session = self.session(id).await?;
        Ok(session.lock().await.events.subscribe())
    }

    /// Serially processes and publishes one complete audio turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is missing, the audio is invalid, or tutor processing
    /// fails.
    pub async fn process_audio_turn(
        &self,
        id: Uuid,
        input_id: Option<String>,
        audio: Audio,
    ) -> Result<(), SessionError> {
        let session = self.session(id).await?;
        let mut session = session.lock().await;
        let transcription_audio = to_transcription_audio(audio)?;
        let result = self
            .tutor
            .process_turn(
                session.level,
                session.models,
                &session.conversation,
                transcription_audio,
            )
            .await;

        let result = match result {
            Ok(result) => result,
            Err(error) => {
                let message = error.to_string();
                let _ = session.events.send(SessionEvent::Error {
                    message: message.clone(),
                });
                return Err(SessionError::Turn(message));
            }
        };

        session.conversation.push(ConversationMessage {
            role: language_tutor::ConversationRole::Learner,
            text: result.transcription.clone(),
        });
        let _ = session.events.send(SessionEvent::InputTranscription {
            input_id: input_id.clone(),
            text: result.transcription,
        });
        if !result.mistakes.is_empty() {
            let _ = session.events.send(SessionEvent::TurnMistakes {
                input_id,
                mistakes: result.mistakes,
            });
        }
        session.conversation.push(ConversationMessage {
            role: language_tutor::ConversationRole::Tutor,
            text: result.response_text.clone(),
        });
        let _ = session.events.send(SessionEvent::Text {
            text: result.response_text,
        });
        let _ = session.events.send(SessionEvent::Audio {
            audio: result.response_audio,
        });
        let _ = session.events.send(SessionEvent::TurnComplete);
        Ok(())
    }

    /// Deletes an active session.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NotFound`] when the session does not exist.
    pub async fn delete(&self, id: Uuid) -> Result<(), SessionError> {
        self.sessions
            .write()
            .await
            .remove(&id)
            .map(|_| ())
            .ok_or(SessionError::NotFound)
    }

    async fn session(&self, id: Uuid) -> Result<Arc<Mutex<Session>>, SessionError> {
        self.sessions
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or(SessionError::NotFound)
    }
}

struct Session {
    conversation: Vec<ConversationMessage>,
    events: broadcast::Sender<SessionEvent>,
    level: KoreanTutorLevel,
    models: ModelConfiguration,
}

fn to_transcription_audio(audio: Audio) -> Result<Audio, SessionError> {
    if audio.mime_type.split(';').next() != Some("audio/pcm") {
        return Ok(audio);
    }
    let sample_rate = audio
        .mime_type
        .split(';')
        .find_map(|part| part.strip_prefix("rate="))
        .and_then(|rate| rate.parse::<u32>().ok())
        .filter(|rate| *rate > 0)
        .ok_or_else(|| {
            SessionError::InvalidAudio("PCM audio must include a sample rate".to_owned())
        })?;
    if audio.data.len() % 2 != 0 {
        return Err(SessionError::InvalidAudio(
            "PCM audio must contain complete 16-bit samples".to_owned(),
        ));
    }
    let data_len = u32::try_from(audio.data.len())
        .map_err(|_| SessionError::InvalidAudio("audio turn is too large".to_owned()))?;
    let mut wav = Vec::with_capacity(44 + audio.data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&audio.data);
    Ok(Audio {
        data: wav,
        mime_type: "audio/wav".to_owned(),
    })
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("voice session not found")]
    NotFound,
    #[error("invalid audio: {0}")]
    InvalidAudio(String),
    #[error("voice turn failed: {0}")]
    Turn(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_pcm_in_a_wave_container() {
        let output = to_transcription_audio(Audio {
            data: vec![1, 2, 3, 4],
            mime_type: "audio/pcm;rate=24000".to_owned(),
        })
        .unwrap();
        assert_eq!(&output.data[..4], b"RIFF");
        assert_eq!(&output.data[8..12], b"WAVE");
        assert_eq!(output.mime_type, "audio/wav");
    }
}
