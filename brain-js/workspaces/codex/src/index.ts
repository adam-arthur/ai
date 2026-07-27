import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  RuntimeError,
  RuntimeResponse,
  type AgentRuntime,
  type RuntimeRequest,
} from "brain-js";

import { commandArguments, parseEvents } from "./command.ts";

interface ProcessOutput {
  readonly code: number | null;
  readonly signal: NodeJS.Signals | null;
  readonly stdout: string;
  readonly stderr: string;
}

/** Invokes a locally installed Codex CLI in ephemeral, non-interactive mode. */
export class CodexRuntime implements AgentRuntime {
  readonly #executable: string;

  constructor(executable = "codex") {
    this.#executable = executable;
  }

  executable(executable: string): CodexRuntime {
    return new CodexRuntime(executable);
  }

  async invoke(request: RuntimeRequest): Promise<RuntimeResponse> {
    let temporary: string;
    try {
      temporary = await mkdtemp(join(tmpdir(), "brain-codex-"));
    } catch (error) {
      throw new RuntimeError(
        `failed to create temporary Codex files: ${message(error)}`,
      );
    }

    try {
      const schemaPath = join(temporary, "output.schema.json");
      const responsePath = join(temporary, "response.json");
      try {
        await writeFile(schemaPath, JSON.stringify(request.outputSchema, null, 2));
      } catch (error) {
        throw new RuntimeError(`failed to write output schema: ${message(error)}`);
      }

      let output: ProcessOutput;
      try {
        output = await runProcess(
          this.#executable,
          commandArguments(request, schemaPath, responsePath),
          request.workingDirectory,
        );
      } catch (error) {
        throw new RuntimeError(
          `failed to launch \`${this.#executable}\`: ${message(error)}`,
        );
      }

      const events = parseEvents(output.stdout);
      const diagnostics = {
        events,
        stdout: output.stdout,
        stderr: output.stderr,
      };
      if (output.code !== 0) {
        const status =
          output.code === null
            ? `terminated by signal ${output.signal ?? "unknown"}`
            : `exit code ${output.code}`;
        throw new RuntimeError(`Codex failed with ${status}`, diagnostics);
      }

      let finalResponse: string;
      try {
        finalResponse = await readFile(responsePath, "utf8");
      } catch (error) {
        throw new RuntimeError(
          `Codex did not produce a final response: ${message(error)}`,
          diagnostics,
        );
      }
      if (finalResponse.trim() === "") {
        throw new RuntimeError("Codex produced an empty final response", diagnostics);
      }
      return new RuntimeResponse(finalResponse, diagnostics);
    } finally {
      await rm(temporary, { force: true, recursive: true }).catch(() => undefined);
    }
  }
}

function runProcess(
  executable: string,
  arguments_: readonly string[],
  workingDirectory: string,
): Promise<ProcessOutput> {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, arguments_, {
      cwd: workingDirectory,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.once("error", reject);
    child.once("close", (code, signal) => {
      resolve({ code, signal, stdout, stderr });
    });
  });
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
