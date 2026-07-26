use std::{num::NonZeroUsize, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{AgentAction, AgentDecision, AgentModel, AgentModelError, AgentRequest, Plan, PlanDirective, ToolRegistry};

const DEFAULT_MAX_TOOL_CALLS_PER_TURN: usize = 8;

#[derive(Clone, Debug)]
pub struct AgentConfig {
    /// Maximum number of model responses.
    max_turns: NonZeroUsize,
    /// Maximum number of tools the model may request in one response.
    max_tool_calls_per_turn: NonZeroUsize,
}

impl AgentConfig {
    pub const fn new(max_turns: NonZeroUsize) -> Self {
        Self {
            max_turns,
            max_tool_calls_per_turn: NonZeroUsize::new(DEFAULT_MAX_TOOL_CALLS_PER_TURN)
                .expect("the default tool-call limit is non-zero"),
        }
    }

    pub const fn max_turns(&self) -> usize {
        self.max_turns.get()
    }

    pub const fn with_max_tool_calls_per_turn(mut self, limit: NonZeroUsize) -> Self {
        self.max_tool_calls_per_turn = limit;
        self
    }

    pub const fn max_tool_calls_per_turn(&self) -> usize {
        self.max_tool_calls_per_turn.get()
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: NonZeroUsize::new(64).expect("64 is non-zero"),
            max_tool_calls_per_turn: NonZeroUsize::new(DEFAULT_MAX_TOOL_CALLS_PER_TURN)
                .expect("the default tool-call limit is non-zero"),
        }
    }
}

pub struct Agent {
    model: Arc<dyn AgentModel>,
    tools: ToolRegistry,
    config: AgentConfig,
}

#[bon::bon]
impl Agent {
    #[builder]
    pub fn new<M>(
        #[builder(start_fn)] model: M, #[builder(default)] tools: ToolRegistry, #[builder(default)] config: AgentConfig,
    ) -> Self
    where
        M: AgentModel + 'static,
    {
        Self {
            model: Arc::new(model),
            tools,
            config,
        }
    }

    pub fn with_model(model: Arc<dyn AgentModel>) -> Self {
        Self {
            model,
            tools: ToolRegistry::new(),
            config: AgentConfig::default(),
        }
    }

    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    pub async fn run(&self, goal: impl Into<String>) -> Result<AgentRun, AgentError> {
        let goal = goal.into();
        let mut plan = None;
        let mut events = Vec::new();
        let mut turns = 0;

        loop {
            self.start_turn(&mut turns)?;
            let AgentDecision {
                plan: directive,
                action,
            } = self
                .model
                .decide(AgentRequest {
                    goal: goal.clone(),
                    plan: plan.clone(),
                    tools: self.tools.definitions(),
                    events: events.clone(),
                    max_tool_calls_per_turn: self.config.max_tool_calls_per_turn(),
                })
                .await?;

            apply_plan_directive(&mut plan, &mut events, directive)?;
            let current_plan = plan.as_mut().ok_or(AgentError::PlanNotCreated)?;

            match action {
                AgentAction::ToolCalls { calls } => {
                    if current_plan.is_complete() {
                        return Err(AgentError::ToolCallAfterPlanComplete);
                    }
                    if calls.is_empty() {
                        return Err(AgentError::EmptyToolCallBatch);
                    }
                    let limit = self.config.max_tool_calls_per_turn();
                    if calls.len() > limit {
                        return Err(AgentError::ToolCallBatchLimitExceeded {
                            count: calls.len(),
                            limit,
                        });
                    }

                    // Resolve the complete batch before starting it so an unknown tool
                    // cannot leave a partially executed batch behind.
                    let resolved = calls
                        .into_iter()
                        .enumerate()
                        .map(|(index, call)| {
                            let tool = self
                                .tools
                                .get(&call.name)
                                .ok_or_else(|| AgentError::UnknownTool(call.name.clone()))?;
                            let call_id = format!("turn-{turns}-call-{}", index + 1);
                            Ok((call_id, call, tool))
                        })
                        .collect::<Result<Vec<_>, AgentError>>()?;

                    for (call_id, call, _) in &resolved {
                        events.push(AgentEvent::ToolCalled {
                            call_id: call_id.clone(),
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        });
                    }
                    for (call_id, call, tool) in resolved {
                        match tool.call(call.arguments).await {
                            Ok(result) => events.push(AgentEvent::ToolReturned {
                                call_id,
                                name: call.name,
                                result,
                            }),
                            Err(error) => events.push(AgentEvent::ToolFailed {
                                call_id,
                                name: call.name,
                                error: error.to_string(),
                            }),
                        }
                    }
                },
                AgentAction::Finish { summary, output } => {
                    if current_plan.remaining_steps() != 1 {
                        return Err(AgentError::PrematureFinish);
                    }
                    let index = current_plan
                        .complete_current(summary.clone())
                        .ok_or(AgentError::NoCurrentStep)?;
                    events.push(AgentEvent::StepCompleted { index, summary });
                    events.push(AgentEvent::Completed { output: output.clone() });
                    return Ok(AgentRun {
                        goal,
                        plan: plan.expect("the plan was checked above"),
                        output,
                        events,
                        turns,
                    });
                },
            }
        }
    }

    fn start_turn(&self, turns: &mut usize) -> Result<(), AgentError> {
        if *turns >= self.config.max_turns() {
            return Err(AgentError::TurnLimitExceeded(self.config.max_turns()));
        }
        *turns += 1;
        Ok(())
    }
}

fn apply_plan_directive(
    plan: &mut Option<Plan>, events: &mut Vec<AgentEvent>, directive: PlanDirective,
) -> Result<(), AgentError> {
    match directive {
        PlanDirective::Create { steps } => {
            if plan.is_some() {
                return Err(AgentError::PlanAlreadyCreated);
            }
            let created = Plan::new(steps).map_err(AgentError::InvalidPlan)?;
            events.push(AgentEvent::PlanCreated { plan: created.clone() });
            *plan = Some(created);
        },
        PlanDirective::Keep => {
            if plan.is_none() {
                return Err(AgentError::PlanNotCreated);
            }
        },
        PlanDirective::Advance { summary } => {
            let plan = plan.as_mut().ok_or(AgentError::PlanNotCreated)?;
            if plan.remaining_steps() <= 1 {
                return Err(AgentError::CannotAdvanceFinalStep);
            }
            let index = plan
                .complete_current(summary.clone())
                .ok_or(AgentError::NoCurrentStep)?;
            events.push(AgentEvent::StepCompleted { index, summary });
        },
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    PlanCreated {
        plan: Plan,
    },
    ToolCalled {
        call_id: String,
        name: String,
        arguments: Value,
    },
    ToolReturned {
        call_id: String,
        name: String,
        result: Value,
    },
    ToolFailed {
        call_id: String,
        name: String,
        error: String,
    },
    StepCompleted {
        index: usize,
        summary: String,
    },
    Completed {
        output: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentRun {
    pub goal: String,
    pub plan: Plan,
    pub output: String,
    pub events: Vec<AgentEvent>,
    pub turns: usize,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent model failed: {0}")]
    Model(#[from] AgentModelError),
    #[error("model returned an invalid plan: {0}")]
    InvalidPlan(#[source] crate::PlanError),
    #[error("model requested unknown tool `{0}`")]
    UnknownTool(String),
    #[error("the model returned an empty tool-call batch")]
    EmptyToolCallBatch,
    #[error("the model requested {count} tool calls in one turn, exceeding the limit of {limit}")]
    ToolCallBatchLimitExceeded { count: usize, limit: usize },
    #[error("the model must create a plan on its first decision")]
    PlanNotCreated,
    #[error("the plan has already been created")]
    PlanAlreadyCreated,
    #[error("there is no current plan step")]
    NoCurrentStep,
    #[error("the final plan step must be completed by finishing")]
    CannotAdvanceFinalStep,
    #[error("a tool cannot be called after the plan is complete")]
    ToolCallAfterPlanComplete,
    #[error("the finish action is only valid when one pending plan step remains")]
    PrematureFinish,
    #[error("agent exceeded its limit of {0} model turns")]
    TurnLimitExceeded(usize),
}
