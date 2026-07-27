import assert from "node:assert/strict";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  Access,
  Internet,
  type RuntimeRequest,
} from "brain-js";
import { CodexRuntime } from "brain-js-codex";

import { commandArguments, parseEvents } from "../src/command.ts";

function request(
  access: (typeof Access)[keyof typeof Access],
  internet: (typeof Internet)[keyof typeof Internet],
  workingDirectory = ".",
): RuntimeRequest {
  return {
    flowName: "investigate",
    nodeName: "research",
    invocation: 1,
    prompt: "Research this",
    outputSchema: { type: "object" },
    workingDirectory,
    access,
    internet,
  };
}

test("maps access and internet settings to Codex arguments", () => {
  const arguments_ = commandArguments(
    request(Access.WorkspaceWrite, Internet.Enabled),
    "schema.json",
    "response.json",
  );

  assert.deepEqual(
    arguments_.slice(arguments_.indexOf("--sandbox"), arguments_.indexOf("--sandbox") + 2),
    ["--sandbox", "workspace-write"],
  );
  assert.equal(arguments_.includes("--search"), true);
  assert.equal(arguments_.includes("--ephemeral"), true);
  assert.equal(arguments_.includes("--output-schema"), true);
});

test("disables web search and preserves non-JSON stdout", () => {
  const arguments_ = commandArguments(
    request(Access.ReadOnly, Internet.Disabled),
    "schema.json",
    "response.json",
  );
  assert.equal(arguments_.includes('web_search="disabled"'), true);

  const events = parseEvents('{"type":"turn.started"}\nwarning\n') as {
    type: string;
  }[];
  assert.equal(events[0]?.type, "turn.started");
  assert.equal(events[1]?.type, "brain.codex.unparsed_stdout");
});

test("invokes an executable and returns its response and diagnostics", async (t) => {
  const temporary = await mkdtemp(join(tmpdir(), "brain-js-codex-test-"));
  t.after(() => rm(temporary, { force: true, recursive: true }));
  const executable = join(temporary, "fake-codex");
  await writeFile(
    executable,
    `#!/usr/bin/env node
import { writeFileSync } from "node:fs";
const responseFlag = process.argv.indexOf("--output-last-message");
writeFileSync(process.argv[responseFlag + 1], JSON.stringify({ answer: "ok" }));
console.log(JSON.stringify({ type: "turn.completed" }));
`,
  );
  await chmod(executable, 0o755);

  const response = await new CodexRuntime()
    .executable(executable)
    .invoke(request(Access.ReadOnly, Internet.Disabled, temporary));

  assert.equal(response.output, '{"answer":"ok"}');
  assert.deepEqual(response.events, [{ type: "turn.completed" }]);
});
