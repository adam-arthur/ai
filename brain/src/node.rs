use std::{marker::PhantomData, sync::Arc};

use crate::{Access, Internet, InvocationError};

/// Creates a typed agent node with read-only, offline defaults.
pub fn node<I, O>(name: impl Into<String>) -> Node<I, O> {
    Node::new(name)
}

#[derive(Clone, Debug)]
pub(crate) struct NodeSpec {
    pub name: String,
    pub prompt: String,
    pub access: Access,
    pub internet: Internet,
}

/// A named agent operation with typed input and output.
#[derive(Debug)]
pub struct Node<I, O> {
    pub(crate) spec: Arc<NodeSpec>,
    marker: PhantomData<fn(I) -> O>,
}

impl<I, O> Clone for Node<I, O> {
    fn clone(&self) -> Self {
        Self {
            spec: Arc::clone(&self.spec),
            marker: PhantomData,
        }
    }
}

impl<I, O> Node<I, O> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            spec: Arc::new(NodeSpec {
                name: name.into(),
                prompt: String::new(),
                access: Access::default(),
                internet: Internet::default(),
            }),
            marker: PhantomData,
        }
    }

    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        Arc::make_mut(&mut self.spec).prompt = prompt.into();
        self
    }

    pub fn access(mut self, access: Access) -> Self {
        Arc::make_mut(&mut self.spec).access = access;
        self
    }

    pub fn internet(mut self, internet: Internet) -> Self {
        Arc::make_mut(&mut self.spec).internet = internet;
        self
    }

    pub fn name(&self) -> &str {
        &self.spec.name
    }

    /// Creates one invocation without consuming the reusable node handle.
    pub fn with(&self, input: I) -> NodeInvocation<I, O> {
        NodeInvocation {
            node: self.clone(),
            input,
        }
    }
}

/// A typed request to invoke a particular node.
#[derive(Debug)]
pub struct NodeInvocation<I, O> {
    pub(crate) node: Node<I, O>,
    pub(crate) input: I,
}

/// The result delivered to the function registered with [`crate::Flow::after`].
pub type NodeOutcome<I, O> = Result<O, NodeFailure<I>>;

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
