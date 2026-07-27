import type { RuntimeRequest } from "brain-js";

export function commandArguments(
  request: RuntimeRequest,
  schemaPath: string,
  responsePath: string,
): string[] {
  const arguments_ = ["--ask-for-approval", "never"];
  if (request.internet === "disabled") {
    arguments_.push("--config", 'web_search="disabled"');
  } else {
    arguments_.push("--search");
  }
  arguments_.push(
    "exec",
    "--ephemeral",
    "--json",
    "--color",
    "never",
    "--sandbox",
    {
      read_only: "read-only",
      workspace_write: "workspace-write",
      full: "danger-full-access",
    }[request.access],
    "--output-schema",
    schemaPath,
    "--output-last-message",
    responsePath,
    request.prompt,
  );
  return arguments_;
}

export function parseEvents(stdout: string): unknown[] {
  return stdout
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => {
      try {
        return JSON.parse(line) as unknown;
      } catch {
        return { type: "brain.codex.unparsed_stdout", line };
      }
    });
}

