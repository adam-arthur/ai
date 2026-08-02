export { toLlmTextResponseJsonSchema }

import * as z from 'zod/v4'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmTextFormat } from '#llm/core/types.ts'

function toLlmTextResponseJsonSchema(args: ToLlmTextResponseJsonSchemaArgs): Record<string, unknown> {
  const schema = z.toJSONSchema(args.format, { target: 'draft-7' }) as Record<string, unknown>
  delete schema['$schema']

  return schema
}

type ToLlmTextResponseJsonSchemaArgs = {
  format: LlmTextFormat
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('converts Zod response formats to JSON Schema', () => {
    assert.deepEqual(
      toLlmTextResponseJsonSchema({
        format: z.object({
          answer: z.string(),
        }),
      }),
      {
        type: 'object',
        properties: {
          answer: {
            type: 'string',
          },
        },
        required: ['answer'],
        additionalProperties: false,
      },
    )
  })
})
