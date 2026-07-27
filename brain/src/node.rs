use std::{marker::PhantomData, sync::atomic::{AtomicU64, Ordering}};

use crate::InvocationError;

/// Creates a named, typed step handle.
///
/// The name must be static so the resulting step remains [`Copy`].
pub fn step<I, O>(name: &'static str) -> Step<I, O> {
    Step::new(name)
}

static NEXT_STEP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StepSpec {
    pub id: u64,
    pub name: &'static str,
}

/// A named, typed routing identity in a flow.
#[derive(Debug)]
pub struct Step<I, O> {
    pub(crate) spec: StepSpec,
    marker: PhantomData<fn(I) -> O>,
}

impl<I, O> Copy for Step<I, O> {}

impl<I, O> Clone for Step<I, O> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I, O> Step<I, O> {
    pub fn new(name: &'static str) -> Self {
        Self {
            spec: StepSpec {
                id: NEXT_STEP_ID.fetch_add(1, Ordering::Relaxed),
                name,
            },
            marker: PhantomData,
        }
    }

    pub fn name(&self) -> &str {
        self.spec.name
    }
}

/// A typed request to invoke a particular step.
#[derive(Debug)]
pub(crate) struct StepInvocation<I, O> {
    pub(crate) step: Step<I, O>,
    pub(crate) input: I,
}

impl<I, O> StepInvocation<I, O> {
    pub(crate) fn new(step: Step<I, O>, input: I) -> Self {
        Self { step, input }
    }
}

/// A failed step invocation that retains ownership of its original input.
#[derive(Debug)]
pub struct StepFailure<I> {
    input: I,
    error: InvocationError,
    invocation: usize,
}

impl<I> StepFailure<I> {
    pub(crate) fn new(input: I, error: InvocationError, invocation: usize) -> Self {
        Self {
            input,
            error,
            invocation,
        }
    }

    pub fn input(&self) -> &I {
        &self.input
    }

    pub fn error(&self) -> &InvocationError {
        &self.error
    }

    pub const fn invocation(&self) -> usize {
        self.invocation
    }

    pub fn into_input(self) -> I {
        self.input
    }

    pub fn into_error(self) -> InvocationError {
        self.error
    }

    pub fn into_parts(self) -> (I, InvocationError) {
        (self.input, self.error)
    }
}
