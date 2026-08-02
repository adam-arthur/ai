export { toOpenAiTextReasoning }

import type { Reasoning } from 'openai/resources/shared'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmThinkingLevel } from '#llm/core/types.ts'

function toOpenAiTextReasoning(thinkingLevel: LlmThinkingLevel | undefined): OpenAiTextReasoning {
  return {
    effort: thinkingLevel ?? 'none',
  }
}

type OpenAiTextReasoning = Reasoning

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('disables OpenAI text reasoning when no thinking level is requested', () => {
    assert.deepEqual(toOpenAiTextReasoning(undefined), {
      effort: 'none',
    })
  })

  test('maps requested thinking levels to OpenAI text reasoning effort', () => {
    assert.deepEqual(toOpenAiTextReasoning('high'), {
      effort: 'high',
    })
  })
})
