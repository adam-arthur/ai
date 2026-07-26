use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A model-created sequence of steps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlanData")]
pub struct Plan {
    steps: Vec<PlanStep>,
}

impl Plan {
    pub fn new(steps: impl IntoIterator<Item = impl Into<String>>) -> Result<Self, PlanError> {
        let steps = steps
            .into_iter()
            .enumerate()
            .map(|(index, description)| PlanStep {
                index,
                description: description.into(),
                status: StepStatus::Pending,
                summary: None,
            })
            .collect::<Vec<_>>();
        Self::try_from(PlanData { steps })
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    pub fn current(&self) -> Option<&PlanStep> {
        self.steps.iter().find(|step| step.status == StepStatus::Pending)
    }

    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|step| step.status == StepStatus::Complete)
    }

    pub fn remaining_steps(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == StepStatus::Pending)
            .count()
    }

    pub(crate) fn complete_current(&mut self, summary: String) -> Option<usize> {
        let step = self.steps.iter_mut().find(|step| step.status == StepStatus::Pending)?;
        step.status = StepStatus::Complete;
        step.summary = Some(summary);
        Some(step.index)
    }
}

#[derive(Deserialize)]
struct PlanData {
    steps: Vec<PlanStep>,
}

impl TryFrom<PlanData> for Plan {
    type Error = PlanError;

    fn try_from(data: PlanData) -> Result<Self, Self::Error> {
        if data.steps.is_empty() {
            return Err(PlanError::Empty);
        }
        for (position, step) in data.steps.iter().enumerate() {
            if step.description.is_empty() {
                return Err(PlanError::EmptyDescription(position));
            }
            if step.index != position {
                return Err(PlanError::InvalidIndex {
                    position,
                    index: step.index,
                });
            }
        }
        Ok(Self { steps: data.steps })
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("a plan must contain at least one step")]
    Empty,
    #[error("plan step {0} has an empty description")]
    EmptyDescription(usize),
    #[error("plan step at position {position} has index {index}")]
    InvalidIndex { position: usize, index: usize },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub index: usize,
    pub description: String,
    pub status: StepStatus,
    pub summary: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Complete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_plans_and_empty_descriptions() {
        assert_eq!(Plan::new(Vec::<String>::new()), Err(PlanError::Empty));
        assert_eq!(Plan::new([""]), Err(PlanError::EmptyDescription(0)));
    }

    #[test]
    fn deserialization_preserves_plan_invariants() {
        let error = serde_json::from_value::<Plan>(serde_json::json!({ "steps": [] })).unwrap_err();
        assert!(error.to_string().contains("at least one step"));
    }
}
