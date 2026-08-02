export { createOpenAiTextModel }

import OpenAI, { APIError } from 'openai'
import { zodTextFormat } from 'openai/helpers/zod'
import type {
  Response as OpenAiResponse,
  ResponseCreateParamsNonStreaming,
  ResponseOutputItem,
  ResponseOutputMessage,
  ResponseOutputText,
} from 'openai/resources/responses/responses'
import * as z from 'zod/v4'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import { parseLlmTextResponse } from '#llm/core/parseLlmTextResponse.ts'
import type { LlmTextFormat, LlmTextModel, LlmTextRequest, LlmTextResponse } from '#llm/core/types.ts'
import { toOpenAiTextReasoning } from '#llm/openai/toOpenAiTextReasoning.ts'

function createOpenAiTextModel(args: OpenAiTextModelOptions): OpenAiTextModel {
  const client: OpenAiTextClient = args.client ?? new OpenAI({ apiKey: args.apiKey })

  return {
    async generateText<TFormat extends LlmTextFormat | undefined = undefined>(
      args: OpenAiTextRequest<TFormat>,
    ): Promise<OpenAiTextResponse<TFormat>> {
      try {
        return parseLlmTextResponse({
          text: getOpenAiTextResponseText(
            await client.responses.create({
              model: args.model,
              input: args.prompt,
              reasoning: toOpenAiTextReasoning(args.thinkingLevel),
              store: false,
              ...(args.systemPrompt ? { instructions: args.systemPrompt } : {}),
              ...(args.maxOutputTokens === undefined ? {} : { max_output_tokens: args.maxOutputTokens }),
              ...(args.format ? { text: { format: zodTextFormat(args.format, 'response') } } : {}),
              ...(args.temperature === undefined ? {} : { temperature: args.temperature }),
            } satisfies OpenAiTextApiRequest),
          ),
          format: args.format,
        })
      } catch (error) {
        if (error instanceof APIError) {
          throw new Error(`OpenAI text model request failed (${error.status ?? 'unknown'}): ${getOpenAiApiErrorMessage(error)}`)
        }

        throw error
      }
    },
  }
}

function getOpenAiApiErrorMessage(error: APIError): string {
  return typeof (error.error as OpenAiApiErrorBody | undefined)?.message === 'string'
    ? (error.error as OpenAiApiErrorBody).message
    : error.message || 'Unknown error'
}

function getOpenAiTextResponseText(response: OpenAiTextApiResponse | undefined): string {
  return (
    response?.output_text ||
    (response?.output
      .flatMap(item => (isOpenAiOutputMessage(item) ? item.content : []))
      .filter(isOpenAiOutputTextContent)
      .map(content => content.text)
      .join('') ??
      '')
  )
}

function isOpenAiOutputMessage(item: OpenAiTextApiOutputItem): item is OpenAiTextApiOutputMessage {
  return item.type === 'message'
}

function isOpenAiOutputTextContent(content: OpenAiTextApiContent): content is OpenAiTextApiOutputTextContent {
  return content.type === 'output_text' && typeof content.text === 'string'
}

type OpenAiTextModel = LlmTextModel

type OpenAiTextRequest<TFormat extends LlmTextFormat | undefined = undefined> = LlmTextRequest<TFormat>

type OpenAiTextResponse<TFormat extends LlmTextFormat | undefined = undefined> = LlmTextResponse<TFormat>

type OpenAiTextModelOptions = {
  apiKey: string
  client?: OpenAiTextClient
}

type OpenAiTextClient = {
  responses: {
    create(args: OpenAiTextApiRequest): Promise<OpenAiTextApiResponse>
  }
}

type OpenAiTextApiRequest = ResponseCreateParamsNonStreaming & {
  input: string
  model: OpenAiTextRequest['model']
  reasoning: NonNullable<ResponseCreateParamsNonStreaming['reasoning']>
  store: false
}

type OpenAiTextApiResponse = Pick<OpenAiResponse, 'output' | 'output_text'>

type OpenAiTextApiOutputItem = ResponseOutputItem

type OpenAiTextApiOutputMessage = ResponseOutputMessage

type OpenAiTextApiContent = ResponseOutputMessage['content'][number]

type OpenAiTextApiOutputTextContent = ResponseOutputText

type OpenAiApiErrorBody = {
  message: string
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('generates text with the Responses API client', async () => {
    const requests: OpenAiTextApiRequest[] = []

    assert.deepEqual(
      await createOpenAiTextModel({
        apiKey: 'test-api-key',
        client: {
          responses: {
            async create(args) {
              requests.push(args)

              return {
                output_text: '',
                output: [
                  {
                    id: 'msg_123',
                    content: [{ annotations: [], text: 'OPENAI_MODEL_QUERY_OK', type: 'output_text' }],
                    role: 'assistant',
                    status: 'completed',
                    type: 'message',
                  },
                ],
              }
            },
          },
        },
      }).generateText({
        model: 'gpt-5.5',
        prompt: 'Reply with a token.',
        systemPrompt: 'Follow the instruction exactly.',
        maxOutputTokens: 32,
        temperature: 0,
      }),
      { text: 'OPENAI_MODEL_QUERY_OK' },
    )
    assert.deepEqual(requests, [
      {
        model: 'gpt-5.5',
        input: 'Reply with a token.',
        reasoning: { effort: 'none' },
        store: false,
        instructions: 'Follow the instruction exactly.',
        max_output_tokens: 32,
        temperature: 0,
      },
    ])
  })

  test('generates structured text with the Responses API client', async () => {
    const requests: OpenAiTextApiRequest[] = []
    const response = await createOpenAiTextModel({
      apiKey: 'test-api-key',
      client: {
        responses: {
          async create(args) {
            requests.push(args)

            return {
              output_text: '{"token":"OPENAI_MODEL_QUERY_OK"}',
              output: [],
            }
          },
        },
      },
    }).generateText({
      model: 'gpt-5.5',
      prompt: 'Reply with a JSON object.',
      format: z.object({
        token: z.string(),
      }),
    })
    const object: { token: string } = response.object

    assert.deepEqual(response, { text: '{"token":"OPENAI_MODEL_QUERY_OK"}', object: { token: 'OPENAI_MODEL_QUERY_OK' } })
    assert.deepEqual(object, { token: 'OPENAI_MODEL_QUERY_OK' })
    assert.equal(requests[0]?.text?.format?.type, 'json_schema')
    assert.equal(requests[0]?.text?.format?.type === 'json_schema' ? requests[0].text.format.name : undefined, 'response')
  })

  test('reports OpenAI API failures', async () => {
    await assert.rejects(
      async () =>
        await createOpenAiTextModel({
          apiKey: 'test-api-key',
          client: {
            responses: {
              async create() {
                throw APIError.generate(401, { error: { message: 'Invalid API key.' } }, 'Unauthorized', new Headers())
              },
            },
          },
        }).generateText({
          model: 'gpt-5.5',
          prompt: 'Reply with a token.',
        }),
      { message: 'OpenAI text model request failed (401): Invalid API key.' },
    )
  })
})
