import assert from "node:assert/strict";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { z } from "zod";

import {
  FlowError,
  FlowErrorKind,
  RuntimeError,
  complete,
  fail,
  flow,
  next,
  node,
  type AgentRuntime,
  type RuntimeRequest,
  type RuntimeResponse,
} from "brain-js";

const researchInputSchema = z.object({ topic: z.string() });

const researchResultSchema = z.object({
  finding: z.string(),
  needsAnalysis: z.boolean(),
});

const analysisInputSchema = z.object({ finding: z.string() });
const analysisResultSchema = z.object({ report: z.string() });

function response(output: string): RuntimeResponse {
  return { output };
}

class QueueRuntime implements AgentRuntime {
  readonly requests: RuntimeRequest[] = [];
  readonly #responses: (RuntimeResponse | RuntimeError)[];

  constructor(responses: (RuntimeResponse | RuntimeError)[]) {
    this.#responses = [...responses];
  }

  async invoke(request: RuntimeRequest): Promise<RuntimeResponse> {
    this.requests.push(request);
    const response = this.#responses.shift();
    if (response === undefined) throw new Error("a mock response must be available");
    if (response instanceof RuntimeError) throw response;
    return response;
  }
}

async function temporaryDirectory(t: test.TestContext): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "brain-js-test-"));
  t.after(() => rm(directory, { force: true, recursive: true }));
  return directory;
}

async function exists(path: string): Promise<boolean> {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

test("runs heterogeneous nodes and completes with a typed value", async (t) => {
  const temporary = await temporaryDirectory(t);
  const debug = join(temporary, "debug");
  const runtime = new QueueRuntime([
    response(
      JSON.stringify({ finding: "typed flows are useful", needsAnalysis: true }),
    ),
    response(JSON.stringify({ report: "ship the experiment" })),
  ]);
  const research = node({
    name: "research",
    input: researchInputSchema,
    output: researchResultSchema,
    prompt: "Research the topic.",
  });
  const analyze = node({
    name: "analyze",
    input: analysisInputSchema,
    output: analysisResultSchema,
    prompt: "Analyze the finding.",
  });

  const run = await flow<string>("investigate")
    .startWith(research.withInput({ topic: "agent workflows" }))
    .on(research, (outcome) => {
      if (!outcome.ok) return fail(outcome.error);
      if (outcome.value.needsAnalysis) {
        return next(analyze.withInput({ finding: outcome.value.finding }));
      }
      return complete(outcome.value.finding);
    })
    .on(analyze, (outcome) =>
      outcome.ok ? complete(outcome.value.report) : fail(outcome.error),
    )
    .run(runtime, { workingDirectory: temporary, debugDirectory: debug });

  assert.equal(run.output, "ship the experiment");
  assert.equal(run.invocations, 2);
  assert.equal(await exists(join(debug, "001-research/prompt.md")), true);
  assert.equal(await exists(join(debug, "001-research/response.json")), true);
  assert.equal(await exists(join(debug, "002-analyze/transition.json")), true);
  assert.equal(runtime.requests.length, 2);
  assert.match(runtime.requests[0]?.prompt ?? "", /agent workflows/);
  assert.deepEqual(runtime.requests[0]?.outputSchema, {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    type: "object",
    properties: {
      finding: { type: "string" },
      needsAnalysis: { type: "boolean" },
    },
    required: ["finding", "needsAnalysis"],
    additionalProperties: false,
  });
});

const attemptInputSchema = z.object({ attempt: z.number() });
const attemptOutputSchema = z.object({ answer: z.string() });

test("routes a failed invocation back to the same node", async (t) => {
  const temporary = await temporaryDirectory(t);
  const debug = join(temporary, "debug");
  const runtime = new QueueRuntime([
    response("not json"),
    response(JSON.stringify({ answer: "recovered" })),
  ]);
  const research = node({
    name: "research",
    input: attemptInputSchema,
    output: attemptOutputSchema,
    prompt: "Return an answer.",
  });

  const run = await flow<string>("retry-by-routing")
    .startWith(research.withInput({ attempt: 1 }))
    .on(research, (outcome) => {
      if (outcome.ok) return complete(outcome.value.answer);
      if (outcome.error.kind === "invalid_output") {
        return next(research.withInput({ attempt: outcome.input.attempt + 1 }));
      }
      return fail(outcome.error);
    })
    .run(runtime, { debugDirectory: debug });

  assert.equal(run.output, "recovered");
  assert.equal(run.invocations, 2);
  assert.equal(await exists(join(debug, "001-research/response.raw.txt")), true);
  assert.equal(await exists(join(debug, "002-research/response.json")), true);
});

test("delivers runtime failures to the handler with the original input", async (t) => {
  const temporary = await temporaryDirectory(t);
  const runtime = new QueueRuntime([new RuntimeError("model unavailable")]);
  const research = node({
    name: "research",
    input: attemptInputSchema,
    output: attemptOutputSchema,
    prompt: "Return an answer.",
  });

  await assert.rejects(
    flow<string>("failure")
      .startWith(research.withInput({ attempt: 7 }))
      .on(research, (outcome) => {
        if (outcome.ok) return complete(outcome.value.answer);
        assert.equal(outcome.input.attempt, 7);
        assert.equal(outcome.invocation, 1);
        assert.equal(outcome.error.kind, "runtime");
        return fail(outcome.error);
      })
      .run(runtime, { debugDirectory: join(temporary, "debug") }),
    (error: unknown) => {
      assert.equal(error instanceof FlowError, true);
      assert.equal((error as FlowError).kind, FlowErrorKind.Failed);
      assert.match((error as Error).message, /model unavailable/);
      return true;
    },
  );
});

test("rejects missing handlers before invoking the runtime", async (t) => {
  const temporary = await temporaryDirectory(t);
  const runtime = new QueueRuntime([]);
  const research = node({
    name: "research",
    input: attemptInputSchema,
    output: attemptOutputSchema,
    prompt: "Return an answer.",
  });

  await assert.rejects(
    flow<string>("invalid")
      .startWith(research.withInput({ attempt: 1 }))
      .run(runtime, { debugDirectory: join(temporary, "debug") }),
    /no `on` handler/,
  );
  assert.equal(runtime.requests.length, 0);
});

test("records invalid inputs without invoking the runtime", async (t) => {
  const temporary = await temporaryDirectory(t);
  const runtime = new QueueRuntime([]);
  const research = node({
    name: "research",
    input: z.object({ value: z.bigint() }),
    output: attemptOutputSchema,
    prompt: "Return an answer.",
  });

  await assert.rejects(
    flow<string>("invalid-input")
      .startWith(research.withInput({ value: 1n }))
      .on(research, (outcome) => {
        if (outcome.ok) throw new Error("expected invalid input");
        assert.equal(outcome.error.kind, "invalid_input");
        return fail(outcome.error);
      })
      .run(runtime, { debugDirectory: join(temporary, "debug") }),
    /failed to validate or serialize node input/,
  );
  assert.equal(runtime.requests.length, 0);
});

test("validates node inputs before invoking the runtime", async (t) => {
  const temporary = await temporaryDirectory(t);
  const runtime = new QueueRuntime([]);
  const research = node({
    name: "research",
    input: attemptInputSchema,
    output: attemptOutputSchema,
    prompt: "Return an answer.",
  });
  const invalidInput = { attempt: "one" } as unknown as { attempt: number };

  await assert.rejects(
    flow<string>("invalid-input-schema")
      .startWith(research.withInput(invalidInput))
      .on(research, (outcome) => {
        if (outcome.ok) throw new Error("expected invalid input");
        assert.equal(outcome.error.kind, "invalid_input");
        assert.equal(outcome.input, invalidInput);
        return fail(outcome.error);
      })
      .run(runtime, { debugDirectory: join(temporary, "debug") }),
    /node input did not match its schema/,
  );
  assert.equal(runtime.requests.length, 0);
});

test("continues trace numbering from the highest existing prefix", async (t) => {
  const temporary = await temporaryDirectory(t);
  const debug = join(temporary, "debug");
  const runtime = new QueueRuntime([
    response(JSON.stringify({ answer: "done" })),
    response(JSON.stringify({ answer: "again" })),
  ]);
  const research = node({
    name: "research",
    input: attemptInputSchema,
    output: attemptOutputSchema,
    prompt: "Return an answer.",
  });
  const execute = (attempt: number) =>
    flow<string>("numbering")
      .startWith(research.withInput({ attempt }))
      .on(research, (outcome) =>
        outcome.ok
          ? complete(outcome.value.answer)
          : fail(outcome.error),
      )
      .run(runtime, { debugDirectory: debug });

  await execute(1);
  await execute(2);

  const invocation = JSON.parse(
    await readFile(join(debug, "002-research/invocation.json"), "utf8"),
  ) as { invocation: number };
  assert.equal(invocation.invocation, 2);
});
