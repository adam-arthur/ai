import assert from "node:assert/strict";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { z } from "zod";

import {
  FlowError,
  FlowErrorKind,
  RunConfig,
  RuntimeError,
  RuntimeResponse,
  complete,
  fail,
  flow,
  next,
  node,
  type AgentRuntime,
  type RuntimeRequest,
} from "brain-js";

interface ResearchInput {
  topic: string;
}

const researchResultSchema = z.object({
  finding: z.string(),
  needsAnalysis: z.boolean(),
});

interface AnalysisInput {
  finding: string;
}

const analysisResultSchema = z.object({ report: z.string() });

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
    new RuntimeResponse(
      JSON.stringify({ finding: "typed flows are useful", needsAnalysis: true }),
    ),
    new RuntimeResponse(JSON.stringify({ report: "ship the experiment" })),
  ]);
  const research = node<ResearchInput, z.infer<typeof researchResultSchema>>(
    "research",
    researchResultSchema,
  ).prompt("Research the topic.");
  const analyze = node<AnalysisInput, z.infer<typeof analysisResultSchema>>(
    "analyze",
    analysisResultSchema,
  ).prompt("Analyze the finding.");
  const analyzeAfterResearch = analyze.clone();

  const run = await flow<string>("investigate")
    .beginsWith(research.with({ topic: "agent workflows" }))
    .after(research, (outcome) => {
      if (!outcome.ok) return fail(outcome.failure.intoError());
      if (outcome.value.needsAnalysis) {
        return next(
          analyzeAfterResearch.with({ finding: outcome.value.finding }),
        );
      }
      return complete(outcome.value.finding);
    })
    .after(analyze, (outcome) =>
      outcome.ok ? complete(outcome.value.report) : fail(outcome.failure.intoError()),
    )
    .runWith(
      runtime,
      new RunConfig()
        .workingDirectory(temporary)
        .debugDirectory(debug),
    );

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

interface AttemptInput {
  attempt: number;
}

const attemptOutputSchema = z.object({ answer: z.string() });

test("routes a failed invocation back to the same node", async (t) => {
  const temporary = await temporaryDirectory(t);
  const debug = join(temporary, "debug");
  const runtime = new QueueRuntime([
    new RuntimeResponse("not json"),
    new RuntimeResponse(JSON.stringify({ answer: "recovered" })),
  ]);
  const research = node<AttemptInput, z.infer<typeof attemptOutputSchema>>(
    "research",
    attemptOutputSchema,
  ).prompt("Return an answer.");
  const retry = research.clone();

  const run = await flow<string>("retry-by-routing")
    .beginsWith(research.with({ attempt: 1 }))
    .after(research, (outcome) => {
      if (outcome.ok) return complete(outcome.value.answer);
      if (outcome.failure.error().isInvalidOutput()) {
        const input = outcome.failure.intoInput();
        return next(retry.with({ attempt: input.attempt + 1 }));
      }
      return fail(outcome.failure.intoError());
    })
    .runWith(runtime, new RunConfig().debugDirectory(debug));

  assert.equal(run.output, "recovered");
  assert.equal(run.invocations, 2);
  assert.equal(await exists(join(debug, "001-research/response.raw.txt")), true);
  assert.equal(await exists(join(debug, "002-research/response.json")), true);
});

test("delivers runtime failures to the handler with the original input", async (t) => {
  const temporary = await temporaryDirectory(t);
  const runtime = new QueueRuntime([new RuntimeError("model unavailable")]);
  const research = node<AttemptInput, z.infer<typeof attemptOutputSchema>>(
    "research",
    attemptOutputSchema,
  ).prompt("Return an answer.");

  await assert.rejects(
    flow<string>("failure")
      .beginsWith(research.with({ attempt: 7 }))
      .after(research, (outcome) => {
        if (outcome.ok) return complete(outcome.value.answer);
        assert.equal(outcome.failure.input().attempt, 7);
        assert.equal(outcome.failure.invocation(), 1);
        assert.equal(outcome.failure.error().isRuntime(), true);
        return fail(outcome.failure.intoError());
      })
      .runWith(
        runtime,
        new RunConfig().debugDirectory(join(temporary, "debug")),
      ),
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
  const research = node<AttemptInput, z.infer<typeof attemptOutputSchema>>(
    "research",
    attemptOutputSchema,
  ).prompt("Return an answer.");

  await assert.rejects(
    flow<string>("invalid")
      .beginsWith(research.with({ attempt: 1 }))
      .runWith(
        runtime,
        new RunConfig().debugDirectory(join(temporary, "debug")),
      ),
    /no `after` handler/,
  );
  assert.equal(runtime.requests.length, 0);
});

test("records invalid inputs without invoking the runtime", async (t) => {
  const temporary = await temporaryDirectory(t);
  const runtime = new QueueRuntime([]);
  const research = node<{ value: bigint }, z.infer<typeof attemptOutputSchema>>(
    "research",
    attemptOutputSchema,
  ).prompt("Return an answer.");

  await assert.rejects(
    flow<string>("invalid-input")
      .beginsWith(research.with({ value: 1n }))
      .after(research, (outcome) => {
        assert.equal(outcome.ok, false);
        assert.equal(outcome.failure.error().isInvalidInput(), true);
        return fail(outcome.failure.intoError());
      })
      .runWith(
        runtime,
        new RunConfig().debugDirectory(join(temporary, "debug")),
      ),
    /failed to serialize node input/,
  );
  assert.equal(runtime.requests.length, 0);
});

test("continues trace numbering from the highest existing prefix", async (t) => {
  const temporary = await temporaryDirectory(t);
  const debug = join(temporary, "debug");
  const runtime = new QueueRuntime([
    new RuntimeResponse(JSON.stringify({ answer: "done" })),
    new RuntimeResponse(JSON.stringify({ answer: "again" })),
  ]);
  const research = node<AttemptInput, z.infer<typeof attemptOutputSchema>>(
    "research",
    attemptOutputSchema,
  ).prompt("Return an answer.");
  const execute = (attempt: number) =>
    flow<string>("numbering")
      .beginsWith(research.with({ attempt }))
      .after(research, (outcome) =>
        outcome.ok
          ? complete(outcome.value.answer)
          : fail(outcome.failure.intoError()),
      )
      .runWith(runtime, new RunConfig().debugDirectory(debug));

  await execute(1);
  await execute(2);

  const invocation = JSON.parse(
    await readFile(join(debug, "002-research/invocation.json"), "utf8"),
  ) as { invocation: number };
  assert.equal(invocation.invocation, 2);
});
