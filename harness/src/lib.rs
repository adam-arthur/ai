//! A small set of building blocks for plan-driven, tool-using LLM agents.
//!
//! Model transport is owned by the `llm` crate. [`AgentModel`] is the narrower
//! harness protocol that translates model output into the next plan directive
//! and agent action.

mod agent;
mod backend;
mod plan;

pub use agent::{Agent, AgentConfig, AgentError, AgentEvent, AgentRun};
pub use async_trait::async_trait;
pub use backend::{AgentAction, AgentDecision, AgentModel, AgentModelError, AgentRequest, AgentToolCall, PlanDirective, StructuredAgentModel};
pub use plan::{Plan, PlanError, PlanStep, StepStatus};
pub use tools::{Tool, ToolDefinition, ToolError, ToolRegistry, ToolRegistryError, TypedTool, WorkspaceTools, WorkspaceToolsError};
