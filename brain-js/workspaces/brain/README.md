# brain-js

`brain-js` is the runtime-neutral core of the `brain-js` workspace. It provides
typed sequential workflows through `node`, `flow`, `next`, `complete`, and
`fail`. Node outputs are validated with Zod, and any `AgentRuntime`
implementation can execute the assembled requests.

```ts
import { z } from "zod";
import { complete, fail, flow, node } from "brain-js";

const result = z.object({ answer: z.string() });
const answer = node<{ question: string }, z.infer<typeof result>>(
  "answer",
  result,
).prompt("Answer the supplied question.");

const run = await flow<string>("answer-question")
  .beginsWith(answer.with({ question: "Why use typed workflows?" }))
  .after(answer, (outcome) =>
    outcome.ok
      ? complete(outcome.value.answer)
      : fail(outcome.failure.intoError()),
  )
  .run(runtime);
```

See the repository-level README for execution semantics, trace contents, and a
complete Codex example.

