//! A provider-neutral interface for interacting with language models.
//!
//! This crate is independent of any agent runtime. Integrations can adapt these
//! types to the traits exposed by their runtime of choice. Consumers call
//! [`ask`] with a [`ModelId`]; backend selection and transport are implementation
//! details.

#![forbid(unsafe_code)]

pub use async_trait::async_trait;

mod client;
mod llama;
mod model;
mod openai;

pub use client::{ClientError, ask};
pub use model::{
    Model, ModelError, ModelId, ModelMessage, ModelRequest, ModelResponse, ModelResponseFormat,
    ModelRole, ModelUsage, ParseModelIdError,
};
