//// Typed, runtime-neutral workflows for constrained agent invocations.
////
//// A flow invokes one agent node at a time. Every node returns a
//// schema-backed Gleam value, and ordinary Gleam code decides which node runs
//// next or whether the flow is complete.

import gleam/dict.{type Dict}
import gleam/dynamic.{type Dynamic}
import gleam/dynamic/decode.{type Decoder}
import gleam/json.{type Json}
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string

/// Best-effort filesystem access requested for one node.
pub type Access {
  ReadOnly
  WorkspaceWrite
  Full
}

/// Best-effort internet access requested for one node.
pub type Internet {
  Disabled
  Enabled
}

/// A runtime-neutral request to invoke one agent node.
pub type RuntimeRequest {
  RuntimeRequest(
    flow_name: String,
    node_name: String,
    invocation: Int,
    prompt: String,
    output_schema: Json,
    working_directory: String,
    access: Access,
    internet: Internet,
  )
}

/// The observable result of a successful runtime invocation.
pub type RuntimeResponse {
  RuntimeResponse(
    output: String,
    events: List(Json),
    stdout: String,
    stderr: String,
  )
}

/// Creates a successful runtime response with no diagnostics.
pub fn runtime_response(output: String) -> RuntimeResponse {
  RuntimeResponse(output:, events: [], stdout: "", stderr: "")
}

/// A failed runtime invocation together with any observable diagnostics.
pub type RuntimeError {
  RuntimeError(
    message: String,
    events: List(Json),
    stdout: String,
    stderr: String,
  )
}

/// Creates a runtime error with no diagnostics.
pub fn runtime_error(message: String) -> RuntimeError {
  RuntimeError(message:, events: [], stdout: "", stderr: "")
}

/// Executes one fully assembled agent invocation.
pub type AgentRuntime =
  fn(RuntimeRequest) -> Result(RuntimeResponse, RuntimeError)

/// The category of a failed node invocation.
pub type InvocationErrorKind {
  InvalidInput
  Runtime
  InvalidOutput
}

/// An error produced while invoking or decoding one node.
pub type InvocationError {
  InvocationError(kind: InvocationErrorKind, message: String)
}

/// A failed step invocation that retains its original input.
pub type StepFailure(input) {
  StepFailure(input: input, error: InvocationError, invocation: Int)
}

/// A consumer-selected failure that stops a flow.
pub type FlowFailure {
  FlowFailure(message: String)
}

/// Creates a consumer-selected flow failure.
pub fn flow_failure(message: String) -> FlowFailure {
  FlowFailure(message)
}

/// An error that prevents a flow from completing.
pub type FlowError {
  InvalidDefinition(message: String)
  Failed(failure: FlowFailure)
  TypeMismatch(node_name: String)
}

/// Renders a flow error as a human-readable message.
pub fn flow_error_message(error: FlowError) -> String {
  case error {
    InvalidDefinition(message) -> "invalid flow definition: " <> message
    Failed(FlowFailure(message)) -> "flow failed: " <> message
    TypeMismatch(name) ->
      "internal flow type mismatch for node `" <> name <> "`"
  }
}

/// Filesystem settings for one sequential flow run.
///
/// Debug trace persistence is intentionally deferred while the Codex backend
/// is stubbed.
pub type RunConfig {
  RunConfig(working_directory: String)
}

/// Creates the default run configuration.
pub fn run_config() -> RunConfig {
  RunConfig(working_directory: ".")
}

/// Sets the working directory included in runtime requests.
pub fn working_directory(_config: RunConfig, path: String) -> RunConfig {
  RunConfig(path)
}

/// A completed flow and its final typed value.
pub type FlowRun(output) {
  FlowRun(name: String, output: output, invocations: Int)
}

type StepId

@external(erlang, "erlang", "make_ref")
fn new_step_id() -> StepId

/// A named, typed routing identity in a flow.
///
/// Gleam does not derive JSON encoders, decoders, or JSON Schema, so a step
/// receives those values explicitly. Only the output needs a decoder and
/// schema; typed inputs use the supplied total encoder.
pub opaque type Step(input, output) {
  Step(
    id: StepId,
    name: String,
    encode_input: fn(input) -> Json,
    output_decoder: Decoder(output),
    output_schema: Json,
  )
}

/// Creates a named, typed step handle.
pub fn step(
  name name: String,
  encode_input encode_input: fn(input) -> Json,
  decode_output output_decoder: Decoder(output),
  output_schema output_schema: Json,
) -> Step(input, output) {
  Step(id: new_step_id(), name:, encode_input:, output_decoder:, output_schema:)
}

/// Returns a step's graph name.
pub fn step_name(step: Step(input, output)) -> String {
  step.name
}

type Invocation {
  Invocation(
    step_id: StepId,
    step_name: String,
    input: Dynamic,
    input_json: Json,
    output_schema: Json,
  )
}

@external(erlang, "gleam_stdlib", "identity")
fn erase(value: input) -> Dynamic

// Invocation values and step handles are opaque. `restore` is called only
// after `registration_for` proves that the invocation and registration carry
// the same unforgeable BEAM reference, so the erased value has the input type
// captured by that registration.
@external(erlang, "gleam_stdlib", "identity")
fn restore(value: Dynamic) -> output

/// The next action selected by a deterministic node handler.
pub opaque type Transition(output) {
  Next(Invocation)
  Complete(output)
  Fail(FlowFailure)
}

/// Routes execution to one step with the supplied input.
pub fn next(
  step: Step(input, node_output),
  input: input,
) -> Transition(output) {
  Next(invocation(step, input))
}

/// Completes a flow with its final typed output.
pub fn complete(output: output) -> Transition(output) {
  Complete(output)
}

/// Stops a flow with a consumer-selected failure.
pub fn fail(failure: FlowFailure) -> Transition(output) {
  Fail(failure)
}

fn invocation(step: Step(input, output), input: input) -> Invocation {
  Invocation(
    step_id: step.id,
    step_name: step.name,
    input: erase(input),
    input_json: step.encode_input(input),
    output_schema: step.output_schema,
  )
}

type NodeSpec {
  NodeSpec(
    step_id: StepId,
    name: String,
    prompt: String,
    access: Access,
    internet: Internet,
  )
}

type HandlerRegistration(output) {
  HandlerRegistration(
    spec: NodeSpec,
    handle: fn(Dynamic, Result(String, InvocationError), Int) ->
      Transition(output),
  )
}

/// Builds a flow from typed step registrations.
pub opaque type FlowBuilder(output) {
  FlowBuilder(
    name: String,
    initial: Option(Invocation),
    handlers: Dict(String, HandlerRegistration(output)),
    definition_errors: List(String),
  )
}

/// Configures a step and its deterministic handlers.
pub opaque type NodeBuilder(flow_output, input, node_output) {
  NodeBuilder(
    flow: FlowBuilder(flow_output),
    step: Step(input, node_output),
    prompt: String,
    access: Access,
    internet: Internet,
    error_handler: Option(fn(StepFailure(input)) -> Transition(flow_output)),
  )
}

/// A readable, single-path agent workflow.
pub opaque type Flow(output) {
  Flow(
    name: String,
    initial: Option(Invocation),
    handlers: Dict(String, HandlerRegistration(output)),
    definition_errors: List(String),
  )
}

/// Creates a typed flow builder.
pub fn flow(name: String) -> FlowBuilder(output) {
  FlowBuilder(name:, initial: None, handlers: dict.new(), definition_errors: [])
}

/// Selects the first step invocation in the flow.
pub fn begins_with(
  builder: FlowBuilder(output),
  step: Step(input, node_output),
  input: input,
) -> FlowBuilder(output) {
  case builder.initial {
    Some(_) ->
      FlowBuilder(..builder, definition_errors: [
        "a flow can only have one initial invocation",
        ..builder.definition_errors
      ])
    None -> FlowBuilder(..builder, initial: Some(invocation(step, input)))
  }
}

/// Starts configuring one typed step in the flow.
pub fn node(
  builder: FlowBuilder(flow_output),
  step: Step(input, node_output),
) -> NodeBuilder(flow_output, input, node_output) {
  NodeBuilder(
    flow: builder,
    step:,
    prompt: "",
    access: ReadOnly,
    internet: Disabled,
    error_handler: None,
  )
}

/// Sets the agent prompt for a node.
pub fn prompt(
  builder: NodeBuilder(flow_output, input, node_output),
  prompt: String,
) -> NodeBuilder(flow_output, input, node_output) {
  NodeBuilder(..builder, prompt:)
}

/// Sets the best-effort filesystem access for a node.
pub fn access(
  builder: NodeBuilder(flow_output, input, node_output),
  access: Access,
) -> NodeBuilder(flow_output, input, node_output) {
  NodeBuilder(..builder, access:)
}

/// Sets the best-effort internet access for a node.
pub fn internet(
  builder: NodeBuilder(flow_output, input, node_output),
  internet: Internet,
) -> NodeBuilder(flow_output, input, node_output) {
  NodeBuilder(..builder, internet:)
}

/// Registers optional recovery behavior for failed invocations.
///
/// Without a catch handler, a failure stops the flow.
pub fn catch(
  builder: NodeBuilder(flow_output, input, node_output),
  handler: fn(StepFailure(input)) -> Transition(flow_output),
) -> NodeBuilder(flow_output, input, node_output) {
  case builder.error_handler {
    None -> NodeBuilder(..builder, error_handler: Some(handler))
    Some(_) -> {
      let flow =
        add_definition_error(
          builder.flow,
          "step `" <> builder.step.name <> "` has more than one `catch` handler",
        )
      NodeBuilder(..builder, flow:)
    }
  }
}

/// Registers success behavior and closes this node definition.
pub fn then(
  builder: NodeBuilder(flow_output, input, node_output),
  handler: fn(node_output) -> Transition(flow_output),
) -> FlowBuilder(flow_output) {
  let name = builder.step.name
  case dict.has_key(builder.flow.handlers, name) {
    True ->
      add_definition_error(
        builder.flow,
        "step `" <> name <> "` has more than one node definition",
      )
    False -> {
      let step = builder.step
      let error_handler = builder.error_handler
      let registration =
        HandlerRegistration(
          spec: NodeSpec(
            step_id: step.id,
            name:,
            prompt: builder.prompt,
            access: builder.access,
            internet: builder.internet,
          ),
          handle: fn(erased_input, response, invocation_number) {
            let typed_input: input = restore(erased_input)
            case response {
              Error(error) ->
                handle_failure(
                  error_handler,
                  StepFailure(typed_input, error, invocation_number),
                )
              Ok(raw_output) ->
                case json.parse(raw_output, step.output_decoder) {
                  Ok(output) -> handler(output)
                  Error(error) ->
                    handle_failure(
                      error_handler,
                      StepFailure(
                        typed_input,
                        InvocationError(
                          InvalidOutput,
                          decode_error_message(error),
                        ),
                        invocation_number,
                      ),
                    )
                }
            }
          },
        )
      FlowBuilder(
        ..builder.flow,
        handlers: dict.insert(builder.flow.handlers, name, registration),
      )
    }
  }
}

fn handle_failure(
  handler: Option(fn(StepFailure(input)) -> Transition(output)),
  failure: StepFailure(input),
) -> Transition(output) {
  case handler {
    Some(handler) -> handler(failure)
    None -> Fail(FlowFailure(failure.error.message))
  }
}

fn decode_error_message(error: json.DecodeError) -> String {
  case error {
    json.UnableToDecode(_) ->
      "node output did not match its Gleam decoder: " <> string.inspect(error)
    _ -> "node returned invalid JSON: " <> string.inspect(error)
  }
}

fn add_definition_error(
  builder: FlowBuilder(output),
  message: String,
) -> FlowBuilder(output) {
  FlowBuilder(..builder, definition_errors: [
    message,
    ..builder.definition_errors
  ])
}

/// Finishes the definition and returns a runnable flow.
pub fn build(builder: FlowBuilder(output)) -> Flow(output) {
  Flow(
    name: builder.name,
    initial: builder.initial,
    handlers: builder.handlers,
    definition_errors: builder.definition_errors,
  )
}

/// Validates a flow definition without invoking a runtime.
pub fn validate(flow: Flow(output)) -> Result(Nil, FlowError) {
  use _ <- result.try(validate_name(flow.name))
  case list.reverse(flow.definition_errors) {
    [first, ..rest] ->
      Error(InvalidDefinition(
        [first, ..rest]
        |> string.join(with: "; "),
      ))
    [] -> {
      use _ <- result.try(
        flow.handlers
        |> dict.values
        |> list.try_each(fn(registration) { validate_node(registration.spec) }),
      )
      case flow.initial {
        None -> Error(InvalidDefinition("the flow has no initial invocation"))
        Some(initial) -> {
          use _ <- result.try(registration_for(flow, initial))
          Ok(Nil)
        }
      }
    }
  }
}

fn validate_name(name: String) -> Result(Nil, FlowError) {
  case string.trim(name) {
    "" -> Error(InvalidDefinition("flow names cannot be empty"))
    _ -> Ok(Nil)
  }
}

fn validate_node(spec: NodeSpec) -> Result(Nil, FlowError) {
  case is_valid_node_name(spec.name) {
    False ->
      Error(InvalidDefinition(
        "node name `"
        <> spec.name
        <> "` must contain only ASCII letters, digits, `-`, or `_`",
      ))
    True ->
      case string.trim(spec.prompt) {
        "" ->
          Error(InvalidDefinition(
            "node `" <> spec.name <> "` has an empty prompt",
          ))
        _ -> Ok(Nil)
      }
  }
}

fn is_valid_node_name(name: String) -> Bool {
  name != ""
  && name
  |> string.to_utf_codepoints
  |> list.all(fn(codepoint) {
    let codepoint = string.utf_codepoint_to_int(codepoint)
    codepoint >= 48
    && codepoint <= 57
    || codepoint >= 65
    && codepoint <= 90
    || codepoint >= 97
    && codepoint <= 122
    || codepoint == 45
    || codepoint == 95
  })
}

fn registration_for(
  flow: Flow(output),
  invocation: Invocation,
) -> Result(HandlerRegistration(output), FlowError) {
  case dict.get(flow.handlers, invocation.step_name) {
    Error(_) ->
      Error(InvalidDefinition(
        "step `" <> invocation.step_name <> "` has no node definition",
      ))
    Ok(registration) if registration.spec.step_id != invocation.step_id ->
      Error(InvalidDefinition(
        "the invocation for step `"
        <> invocation.step_name
        <> "` did not use its registered step handle",
      ))
    Ok(registration) -> Ok(registration)
  }
}

/// Runs the flow using the default configuration.
pub fn run(
  flow: Flow(output),
  runtime: AgentRuntime,
) -> Result(FlowRun(output), FlowError) {
  run_with(flow, runtime, run_config())
}

/// Runs the flow with an explicit working directory.
pub fn run_with(
  flow: Flow(output),
  runtime: AgentRuntime,
  config: RunConfig,
) -> Result(FlowRun(output), FlowError) {
  use _ <- result.try(validate(flow))
  let assert Some(initial) = flow.initial
  execute(flow, runtime, config, initial, 1)
}

fn execute(
  flow: Flow(output),
  runtime: AgentRuntime,
  config: RunConfig,
  current: Invocation,
  invocation_number: Int,
) -> Result(FlowRun(output), FlowError) {
  use registration <- result.try(registration_for(flow, current))
  let node = registration.spec
  let prompt = assemble_prompt(node.prompt, current.input_json)
  let request =
    RuntimeRequest(
      flow_name: flow.name,
      node_name: node.name,
      invocation: invocation_number,
      prompt:,
      output_schema: current.output_schema,
      working_directory: config.working_directory,
      access: node.access,
      internet: node.internet,
    )
  let response =
    runtime(request)
    |> result.map(fn(response) { response.output })
    |> result.map_error(fn(error) { InvocationError(Runtime, error.message) })
  let transition =
    registration.handle(current.input, response, invocation_number)
  case transition {
    Next(next_invocation) ->
      execute(flow, runtime, config, next_invocation, invocation_number + 1)
    Complete(output) ->
      Ok(FlowRun(name: flow.name, output:, invocations: invocation_number))
    Fail(failure) -> Error(Failed(failure))
  }
}

fn assemble_prompt(node_prompt: String, input: Json) -> String {
  string.trim(node_prompt)
  <> "\n\nNode input (JSON):\n```json\n"
  <> json.to_string(input)
  <> "\n```\n\nReturn only the JSON result for this node."
}
