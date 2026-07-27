import { mkdir, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { toJSONSchema } from "zod";

import {
  FlowError,
  FlowFailure,
  InvocationError,
  errorMessage,
} from "./error.ts";
import {
  NodeFailure,
  type Node,
  type NodeInvocation,
  type NodeOutcome,
  type NodeSpec,
} from "./node.ts";
import {
  type AgentRuntime,
  RuntimeError,
  type RuntimeRequest,
  type RuntimeResponse,
} from "./runtime.ts";

type AnyInvocation = NodeInvocation<unknown, unknown>;

type TransitionKind<W> =
  | { readonly type: "next"; readonly invocation: AnyInvocation }
  | { readonly type: "complete"; readonly output: W }
  | { readonly type: "fail"; readonly failure: FlowFailure };

/** The next action selected by a deterministic node handler. */
export class Transition<out W> {
  readonly _kind: TransitionKind<W>;

  /** @internal */
  constructor(kind: TransitionKind<W>) {
    this._kind = kind;
  }
}

/** Routes execution to one node invocation. */
export function next<I, O>(invocation: NodeInvocation<I, O>): Transition<never> {
  return new Transition({
    type: "next",
    invocation: invocation as AnyInvocation,
  });
}

/** Completes a flow with its final typed output. */
export function complete<W>(output: W): Transition<W> {
  return new Transition({ type: "complete", output });
}

/** Stops a flow with a consumer-selected failure. */
export function fail(
  error: FlowFailure | InvocationError | Error | string,
): Transition<never> {
  const failure =
    error instanceof FlowFailure ? error : new FlowFailure(errorMessage(error));
  return new Transition({ type: "fail", failure });
}

export interface RunConfigOptions {
  workingDirectory?: string;
  debugDirectory?: string;
}

/** Filesystem settings for one sequential flow run. */
export class RunConfig {
  readonly workingDirectoryPath: string;
  readonly debugDirectoryPath: string;

  constructor(options: RunConfigOptions = {}) {
    this.workingDirectoryPath = options.workingDirectory ?? ".";
    this.debugDirectoryPath = options.debugDirectory ?? "debug";
  }

  workingDirectory(path: string): RunConfig {
    return new RunConfig({
      workingDirectory: path,
      debugDirectory: this.debugDirectoryPath,
    });
  }

  debugDirectory(path: string): RunConfig {
    return new RunConfig({
      workingDirectory: this.workingDirectoryPath,
      debugDirectory: path,
    });
  }
}

/** A completed flow and its final typed value. */
export interface FlowRun<W> {
  readonly name: string;
  readonly output: W;
  /** Number of node invocations performed by this run. */
  readonly invocations: number;
}

interface HandlerRegistration<W> {
  readonly spec: NodeSpec<unknown>;
  readonly handle: (outcome: NodeOutcome<unknown, unknown>) => Transition<W>;
}

/** A readable, single-path agent workflow. */
export class Flow<W> {
  readonly #name: string;
  #initial?: AnyInvocation;
  readonly #handlers = new Map<string, HandlerRegistration<W>>();
  readonly #definitionErrors: string[] = [];

  constructor(name: string) {
    this.#name = name;
  }

  /** Selects the first node invocation in the flow. */
  beginsWith<I, O>(invocation: NodeInvocation<I, O>): this {
    if (this.#initial !== undefined) {
      this.#definitionErrors.push("a flow can only have one initial invocation");
    } else {
      this.#initial = invocation as AnyInvocation;
    }
    return this;
  }

  /** Registers the single function that handles success and failure for a node. */
  after<I, O>(
    node: Node<I, O>,
    handler: (outcome: NodeOutcome<I, O>) => Transition<W>,
  ): this {
    const name = node.name;
    if (this.#handlers.has(name)) {
      this.#definitionErrors.push(
        `node \`${name}\` has more than one \`after\` handler`,
      );
      return this;
    }
    this.#handlers.set(name, {
      spec: node._spec as NodeSpec<unknown>,
      handle: handler as (
        outcome: NodeOutcome<unknown, unknown>,
      ) => Transition<W>,
    });
    return this;
  }

  /** Runs in the current directory and writes traces beneath `debug`. */
  async run(runtime: AgentRuntime): Promise<FlowRun<W>> {
    return this.runWith(runtime, new RunConfig());
  }

  /** Runs with explicit working and debug directories. */
  async runWith(
    runtime: AgentRuntime,
    config: RunConfig,
  ): Promise<FlowRun<W>> {
    this.validate();
    let current = this.#initial as AnyInvocation;
    let sequence = await prepareDebugDirectory(config.debugDirectoryPath);
    let runInvocations = 0;

    for (;;) {
      const registration = this.registrationFor(current);
      sequence += 1;
      runInvocations += 1;

      const nodeName = current.node.name;
      const invocationDirectory = join(
        config.debugDirectoryPath,
        `${String(sequence).padStart(3, "0")}-${nodeName}`,
      );
      await io(invocationDirectory, mkdir(invocationDirectory));

      await writeJson(join(invocationDirectory, "invocation.json"), {
        flow: this.#name,
        node: nodeName,
        invocation: sequence,
        access: current.node._spec.access,
        internet: current.node._spec.internet,
        working_directory: config.workingDirectoryPath,
      });

      let outputSchema: unknown;
      try {
        outputSchema = toJSONSchema(current.node._spec.outputSchema);
      } catch (error) {
        throw FlowError.invalidDefinition(
          `failed to generate the output schema for node \`${nodeName}\`: ${errorMessage(error)}`,
        );
      }
      await writeJson(join(invocationDirectory, "output.schema.json"), outputSchema);

      const { outcome, parsedOutput } = await this.invoke(
        current,
        runtime,
        config,
        invocationDirectory,
        outputSchema,
        sequence,
      );
      if (parsedOutput !== undefined) {
        await writeJson(join(invocationDirectory, "response.json"), parsedOutput);
      }

      const transition = registration.handle(outcome);
      if (!(transition instanceof Transition)) {
        throw FlowError.typeMismatch(nodeName);
      }
      await recordTransition(invocationDirectory, transition);

      switch (transition._kind.type) {
        case "next":
          current = transition._kind.invocation;
          break;
        case "complete":
          return {
            name: this.#name,
            output: transition._kind.output,
            invocations: runInvocations,
          };
        case "fail":
          throw FlowError.failed(transition._kind.failure);
      }
    }
  }

  private async invoke(
    invocation: AnyInvocation,
    runtime: AgentRuntime,
    config: RunConfig,
    invocationDirectory: string,
    outputSchema: unknown,
    sequence: number,
  ): Promise<{
    outcome: NodeOutcome<unknown, unknown>;
    parsedOutput?: unknown;
  }> {
    let input: unknown;
    try {
      input = jsonValue(invocation.input);
    } catch (error) {
      await writeText(join(invocationDirectory, "input.error.txt"), errorMessage(error));
      return {
        outcome: {
          ok: false,
          failure: new NodeFailure(
            invocation.input,
            InvocationError.invalidInput(
              `failed to serialize node input: ${errorMessage(error)}`,
            ),
            sequence,
          ),
        },
      };
    }
    await writeJson(join(invocationDirectory, "input.json"), input);

    const prompt = assemblePrompt(invocation.node._spec.prompt, input);
    await writeText(join(invocationDirectory, "prompt.md"), prompt);
    const request: RuntimeRequest = {
      flowName: this.#name,
      nodeName: invocation.node.name,
      invocation: sequence,
      prompt,
      outputSchema,
      workingDirectory: config.workingDirectoryPath,
      access: invocation.node._spec.access,
      internet: invocation.node._spec.internet,
    };

    let response: RuntimeResponse;
    try {
      response = await runtime.invoke(request);
    } catch (error) {
      const runtimeError =
        error instanceof RuntimeError
          ? error
          : new RuntimeError(errorMessage(error));
      await recordRuntimeFailure(invocationDirectory, runtimeError);
      return {
        outcome: {
          ok: false,
          failure: new NodeFailure(
            invocation.input,
            InvocationError.runtime(runtimeError.message),
            sequence,
          ),
        },
      };
    }
    await recordRuntimeSuccess(invocationDirectory, response);

    let value: unknown;
    try {
      value = JSON.parse(response.output);
    } catch (error) {
      return {
        outcome: {
          ok: false,
          failure: new NodeFailure(
            invocation.input,
            InvocationError.invalidOutput(
              `node returned invalid JSON: ${errorMessage(error)}`,
            ),
            sequence,
          ),
        },
      };
    }

    const parsed = invocation.node._spec.outputSchema.safeParse(value);
    if (!parsed.success) {
      return {
        outcome: {
          ok: false,
          failure: new NodeFailure(
            invocation.input,
            InvocationError.invalidOutput(
              `node output did not match its TypeScript schema: ${parsed.error.message}`,
            ),
            sequence,
          ),
        },
        parsedOutput: value,
      };
    }
    return {
      outcome: { ok: true, value: parsed.data },
      parsedOutput: value,
    };
  }

  private validate(): void {
    if (this.#name.trim() === "") {
      throw FlowError.invalidDefinition("flow names cannot be empty");
    }
    if (this.#definitionErrors.length > 0) {
      throw FlowError.invalidDefinition(this.#definitionErrors.join("; "));
    }
    if (this.#initial === undefined) {
      throw FlowError.invalidDefinition("the flow has no initial invocation");
    }
    for (const registration of this.#handlers.values()) {
      validateNode(registration.spec);
    }
    this.registrationFor(this.#initial);
  }

  private registrationFor(invocation: AnyInvocation): HandlerRegistration<W> {
    const name = invocation.node.name;
    const registration = this.#handlers.get(name);
    if (registration === undefined) {
      throw FlowError.invalidDefinition(
        `node \`${name}\` has no \`after\` handler`,
      );
    }
    if (registration.spec !== invocation.node._spec) {
      throw FlowError.invalidDefinition(
        `the invocation for node \`${name}\` did not use its registered node handle`,
      );
    }
    return registration;
  }
}

/** Creates an empty typed flow definition. */
export function flow<W>(name: string): Flow<W> {
  return new Flow(name);
}

function validateNode(spec: NodeSpec<unknown>): void {
  if (!/^[A-Za-z0-9_-]+$/.test(spec.name)) {
    throw FlowError.invalidDefinition(
      `node name \`${spec.name}\` must contain only ASCII letters, digits, \`-\`, or \`_\``,
    );
  }
  if (spec.prompt.trim() === "") {
    throw FlowError.invalidDefinition(`node \`${spec.name}\` has an empty prompt`);
  }
}

async function prepareDebugDirectory(path: string): Promise<number> {
  await io(path, mkdir(path, { recursive: true }));
  const entries = await io(path, readdir(path));
  let highest = 0;
  for (const entry of entries) {
    const match = /^(\d+)-/.exec(entry);
    if (match?.[1] !== undefined) {
      const sequence = Number(match[1]);
      if (Number.isSafeInteger(sequence)) highest = Math.max(highest, sequence);
    }
  }
  return highest;
}

function jsonValue(value: unknown): unknown {
  const encoded = JSON.stringify(value);
  if (encoded === undefined) {
    throw new TypeError("the value is not representable as JSON");
  }
  return JSON.parse(encoded);
}

function assemblePrompt(nodePrompt: string, input: unknown): string {
  return `${nodePrompt.trim()}\n\nNode input (JSON):\n\`\`\`json\n${JSON.stringify(input, null, 2)}\n\`\`\`\n\nReturn only the JSON result for this node.`;
}

async function recordRuntimeSuccess(
  directory: string,
  response: RuntimeResponse,
): Promise<void> {
  await writeText(join(directory, "stdout.log"), response.stdout);
  await writeText(join(directory, "stderr.log"), response.stderr);
  await writeJsonLines(join(directory, "runtime-events.jsonl"), response.events);
  await writeText(join(directory, "response.raw.txt"), response.output);
}

async function recordRuntimeFailure(
  directory: string,
  error: RuntimeError,
): Promise<void> {
  await writeText(join(directory, "stdout.log"), error.stdout);
  await writeText(join(directory, "stderr.log"), error.stderr);
  await writeJsonLines(join(directory, "runtime-events.jsonl"), error.events);
  await writeText(join(directory, "runtime.error.txt"), error.message);
}

async function recordTransition<W>(
  directory: string,
  transition: Transition<W>,
): Promise<void> {
  switch (transition._kind.type) {
    case "next":
      await writeJson(join(directory, "transition.json"), {
        type: "next",
        node: transition._kind.invocation.node.name,
      });
      return;
    case "complete":
      await writeJson(join(directory, "transition.json"), { type: "complete" });
      return;
    case "fail":
      await writeJson(join(directory, "transition.json"), {
        type: "fail",
        error: transition._kind.failure.message,
      });
  }
}

async function writeText(path: string, contents: string): Promise<void> {
  await io(path, writeFile(path, contents));
}

async function writeJson(path: string, value: unknown): Promise<void> {
  let contents: string;
  try {
    contents = `${JSON.stringify(value, null, 2)}\n`;
  } catch (error) {
    throw FlowError.invalidDefinition(
      `failed to encode debug JSON: ${errorMessage(error)}`,
    );
  }
  await io(path, writeFile(path, contents));
}

async function writeJsonLines(
  path: string,
  values: readonly unknown[],
): Promise<void> {
  let contents: string;
  try {
    contents = values.map((value) => JSON.stringify(value)).join("\n");
    if (contents !== "") contents += "\n";
  } catch (error) {
    throw FlowError.invalidDefinition(
      `failed to encode runtime event: ${errorMessage(error)}`,
    );
  }
  await io(path, writeFile(path, contents));
}

async function io<T>(path: string, operation: Promise<T>): Promise<T> {
  try {
    return await operation;
  } catch (error) {
    throw FlowError.io(path, error);
  }
}
