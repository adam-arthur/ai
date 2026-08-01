//! Types and transport for OpenAI-compatible chat completion APIs.

mod client;
mod protocol;

pub(crate) use client::{OpenAiClient, OpenAiClientError};
pub(crate) use protocol::{
    ChatCompletion, ChatContent, ChatContentPart, ChatMessage, ChatRequest, ImageUrl,
    JsonSchemaFormat, ResponseFormat, Role,
};
