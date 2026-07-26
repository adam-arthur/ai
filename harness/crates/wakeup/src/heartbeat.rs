use std::{future::Future, sync::Arc, time::Duration};

use thiserror::Error;
use tokio::time::{self, MissedTickBehavior};

use crate::BoxFuture;

type Check = Arc<dyn Fn() -> BoxFuture<'static, Result<bool, HeartbeatError>> + Send + Sync>;
type Effect = Arc<dyn Fn() -> BoxFuture<'static, Result<(), HeartbeatError>> + Send + Sync>;

/// Periodically checks a condition and runs an effect when it is true.
#[derive(Clone)]
pub struct Heartbeat {
    interval: Duration,
    check: Check,
    effect: Effect,
}

impl Heartbeat {
    pub fn new<C, CFut, E, EFut>(interval: Duration, check: C, effect: E) -> Self
    where
        C: Fn() -> CFut + Send + Sync + 'static,
        CFut: Future<Output = Result<bool, HeartbeatError>> + Send + 'static,
        E: Fn() -> EFut + Send + Sync + 'static,
        EFut: Future<Output = Result<(), HeartbeatError>> + Send + 'static,
    {
        Self {
            interval,
            check: Arc::new(move || Box::pin(check())),
            effect: Arc::new(move || Box::pin(effect())),
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Performs one check immediately.
    pub async fn poll_once(&self) -> Result<HeartbeatOutcome, HeartbeatError> {
        if (self.check)().await? {
            (self.effect)().await?;
            Ok(HeartbeatOutcome::EffectExecuted)
        } else {
            Ok(HeartbeatOutcome::ConditionFalse)
        }
    }

    /// Runs until `shutdown` resolves. The first check occurs after one interval.
    pub async fn run_until<F>(&self, shutdown: F) -> Result<(), HeartbeatError>
    where
        F: Future<Output = ()> + Send,
    {
        if self.interval.is_zero() {
            return Err(HeartbeatError::new("heartbeat interval must be greater than zero"));
        }
        let mut timer = time::interval(self.interval);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        timer.tick().await; // Consume Tokio's immediate first tick.
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => return Ok(()),
                _ = timer.tick() => { self.poll_once().await?; }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatOutcome {
    ConditionFalse,
    EffectExecuted,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{message}")]
pub struct HeartbeatError {
    pub message: String,
}

impl HeartbeatError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
