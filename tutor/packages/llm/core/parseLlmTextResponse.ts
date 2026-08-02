export { parseLlmTextResponse }

import * as z from 'zod/v4'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmTextFormat, LlmTextResponse } from '#llm/core/types.ts'

function parseLlmTextResponse<TFormat extends LlmTextFormat | undefined = undefined>(
  args: ParseLlmTextResponseArgs<TFormat>,
): LlmTextResponse<TFormat> {
  return {
    text: args.text,
    ...(args.format ? { object: args.format.parse(JSON.parse(args.text)) } : {}),
  } as LlmTextResponse<TFormat>
}

type ParseLlmTextResponseArgs<TFormat extends LlmTextFormat | undefined = undefined> = {
  text: string
  format: TFormat | undefined
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('returns plain text responses unchanged', () => {
    assert.deepEqual(parseLlmTextResponse({ text: 'plain text', format: undefined }), { text: 'plain text' })
  })

  test('parses structured responses with inferred Zod output types', () => {
    const response = parseLlmTextResponse({
      text: '{"answer":"yes","score":1}',
      format: z.object({
        answer: z.string(),
        score: z.number(),
      }),
    })
    const object: { answer: string; score: number } = response.object

    assert.deepEqual(object, { answer: 'yes', score: 1 })
  })
})
