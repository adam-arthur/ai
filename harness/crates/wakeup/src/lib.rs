//! Wakeup primitives for event-driven agents.
//!
//! The crate currently provides periodic condition checks through [`Heartbeat`].
//! Future wake sources can represent external events such as webhooks, messages,
//! and queue deliveries.

#![forbid(unsafe_code)]

mod heartbeat;

pub use heartbeat::{Heartbeat, HeartbeatError, HeartbeatOutcome};

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;
