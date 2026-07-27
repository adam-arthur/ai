export const Access = {
  ReadOnly: "read_only",
  WorkspaceWrite: "workspace_write",
  Full: "full",
} as const;

export type Access = (typeof Access)[keyof typeof Access];

export const Internet = {
  Disabled: "disabled",
  Enabled: "enabled",
} as const;

export type Internet = (typeof Internet)[keyof typeof Internet];

export interface RuntimeRequest {
  flowName: string;
  nodeName: string;
  invocation: number;
  prompt: string;
  outputSchema: unknown;
  workingDirectory: string;
  access: Access;
  internet: Internet;
}

export interface RuntimeDiagnostics {
  events?: readonly unknown[];
  stdout?: string;
  stderr?: string;
}

/** The observable result of a successful runtime invocation. */
export class RuntimeResponse {
  readonly output: string;
  readonly events: readonly unknown[];
  readonly stdout: string;
  readonly stderr: string;

  constructor(output: string, diagnostics: RuntimeDiagnostics = {}) {
    this.output = output;
    this.events = diagnostics.events ?? [];
    this.stdout = diagnostics.stdout ?? "";
    this.stderr = diagnostics.stderr ?? "";
  }
}

/** A failed runtime invocation together with any observable diagnostics. */
export class RuntimeError extends Error {
  readonly events: readonly unknown[];
  readonly stdout: string;
  readonly stderr: string;

  constructor(message: string, diagnostics: RuntimeDiagnostics = {}) {
    super(message);
    this.name = "RuntimeError";
    this.events = diagnostics.events ?? [];
    this.stdout = diagnostics.stdout ?? "";
    this.stderr = diagnostics.stderr ?? "";
  }

  withDiagnostics(diagnostics: RuntimeDiagnostics): RuntimeError {
    return new RuntimeError(this.message, diagnostics);
  }
}

/** Executes one fully assembled agent invocation. */
export interface AgentRuntime {
  invoke(request: RuntimeRequest): Promise<RuntimeResponse>;
}
