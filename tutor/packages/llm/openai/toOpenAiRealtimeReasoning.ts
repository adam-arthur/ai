export { toOpenAiRealtimeReasoning }

import type { RealtimeReasoning } from 'openai/resources/realtime/realtime'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmThinkingLevel } from '#llm/core/types.ts'

function toOpenAiRealtimeReasoning(thinkingLevel: LlmThinkingLevel): OpenAiRealtimeReasoning {
  return {
    effort: thinkingLevel,
  }
}

type OpenAiRealtimeReasoning = RealtimeReasoning

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('maps requested thinking levels to OpenAI Realtime reasoning effort', () => {
    assert.deepEqual(toOpenAiRealtimeReasoning('medium'), {
      effort: 'medium',
    })
  })
})
