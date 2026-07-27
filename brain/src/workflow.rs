use std::{any::Any, collections::BTreeMap, fs, marker::PhantomData, path::{Path, PathBuf}};

use schemars::{JsonSchema, schema_for};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{AgentRuntime, FlowError, FlowFailure, InvocationError, Node, NodeFailure, NodeInvocation, NodeOutcome, RuntimeError, RuntimeRequest, RuntimeResponse, node::NodeSpec};

/// Creates an empty typed flow definition.
pub fn flow<W>(name: impl Into<String>) -> Flow<W> {
    Flow::new(name)
}

/// Routes execution to one node invocation.
pub fn next<W, I, O>(invocation: NodeInvocation<I, O>) -> Transition<W>
where
    I: Serialize + Send + 'static,
    O: DeserializeOwned + JsonSchema + Send + 'static,
{
    Transition {
        kind: TransitionKind::Next(Box::new(TypedInvocation::from(invocation))),
    }
}

/// Completes a flow with its final typed output.
pub fn complete<W>(output: W) -> Transition<W> {
    Transition {
        kind: TransitionKind::Complete(output),
    }
}

/// Stops a flow with a consumer-selected failure.
pub fn fail<W>(error: impl Into<FlowFailure>) -> Transition<W> {
    Transition {
        kind: TransitionKind::Fail(error.into()),
    }
}

/// The next action selected by a deterministic node handler.
pub struct Transition<W> {
    kind: TransitionKind<W>,
}

enum TransitionKind<W> {
    Next(Box<dyn ErasedInvocation>),
    Complete(W),
    Fail(FlowFailure),
}

/// Filesystem settings for one sequential flow run.
#[derive(Clone, Debug)]
pub struct RunConfig {
    working_directory: PathBuf,
    debug_directory: PathBuf,
}

impl RunConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = path.into();
        self
    }

    pub fn debug_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.debug_directory = path.into();
        self
    }

    pub fn working_directory_path(&self) -> &Path {
        &self.working_directory
    }

    pub fn debug_directory_path(&self) -> &Path {
        &self.debug_directory
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            working_directory: PathBuf::from("."),
            debug_directory: PathBuf::from("debug"),
        }
    }
}

/// A completed flow and its final typed value.
#[derive(Debug)]
pub struct FlowRun<W> {
    pub name: String,
    pub output: W,
    /// Number of node invocations performed by this run.
    pub invocations: usize,
}

/// A readable, single-path agent workflow.
pub struct Flow<W> {
    name: String,
    initial: Option<Box<dyn ErasedInvocation>>,
    handlers: BTreeMap<String, HandlerRegistration<W>>,
    definition_errors: Vec<String>,
}

impl<W> Flow<W> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            initial: None,
            handlers: BTreeMap::new(),
            definition_errors: Vec::new(),
        }
    }

    /// Selects the first node invocation in the flow.
    pub fn begins_with<I, O>(mut self, invocation: NodeInvocation<I, O>) -> Self
    where
        I: Serialize + Send + 'static,
        O: DeserializeOwned + JsonSchema + Send + 'static,
    {
        if self.initial.is_some() {
            self.definition_errors
                .push("a flow can only have one initial invocation".into());
        } else {
            self.initial = Some(Box::new(TypedInvocation::from(invocation)));
        }
        self
    }

    /// Registers the single function that handles both success and failure for a node.
    pub fn after<I, O, F>(mut self, node: Node<I, O>, handler: F) -> Self
    where
        I: Serialize + Send + 'static,
        O: DeserializeOwned + JsonSchema + Send + 'static,
        F: Fn(NodeOutcome<I, O>) -> Transition<W> + Send + Sync + 'static,
    {
        let name = node.spec.name;
        if self.handlers.contains_key(name) {
            self.definition_errors
                .push(format!("node `{name}` has more than one `after` handler"));
            return self;
        }
        self.handlers.insert(
            name.to_owned(),
            HandlerRegistration {
                spec: node.spec,
                handler: Box::new(TypedHandler {
                    handler,
                    marker: PhantomData,
                }),
            },
        );
        self
    }

    /// Runs the flow in the current directory and writes traces beneath `debug`.
    pub async fn run<R>(self, runtime: &R) -> Result<FlowRun<W>, FlowError>
    where
        R: AgentRuntime + ?Sized,
    {
        self.run_with(runtime, RunConfig::default()).await
    }

    /// Runs the flow with explicit working and debug directories.
    pub async fn run_with<R>(mut self, runtime: &R, config: RunConfig) -> Result<FlowRun<W>, FlowError>
    where
        R: AgentRuntime + ?Sized,
    {
        self.validate()?;
        let mut current = self.initial.take().expect("the initial invocation was validated above");
        let mut sequence = prepare_debug_directory(&config.debug_directory)?;
        let mut run_invocations = 0;

        loop {
            let registration = self.registration_for(current.as_ref())?;
            sequence += 1;
            run_invocations += 1;

            let node_name = current.spec().name;
            let invocation_directory = config.debug_directory.join(format!("{sequence:03}-{node_name}"));
            fs::create_dir(&invocation_directory).map_err(|error| FlowError::io(&invocation_directory, error))?;

            write_json(
                invocation_directory.join("invocation.json"),
                &json!({
                    "flow": self.name,
                    "node": node_name,
                    "invocation": sequence,
                    "access": current.spec().access,
                    "internet": current.spec().internet,
                    "working_directory": config.working_directory,
                }),
            )?;

            let schema = current.output_schema();
            write_json(invocation_directory.join("output.schema.json"), &schema)?;

            let erased_outcome = match current.input_json() {
                Ok(input) => {
                    write_json(invocation_directory.join("input.json"), &input)?;
                    let prompt = assemble_prompt(current.spec().prompt, &input)?;
                    write_text(invocation_directory.join("prompt.md"), &prompt)?;

                    let request = RuntimeRequest {
                        flow_name: self.name.clone(),
                        node_name: node_name.to_owned(),
                        invocation: sequence,
                        prompt,
                        output_schema: schema,
                        working_directory: config.working_directory.clone(),
                        access: current.spec().access,
                        internet: current.spec().internet,
                    };
                    let response = runtime.invoke(request).await;
                    record_runtime_result(&invocation_directory, &response)?;
                    match response {
                        Ok(response) => current.into_outcome(Ok(response.output), sequence),
                        Err(error) => current.into_outcome(Err(InvocationError::runtime(error.message)), sequence),
                    }
                },
                Err(error) => {
                    write_text(invocation_directory.join("input.error.txt"), &error.to_string())?;
                    current.into_outcome(
                        Err(InvocationError::invalid_input(format!(
                            "failed to serialize node input: {error}"
                        ))),
                        sequence,
                    )
                },
            };

            if let Some(parsed_output) = &erased_outcome.parsed_output {
                write_json(invocation_directory.join("response.json"), parsed_output)?;
            }
            let transition = registration.handler.handle(erased_outcome.outcome, node_name)?;
            record_transition(&invocation_directory, &transition)?;

            match transition.kind {
                TransitionKind::Next(invocation) => current = invocation,
                TransitionKind::Complete(output) => {
                    return Ok(FlowRun {
                        name: self.name,
                        output,
                        invocations: run_invocations,
                    });
                },
                TransitionKind::Fail(error) => return Err(FlowError::Failed(error)),
            }
        }
    }

    fn validate(&self) -> Result<(), FlowError> {
        if self.name.trim().is_empty() {
            return Err(FlowError::InvalidDefinition("flow names cannot be empty".into()));
        }
        if !self.definition_errors.is_empty() {
            return Err(FlowError::InvalidDefinition(self.definition_errors.join("; ")));
        }
        let initial = self
            .initial
            .as_ref()
            .ok_or_else(|| FlowError::InvalidDefinition("the flow has no initial invocation".into()))?;
        for registration in self.handlers.values() {
            validate_node(&registration.spec)?;
        }
        self.registration_for(initial.as_ref())?;
        Ok(())
    }

    fn registration_for(&self, invocation: &dyn ErasedInvocation) -> Result<&HandlerRegistration<W>, FlowError> {
        let name = invocation.spec().name;
        let registration = self
            .handlers
            .get(name)
            .ok_or_else(|| FlowError::InvalidDefinition(format!("node `{name}` has no `after` handler")))?;
        if registration.spec != *invocation.spec() {
            return Err(FlowError::InvalidDefinition(format!(
                "the invocation for node `{name}` did not use its registered node handle"
            )));
        }
        Ok(registration)
    }
}

fn validate_node(spec: &NodeSpec) -> Result<(), FlowError> {
    if spec.name.is_empty()
        || !spec
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(FlowError::InvalidDefinition(format!(
            "node name `{}` must contain only ASCII letters, digits, `-`, or `_`",
            spec.name
        )));
    }
    if spec.prompt.trim().is_empty() {
        return Err(FlowError::InvalidDefinition(format!(
            "node `{}` has an empty prompt",
            spec.name
        )));
    }
    Ok(())
}

fn prepare_debug_directory(path: &Path) -> Result<usize, FlowError> {
    fs::create_dir_all(path).map_err(|error| FlowError::io(path, error))?;
    let entries = fs::read_dir(path).map_err(|error| FlowError::io(path, error))?;
    let mut highest = 0;
    for entry in entries {
        let entry = entry.map_err(|error| FlowError::io(path, error))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some((prefix, _)) = name.split_once('-') else {
            continue;
        };
        if let Ok(sequence) = prefix.parse::<usize>() {
            highest = highest.max(sequence);
        }
    }
    Ok(highest)
}

fn assemble_prompt(node_prompt: &str, input: &Value) -> Result<String, FlowError> {
    let input = serde_json::to_string_pretty(input)
        .map_err(|error| FlowError::InvalidDefinition(format!("failed to render node input: {error}")))?;
    Ok(format!(
        "{}\n\nNode input (JSON):\n```json\n{input}\n```\n\nReturn only the JSON result for this node.",
        node_prompt.trim()
    ))
}

fn record_runtime_result(directory: &Path, result: &Result<RuntimeResponse, RuntimeError>) -> Result<(), FlowError> {
    match result {
        Ok(response) => {
            write_text(directory.join("stdout.log"), &response.stdout)?;
            write_text(directory.join("stderr.log"), &response.stderr)?;
            write_json_lines(directory.join("runtime-events.jsonl"), &response.events)?;
            write_text(directory.join("response.raw.txt"), &response.output)?;
        },
        Err(error) => {
            write_text(directory.join("stdout.log"), &error.stdout)?;
            write_text(directory.join("stderr.log"), &error.stderr)?;
            write_json_lines(directory.join("runtime-events.jsonl"), &error.events)?;
            write_text(directory.join("runtime.error.txt"), &error.message)?;
        },
    }
    Ok(())
}

fn record_transition<W>(directory: &Path, transition: &Transition<W>) -> Result<(), FlowError> {
    let value = match &transition.kind {
        TransitionKind::Next(invocation) => json!({ "type": "next", "node": invocation.spec().name }),
        TransitionKind::Complete(_) => json!({ "type": "complete" }),
        TransitionKind::Fail(error) => json!({ "type": "fail", "error": error.message() }),
    };
    write_json(directory.join("transition.json"), &value)
}

fn write_text(path: impl Into<PathBuf>, contents: &str) -> Result<(), FlowError> {
    let path = path.into();
    fs::write(&path, contents).map_err(|error| FlowError::io(path, error))
}

fn write_json(path: impl Into<PathBuf>, value: &Value) -> Result<(), FlowError> {
    let path = path.into();
    let contents = serde_json::to_string_pretty(value)
        .map_err(|error| FlowError::InvalidDefinition(format!("failed to encode debug JSON: {error}")))?;
    fs::write(&path, format!("{contents}\n")).map_err(|error| FlowError::io(path, error))
}

fn write_json_lines(path: impl Into<PathBuf>, values: &[Value]) -> Result<(), FlowError> {
    let path = path.into();
    let mut contents = String::new();
    for value in values {
        let line = serde_json::to_string(value)
            .map_err(|error| FlowError::InvalidDefinition(format!("failed to encode runtime event: {error}")))?;
        contents.push_str(&line);
        contents.push('\n');
    }
    fs::write(&path, contents).map_err(|error| FlowError::io(path, error))
}

struct HandlerRegistration<W> {
    spec: NodeSpec,
    handler: Box<dyn ErasedHandler<W>>,
}

trait ErasedHandler<W>: Send + Sync {
    fn handle(&self, outcome: Box<dyn Any + Send>, node_name: &str) -> Result<Transition<W>, FlowError>;
}

struct TypedHandler<F, I, O> {
    handler: F,
    marker: PhantomData<fn(I) -> O>,
}

impl<W, I, O, F> ErasedHandler<W> for TypedHandler<F, I, O>
where
    I: Send + 'static,
    O: Send + 'static,
    F: Fn(NodeOutcome<I, O>) -> Transition<W> + Send + Sync,
{
    fn handle(&self, outcome: Box<dyn Any + Send>, node_name: &str) -> Result<Transition<W>, FlowError> {
        let outcome = outcome
            .downcast::<NodeOutcome<I, O>>()
            .map_err(|_| FlowError::TypeMismatch(node_name.into()))?;
        Ok((self.handler)(*outcome))
    }
}

struct ErasedNodeOutcome {
    outcome: Box<dyn Any + Send>,
    parsed_output: Option<Value>,
}

trait ErasedInvocation: Send {
    fn spec(&self) -> &NodeSpec;
    fn input_json(&self) -> Result<Value, serde_json::Error>;
    fn output_schema(&self) -> Value;
    fn into_outcome(self: Box<Self>, result: Result<String, InvocationError>, invocation: usize) -> ErasedNodeOutcome;
}

struct TypedInvocation<I, O> {
    node: Node<I, O>,
    input: I,
}

impl<I, O> From<NodeInvocation<I, O>> for TypedInvocation<I, O> {
    fn from(invocation: NodeInvocation<I, O>) -> Self {
        Self {
            node: invocation.node,
            input: invocation.input,
        }
    }
}

impl<I, O> ErasedInvocation for TypedInvocation<I, O>
where
    I: Serialize + Send + 'static,
    O: DeserializeOwned + JsonSchema + Send + 'static,
{
    fn spec(&self) -> &NodeSpec {
        &self.node.spec
    }

    fn input_json(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(&self.input)
    }

    fn output_schema(&self) -> Value {
        serde_json::to_value(schema_for!(O)).expect("a generated JSON Schema must serialize")
    }

    fn into_outcome(self: Box<Self>, result: Result<String, InvocationError>, invocation: usize) -> ErasedNodeOutcome {
        let Self { input, .. } = *self;
        match result {
            Ok(raw_output) => match serde_json::from_str::<Value>(&raw_output) {
                Ok(value) => match serde_json::from_value::<O>(value.clone()) {
                    Ok(output) => ErasedNodeOutcome {
                        outcome: Box::new(Ok::<O, NodeFailure<I>>(output)),
                        parsed_output: Some(value),
                    },
                    Err(error) => ErasedNodeOutcome {
                        outcome: Box::new(Err::<O, NodeFailure<I>>(NodeFailure::new(
                            input,
                            InvocationError::invalid_output(format!(
                                "node output did not match its Rust type: {error}"
                            )),
                            invocation,
                        ))),
                        parsed_output: Some(value),
                    },
                },
                Err(error) => ErasedNodeOutcome {
                    outcome: Box::new(Err::<O, NodeFailure<I>>(NodeFailure::new(
                        input,
                        InvocationError::invalid_output(format!("node returned invalid JSON: {error}")),
                        invocation,
                    ))),
                    parsed_output: None,
                },
            },
            Err(error) => ErasedNodeOutcome {
                outcome: Box::new(Err::<O, NodeFailure<I>>(NodeFailure::new(input, error, invocation))),
                parsed_output: None,
            },
        }
    }
}
