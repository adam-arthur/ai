import brain
import brain/codex
import gleam/dynamic/decode
import gleam/json
import gleam/string
import gleeunit

pub fn main() -> Nil {
  gleeunit.main()
}

type ResearchInput {
  ResearchInput(topic: String)
}

type ResearchResult {
  ResearchResult(finding: String, needs_analysis: Bool)
}

type AnalysisInput {
  AnalysisInput(finding: String)
}

type AnalysisResult {
  AnalysisResult(report: String)
}

fn encode_research_input(input: ResearchInput) -> json.Json {
  json.object([#("topic", json.string(input.topic))])
}

fn decode_research_result() -> decode.Decoder(ResearchResult) {
  use finding <- decode.field("finding", decode.string)
  use needs_analysis <- decode.field("needs_analysis", decode.bool)
  decode.success(ResearchResult(finding:, needs_analysis:))
}

fn encode_analysis_input(input: AnalysisInput) -> json.Json {
  json.object([#("finding", json.string(input.finding))])
}

fn decode_analysis_result() -> decode.Decoder(AnalysisResult) {
  use report <- decode.field("report", decode.string)
  decode.success(AnalysisResult(report:))
}

fn object_schema(
  properties: List(#(String, json.Json)),
  required: List(String),
) -> json.Json {
  json.object([
    #("type", json.string("object")),
    #("properties", json.object(properties)),
    #("required", json.array(required, of: json.string)),
    #("additionalProperties", json.bool(False)),
  ])
}

fn research_schema() -> json.Json {
  object_schema(
    [
      #("finding", json.object([#("type", json.string("string"))])),
      #("needs_analysis", json.object([#("type", json.string("boolean"))])),
    ],
    ["finding", "needs_analysis"],
  )
}

fn analysis_schema() -> json.Json {
  object_schema([#("report", json.object([#("type", json.string("string"))]))], [
    "report",
  ])
}

pub fn runs_heterogeneous_nodes_and_completes_test() {
  let research =
    brain.step(
      "research",
      encode_research_input,
      decode_research_result(),
      research_schema(),
    )
  let analyze =
    brain.step(
      "analyze",
      encode_analysis_input,
      decode_analysis_result(),
      analysis_schema(),
    )
  let graph =
    brain.flow("investigate")
    |> brain.begins_with(research, ResearchInput("agent workflows"))
    |> brain.node(research)
    |> brain.prompt("Research the topic.")
    |> brain.internet(brain.Enabled)
    |> brain.then(fn(result) {
      case result.needs_analysis {
        True -> brain.next(analyze, AnalysisInput(result.finding))
        False -> brain.complete(result.finding)
      }
    })
    |> brain.node(analyze)
    |> brain.prompt("Analyze the finding.")
    |> brain.then(fn(result) { brain.complete(result.report) })
    |> brain.build
  let runtime = fn(request: brain.RuntimeRequest) {
    case request.node_name {
      "research" -> {
        assert request.internet == brain.Enabled
        assert string.contains(request.prompt, "agent workflows")
        Ok(brain.runtime_response(
          "{\"finding\":\"typed flows are useful\",\"needs_analysis\":true}",
        ))
      }
      "analyze" ->
        Ok(brain.runtime_response("{\"report\":\"ship the experiment\"}"))
      _ -> Error(brain.runtime_error("unexpected node"))
    }
  }

  let assert Ok(run) = brain.run(graph, runtime)
  assert run.name == "investigate"
  assert run.output == "ship the experiment"
  assert run.invocations == 2
}

type AttemptInput {
  AttemptInput(attempt: Int)
}

type AttemptOutput {
  AttemptOutput(answer: String)
}

fn encode_attempt(input: AttemptInput) -> json.Json {
  json.object([#("attempt", json.int(input.attempt))])
}

fn decode_attempt_output() -> decode.Decoder(AttemptOutput) {
  use answer <- decode.field("answer", decode.string)
  decode.success(AttemptOutput(answer:))
}

fn attempt_schema() -> json.Json {
  object_schema([#("answer", json.object([#("type", json.string("string"))]))], [
    "answer",
  ])
}

pub fn catch_can_route_back_to_the_same_step_test() {
  let research =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("retry-by-routing")
    |> brain.begins_with(research, AttemptInput(1))
    |> brain.node(research)
    |> brain.prompt("Return an answer.")
    |> brain.catch(fn(failure) {
      case failure.error.kind {
        brain.InvalidOutput ->
          brain.next(research, AttemptInput(failure.input.attempt + 1))
        _ -> brain.fail(brain.flow_failure(failure.error.message))
      }
    })
    |> brain.then(fn(result) { brain.complete(result.answer) })
    |> brain.build
  let runtime = fn(request: brain.RuntimeRequest) {
    case request.invocation {
      1 -> Ok(brain.runtime_response("not json"))
      _ -> Ok(brain.runtime_response("{\"answer\":\"recovered\"}"))
    }
  }

  let assert Ok(run) = brain.run(graph, runtime)
  assert run.output == "recovered"
  assert run.invocations == 2
}

pub fn runtime_failure_reaches_catch_with_original_input_test() {
  let research =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("failure")
    |> brain.begins_with(research, AttemptInput(7))
    |> brain.node(research)
    |> brain.prompt("Return an answer.")
    |> brain.catch(fn(failure) {
      assert failure.input.attempt == 7
      assert failure.invocation == 1
      assert failure.error.kind == brain.Runtime
      brain.complete("caught")
    })
    |> brain.then(fn(result) { brain.complete(result.answer) })
    |> brain.build
  let runtime = fn(_request) { Error(brain.runtime_error("model unavailable")) }

  let assert Ok(run) = brain.run(graph, runtime)
  assert run.output == "caught"
}

pub fn failures_stop_without_a_catch_handler_test() {
  let research =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("failure")
    |> brain.begins_with(research, AttemptInput(1))
    |> brain.node(research)
    |> brain.prompt("Return an answer.")
    |> brain.then(fn(result) { brain.complete(result.answer) })
    |> brain.build
  let runtime = fn(_request) { Error(brain.runtime_error("model unavailable")) }

  let assert Error(brain.Failed(failure)) = brain.run(graph, runtime)
  assert failure.message == "model unavailable"
}

pub fn rejects_missing_handlers_before_invoking_runtime_test() {
  let research =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("invalid")
    |> brain.begins_with(research, AttemptInput(1))
    |> brain.build
  let runtime = fn(_request) {
    panic as "the runtime must not be called for an invalid graph"
  }

  let assert Error(brain.InvalidDefinition(message)) = brain.run(graph, runtime)
  assert string.contains(message, "no node definition")
}

pub fn rejects_a_different_step_with_the_same_name_test() {
  let initial =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let registered =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("invalid")
    |> brain.begins_with(initial, AttemptInput(1))
    |> brain.node(registered)
    |> brain.prompt("Return an answer.")
    |> brain.then(fn(result) { brain.complete(result.answer) })
    |> brain.build
  let runtime = fn(_request) {
    panic as "the runtime must not be called for an invalid graph"
  }

  let assert Error(brain.InvalidDefinition(message)) = brain.run(graph, runtime)
  assert string.contains(message, "registered step handle")
}

pub fn validates_names_and_prompts_test() {
  let research =
    brain.step(
      "not valid",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("valid")
    |> brain.begins_with(research, AttemptInput(1))
    |> brain.node(research)
    |> brain.prompt("Return an answer.")
    |> brain.then(fn(result) { brain.complete(result.answer) })
    |> brain.build

  let assert Error(brain.InvalidDefinition(message)) = brain.validate(graph)
  assert string.contains(message, "ASCII letters")
}

pub fn codex_backend_is_stubbed_test() {
  let research =
    brain.step(
      "research",
      encode_attempt,
      decode_attempt_output(),
      attempt_schema(),
    )
  let graph =
    brain.flow("codex-stub")
    |> brain.begins_with(research, AttemptInput(1))
    |> brain.node(research)
    |> brain.prompt("Return an answer.")
    |> brain.then(fn(result) { brain.complete(result.answer) })
    |> brain.build

  let assert Error(brain.Failed(failure)) =
    brain.run(graph, codex.new() |> codex.as_runtime)
  assert string.contains(failure.message, "not implemented")
}
