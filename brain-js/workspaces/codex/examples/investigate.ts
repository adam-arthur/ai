import { z } from "zod";

import { Internet, complete, fail, flow, next, node } from "brain-js";
import { CodexRuntime } from "brain-js-codex";

interface ResearchInput {
  topic: string;
}

const researchResult = z.object({ finding: z.string() });

interface AnalysisInput {
  finding: string;
}

const analysisResult = z.object({
  report: z.string(),
  followUp: z.string().nullable(),
});

const topic = process.argv[2] ?? "typed agent workflows";
const research = node<ResearchInput, z.infer<typeof researchResult>>(
  "research",
  researchResult,
)
  .prompt("Research the topic and return one important, well-supported finding.")
  .internet(Internet.Enabled);
const analyze = node<AnalysisInput, z.infer<typeof analysisResult>>(
  "analyze",
  analysisResult,
).prompt(
  "Analyze the finding. Return a final report, or a focused follow-up topic if more research is needed.",
);
const analyzeAfterResearch = analyze.clone();
const researchAfterAnalysis = research.clone();

const run = await flow<string>("investigate")
  .beginsWith(research.with({ topic }))
  .after(research, (outcome) =>
    outcome.ok
      ? next(analyzeAfterResearch.with({ finding: outcome.value.finding }))
      : fail(outcome.failure.intoError()),
  )
  .after(analyze, (outcome) => {
    if (!outcome.ok) return fail(outcome.failure.intoError());
    return outcome.value.followUp === null
      ? complete(outcome.value.report)
      : next(researchAfterAnalysis.with({ topic: outcome.value.followUp }));
  })
  .run(new CodexRuntime());

console.log(run.output);

