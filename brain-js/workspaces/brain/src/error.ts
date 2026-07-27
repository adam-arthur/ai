export const InvocationErrorKind = {
  InvalidInput: "invalid_input",
  Runtime: "runtime",
  InvalidOutput: "invalid_output",
} as const;

export type InvocationErrorKind =
  (typeof InvocationErrorKind)[keyof typeof InvocationErrorKind];

/** An error produced while invoking or decoding one node. */
export class InvocationError extends Error {
  readonly kind: InvocationErrorKind;

  private constructor(kind: InvocationErrorKind, message: string) {
    super(message);
    this.name = "InvocationError";
    this.kind = kind;
  }

  static invalidInput(message: string): InvocationError {
    return new InvocationError(InvocationErrorKind.InvalidInput, message);
  }

  static runtime(message: string): InvocationError {
    return new InvocationError(InvocationErrorKind.Runtime, message);
  }

  static invalidOutput(message: string): InvocationError {
    return new InvocationError(InvocationErrorKind.InvalidOutput, message);
  }

  isInvalidInput(): boolean {
    return this.kind === InvocationErrorKind.InvalidInput;
  }

  isRuntime(): boolean {
    return this.kind === InvocationErrorKind.Runtime;
  }

  isInvalidOutput(): boolean {
    return this.kind === InvocationErrorKind.InvalidOutput;
  }
}

/** A consumer-selected failure that stops a flow. */
export class FlowFailure extends Error {
  constructor(message: string) {
    super(message);
    this.name = "FlowFailure";
  }
}

export const FlowErrorKind = {
  InvalidDefinition: "invalid_definition",
  Failed: "failed",
  Io: "io",
  TypeMismatch: "type_mismatch",
} as const;

export type FlowErrorKind = (typeof FlowErrorKind)[keyof typeof FlowErrorKind];

/** An error that prevents a flow from completing. */
export class FlowError extends Error {
  readonly kind: FlowErrorKind;
  readonly path: string | undefined;

  private constructor(
    kind: FlowErrorKind,
    message: string,
    options?: ErrorOptions & { path?: string },
  ) {
    super(message, options);
    this.name = "FlowError";
    this.kind = kind;
    this.path = options?.path;
  }

  static invalidDefinition(message: string): FlowError {
    return new FlowError(
      FlowErrorKind.InvalidDefinition,
      `invalid flow definition: ${message}`,
    );
  }

  static failed(failure: FlowFailure): FlowError {
    return new FlowError(FlowErrorKind.Failed, `flow failed: ${failure.message}`, {
      cause: failure,
    });
  }

  static io(path: string, cause: unknown): FlowError {
    const renderedPath = path;
    return new FlowError(
      FlowErrorKind.Io,
      `failed to access \`${renderedPath}\`: ${errorMessage(cause)}`,
      { cause, path: renderedPath },
    );
  }

  static typeMismatch(nodeName: string): FlowError {
    return new FlowError(
      FlowErrorKind.TypeMismatch,
      `internal flow type mismatch for node \`${nodeName}\``,
    );
  }
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
