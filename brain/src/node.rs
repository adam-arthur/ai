use std::{marker::PhantomData, sync::atomic::{AtomicU64, Ordering}};

use crate::{Access, Internet, InvocationError};

/// Creates a typed agent node with read-only, offline defaults.
///
/// The name must be static so the resulting node remains [`Copy`].
pub fn node<I, O>(name: &'static str) -> Node<I, O> {
    Node::new(name)
}

static NEXT_NODE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NodeSpec {
    pub id: u64,
    pub name: &'static str,
    pub prompt: &'static str,
    pub access: Access,
    pub internet: Internet,
}

/// A named agent operation with typed input and output.
#[derive(Debug)]
pub struct Node<I, O> {
    pub(crate) spec: NodeSpec,
    marker: PhantomData<fn(I) -> O>,
}

impl<I, O> Copy for Node<I, O> {}

impl<I, O> Clone for Node<I, O> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I, O> Node<I, O> {
    pub fn new(name: &'static str) -> Self {
        Self {
            spec: NodeSpec {
                id: NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed),
                name,
                prompt: "",
                access: Access::default(),
                internet: Internet::default(),
            },
            marker: PhantomData,
        }
    }

    /// Sets the static prompt used for every invocation of this node.
    pub fn prompt(mut self, prompt: &'static str) -> Self {
        self.spec.prompt = prompt;
        self
    }

    pub fn access(mut self, access: Access) -> Self {
        self.spec.access = access;
        self
    }

    pub fn internet(mut self, internet: Internet) -> Self {
        self.spec.internet = internet;
        self
    }

    pub fn name(&self) -> &str {
        self.spec.name
    }

    /// Creates one invocation without consuming the reusable node handle.
    pub fn with(&self, input: I) -> NodeInvocation<I, O> {
        NodeInvocation { node: *self, input }
    }
}

/// A typed request to invoke a particular node.
#[derive(Debug)]
pub struct NodeInvocation<I, O> {
    pub(crate) node: Node<I, O>,
    pub(crate) input: I,
}

/// A failed node invocation that retains ownership of its original input.
#[derive(Debug)]
pub struct NodeFailure<I> {
    input: I,
    error: InvocationError,
    invocation: usize,
}

impl<I> NodeFailure<I> {
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
