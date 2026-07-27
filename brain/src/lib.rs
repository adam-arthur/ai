//! Typed, runtime-neutral workflows for constrained agent invocations.
//!
//! A flow invokes one agent node at a time. Every node returns a schema-backed
//! Rust value, and ordinary Rust code decides which node runs next or whether
//! the flow is complete.

#![forbid(unsafe_code)]

mod error;
mod node;
mod runtime;
mod workflow;

pub use async_trait::async_trait;
pub use error::{FlowError, FlowFailure, InvocationError, InvocationErrorKind};
pub use node::{Node, NodeFailure, NodeInvocation, NodeOutcome, node};
pub use runtime::{Access, AgentRuntime, Internet, RuntimeError, RuntimeRequest, RuntimeResponse};
pub use workflow::{Flow, FlowRun, RunConfig, Transition, complete, fail, flow, next};
