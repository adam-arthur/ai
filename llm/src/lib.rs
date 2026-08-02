//! A provider-neutral interface for interacting with language models.
//!
//! This crate is independent of any agent runtime. Integrations can adapt these
//! types to the traits exposed by their runtime of choice. Consumers call
//! [`ask`] with an [`AskRequest`]; backend selection and transport are
//! implementation details.

#![forbid(unsafe_code)]

pub use async_trait::async_trait;

mod audio;
mod client;
mod google;
mod llama;
mod model;
mod openai;
mod openai_compatible;

pub use audio::{
    Audio, SpeechSynthesisModelId, SpeechSynthesisRequest, TranscriptionModelId,
    TranscriptionRequest,
};
pub use client::{AskRequest, ClientError, ask, complete, synthesize, transcribe};
pub use model::{
    ImageInput, Model, ModelError, ModelId, ModelMessage, ModelRequest, ModelResponse,
    ModelResponseSchema, ModelRole, ModelUsage, ParseModelIdError,
};
pub use schemars::JsonSchema;
