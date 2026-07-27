import { z } from "zod";

import { complete, fail, flow, next, node } from "brain-js";
import { CodexRuntime } from "brain-js-codex";

const research = node({
  name: "research",
  input: z.object({ topic: z.string() }),
  output: z.object({ finding: z.string() }),
  prompt: "Research the topic and return one important, well-supported finding.",
  internet: true,
});

const analyze = node({
  name: "analyze",
  input: z.object({ finding: z.string() }),
  output: z.object({
    report: z.string(),
    followUp: z.string().nullable(),
  }),
  prompt:
    "Analyze the finding. Return a final report, or a focused follow-up topic if more research is needed.",
});

const topic = process.argv[2] ?? "typed agent workflows";

const run = await flow<string>("investigate")
  .startWith(research.withInput({ topic }))
  .on(research, (result) =>
    result.ok
      ? next(analyze.withInput({ finding: result.value.finding }))
      : fail(result.error),
  )
  .on(analyze, (result) => {
    if (!result.ok) return fail(result.error);
    return result.value.followUp === null
      ? complete(result.value.report)
      : next(research.withInput({ topic: result.value.followUp }));
  })
  .run(new CodexRuntime());

console.log(run.output);
