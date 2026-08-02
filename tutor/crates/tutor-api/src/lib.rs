use std::{convert::Infallible, path::Path, sync::Arc};

use axum::{Json, Router, extract::{DefaultBodyLimit, Path as AxumPath, State}, http::StatusCode, response::{IntoResponse, Response, Sse, sse::Event}, routing::{delete, get, post}};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use language_tutor::{KoreanTutorLevel, KoreanTutorMistake, KoreanTutorMistakeKind, ModelConfiguration, SpeechSynthesisModel, TextModel, TranscriptionModel};
use llm::Audio;
use serde::{Deserialize, Serialize};
use tokio_stream::{StreamExt as _, wrappers::BroadcastStream};
use ts_rs::TS;
use uuid::Uuid;
use voice_session::{SessionError, SessionEvent, SessionService};

const MAX_AUDIO_TURN_BYTES: usize = 12 * 1024 * 1024;

pub fn router(sessions: Arc<SessionService>) -> Router {
    Router::new()
        .route("/api/turn-voice-sessions", post(create_session))
        .route("/api/turn-voice-sessions/{id}", delete(delete_session))
        .route("/api/turn-voice-sessions/{id}/events", get(session_events))
        .route("/api/turn-voice-sessions/{id}/audio-turns", post(send_audio_turn))
        .layer(DefaultBodyLimit::max(MAX_AUDIO_TURN_BYTES))
        .with_state(sessions)
}

async fn create_session(
    State(sessions): State<Arc<SessionService>>, Json(request): Json<CreateSessionRequest>,
) -> Json<CreateSessionResponse> {
    Json(CreateSessionResponse {
        id: sessions.create(request.level, request.model_configuration).await,
    })
}

async fn delete_session(
    State(sessions): State<Arc<SessionService>>, AxumPath(id): AxumPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    sessions.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn send_audio_turn(
    State(sessions): State<Arc<SessionService>>, AxumPath(id): AxumPath<Uuid>,
    Json(request): Json<SendAudioTurnRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    let data = STANDARD
        .decode(request.audio.data)
        .map_err(|_| ApiError::BadRequest("audio data is not valid base64".to_owned()))?;
    if data.is_empty() {
        return Err(ApiError::BadRequest("audio data is empty".to_owned()));
    }
    sessions
        .process_audio_turn(
            id,
            request.input_id,
            Audio {
                data,
                mime_type: request.audio.mime_type,
            },
        )
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn session_events(
    State(sessions): State<Arc<SessionService>>, AxumPath(id): AxumPath<Uuid>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let receiver = sessions.subscribe(id).await?;
    let stream = BroadcastStream::new(receiver).filter_map(|result| match result {
        Ok(event) => Some(Ok(Event::default()
            .json_data(ClientEvent::from(event))
            .expect("client events serialize"))),
        Err(_) => None,
    });
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub level: KoreanTutorLevel,
    pub model_configuration: ModelConfiguration,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct CreateSessionResponse {
    pub id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SendAudioTurnRequest {
    pub audio: AudioPayload,
    pub input_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AudioPayload {
    pub data: String,
    pub mime_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct InputTranscription {
    pub input_id: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[ts(tag = "type", rename_all = "kebab-case")]
pub enum ClientEvent {
    InputTranscription {
        transcription: InputTranscription,
    },
    TurnMistakes {
        #[serde(rename = "inputId")]
        #[ts(rename = "inputId")]
        input_id: Option<String>,
        mistakes: Vec<KoreanTutorMistake>,
    },
    Text {
        text: String,
    },
    Audio {
        audio: AudioPayload,
    },
    TurnComplete,
    Error {
        message: String,
    },
}

impl From<SessionEvent> for ClientEvent {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::InputTranscription { input_id, text } => Self::InputTranscription {
                transcription: InputTranscription { input_id, text },
            },
            SessionEvent::TurnMistakes { input_id, mistakes } => Self::TurnMistakes { input_id, mistakes },
            SessionEvent::Text { text } => Self::Text { text },
            SessionEvent::Audio { audio } => Self::Audio {
                audio: AudioPayload {
                    data: STANDARD.encode(audio.data),
                    mime_type: audio.mime_type,
                },
            },
            SessionEvent::TurnComplete => Self::TurnComplete,
            SessionEvent::Error { message } => Self::Error { message },
        }
    }
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    Session(SessionError),
}

impl From<SessionError> for ApiError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Session(SessionError::NotFound) => (StatusCode::NOT_FOUND, "voice session not found".to_owned()),
            Self::BadRequest(message) | Self::Session(SessionError::InvalidAudio(message)) => {
                (StatusCode::BAD_REQUEST, message)
            },
            Self::Session(SessionError::Turn(message)) => (StatusCode::BAD_GATEWAY, message),
        };
        (status, Json(serde_json::json!({"message": message}))).into_response()
    }
}

/// Writes the complete browser API contract generated from Rust wire types.
///
/// # Errors
///
/// Returns an I/O error when the destination directory or file cannot be written.
pub fn export_type_script(path: impl AsRef<Path>) -> std::io::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    let config = ts_rs::Config::default();
    let declarations = [
        KoreanTutorLevel::decl(&config),
        TextModel::decl(&config),
        TranscriptionModel::decl(&config),
        SpeechSynthesisModel::decl(&config),
        ModelConfiguration::decl(&config),
        KoreanTutorMistakeKind::decl(&config),
        KoreanTutorMistake::decl(&config),
        AudioPayload::decl(&config),
        InputTranscription::decl(&config),
        CreateSessionRequest::decl(&config),
        CreateSessionResponse::decl(&config),
        SendAudioTurnRequest::decl(&config),
        ClientEvent::decl(&config),
    ]
    .map(|declaration| format!("export {declaration}"))
    .join("\n\n");
    std::fs::write(
        path,
        format!("// Generated by `cargo run -p tutor-server --bin export-types`. Do not edit.\n\n{declarations}\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_event_names_for_event_source_clients() {
        let value = serde_json::to_value(ClientEvent::TurnComplete).unwrap();
        assert_eq!(value, serde_json::json!({"type": "turn-complete"}));
    }

    #[test]
    fn generated_contract_includes_camel_case_fields() {
        let config = ts_rs::Config::default();
        assert!(ModelConfiguration::decl(&config).contains("mistakeDetection"));
        assert!(ClientEvent::decl(&config).contains("input-transcription"));
    }
}
