# brain-js

`brain-js` is the runtime-neutral core of the `brain-js` workspace. It provides
typed sequential workflows through `node`, `flow`, `next`, `complete`, and
`fail`. Node inputs and outputs are validated with Zod, and any `AgentRuntime`
implementation can execute the assembled requests.

```ts
import { z } from "zod";
import { complete, fail, flow, node } from "brain-js";

const answer = node({
  name: "answer",
  input: z.object({ question: z.string() }),
  output: z.object({ answer: z.string() }),
  prompt: "Answer the supplied question.",
});

const run = await flow<string>("answer-question")
  .startWith(answer.withInput({ question: "Why use typed workflows?" }))
  .on(answer, (result) =>
    result.ok ? complete(result.value.answer) : fail(result.error),
  )
  .run(runtime);
```

See the repository-level README for execution semantics, trace contents, and a
complete Codex example.
