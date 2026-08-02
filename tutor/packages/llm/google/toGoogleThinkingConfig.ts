export { toGoogleThinkingConfig }

import { ThinkingLevel } from '@google/genai'
import type { ThinkingConfig } from '@google/genai'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmThinkingLevel } from '#llm/core/types.ts'

function toGoogleThinkingConfig(thinkingLevel: LlmThinkingLevel | undefined): ThinkingConfig {
  switch (thinkingLevel) {
    case undefined:
      return {
        thinkingBudget: 0,
      }
    case 'low':
      return {
        thinkingLevel: ThinkingLevel.LOW,
      }
    case 'medium':
      return {
        thinkingLevel: ThinkingLevel.MEDIUM,
      }
    case 'high':
      return {
        thinkingLevel: ThinkingLevel.HIGH,
      }
  }
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('disables Gemini thinking when no thinking level is requested', () => {
    assert.deepEqual(toGoogleThinkingConfig(undefined), {
      thinkingBudget: 0,
    })
  })

  test('maps requested thinking levels to Gemini thinking levels', () => {
    assert.deepEqual(toGoogleThinkingConfig('low'), {
      thinkingLevel: ThinkingLevel.LOW,
    })
    assert.deepEqual(toGoogleThinkingConfig('medium'), {
      thinkingLevel: ThinkingLevel.MEDIUM,
    })
    assert.deepEqual(toGoogleThinkingConfig('high'), {
      thinkingLevel: ThinkingLevel.HIGH,
    })
  })
})
