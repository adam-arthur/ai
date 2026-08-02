export { createOpenAiSpeechSynthesisModel }

import OpenAI, { APIError } from 'openai'
import type { SpeechCreateParams } from 'openai/resources/audio/speech'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmAudioFormat, LlmSpeechSynthesisModel, LlmSpeechSynthesisRequest, LlmSpeechSynthesisResponse } from '#llm/core/types.ts'

const defaultOpenAiSpeechSynthesisOutputMimeType = 'audio/pcm;rate=24000'

function createOpenAiSpeechSynthesisModel(args: OpenAiSpeechSynthesisModelOptions): OpenAiSpeechSynthesisModel {
  const client: OpenAiSpeechSynthesisClient = args.client ?? new OpenAI({ apiKey: args.apiKey })

  return {
    async synthesizeSpeech(args: OpenAiSpeechSynthesisRequest): Promise<OpenAiSpeechSynthesisResponse> {
      try {
        return {
          audio: {
            data: new Uint8Array(
              await (
                await client.audio.speech.create({
                  input: args.text,
                  model: args.model,
                  voice: args.voiceName,
                  response_format: toOpenAiSpeechResponseFormat(args.outputAudio),
                  ...(args.instructions ? { instructions: args.instructions } : {}),
                  ...(typeof args.speechSpeed === 'number' ? { speed: toOpenAiSpeechSpeed(args.speechSpeed) } : {}),
                } satisfies OpenAiSpeechSynthesisApiRequest)
              ).arrayBuffer(),
            ),
            mimeType: toOpenAiSpeechOutputMimeType(args.outputAudio),
          },
        }
      } catch (error) {
        if (error instanceof APIError) {
          throw new Error(`OpenAI speech synthesis request failed (${error.status ?? 'unknown'}): ${getOpenAiApiErrorMessage(error)}`)
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

function toOpenAiSpeechResponseFormat(args: OpenAiSpeechSynthesisOutputAudio | undefined): OpenAiSpeechSynthesisResponseFormat {
  switch ((args?.mimeType ?? defaultOpenAiSpeechSynthesisOutputMimeType).split(';')[0]) {
    case 'audio/aac':
      return 'aac'
    case 'audio/flac':
    case 'audio/x-flac':
      return 'flac'
    case 'audio/mpeg':
    case 'audio/mp3':
      return 'mp3'
    case 'audio/ogg':
    case 'audio/opus':
      return 'opus'
    case 'audio/pcm':
      return 'pcm'
    case 'audio/wav':
    case 'audio/wave':
    case 'audio/x-wav':
      return 'wav'
  }

  throw new Error(`Unsupported OpenAI speech synthesis audio format "${args?.mimeType ?? defaultOpenAiSpeechSynthesisOutputMimeType}".`)
}

function toOpenAiSpeechOutputMimeType(args: OpenAiSpeechSynthesisOutputAudio | undefined): string {
  if (!args) {
    return defaultOpenAiSpeechSynthesisOutputMimeType
  }

  if (args.sampleRateHertz && args.mimeType.split(';')[0] === 'audio/pcm' && !args.mimeType.match(/(?:^|;)rate=\d+(?:;|$)/)) {
    return `${args.mimeType};rate=${args.sampleRateHertz}`
  }

  return args.mimeType
}

function toOpenAiSpeechSpeed(speechSpeed: number): number {
  if (!Number.isFinite(speechSpeed) || speechSpeed < 0.25 || speechSpeed > 4) {
    throw new Error('OpenAI speech synthesis speed must be between 0.25 and 4.')
  }

  return speechSpeed
}

type OpenAiSpeechSynthesisModel = LlmSpeechSynthesisModel

type OpenAiSpeechSynthesisRequest = LlmSpeechSynthesisRequest

type OpenAiSpeechSynthesisResponse = LlmSpeechSynthesisResponse

type OpenAiSpeechSynthesisOutputAudio = LlmAudioFormat

type OpenAiSpeechSynthesisModelOptions = {
  apiKey: string
  client?: OpenAiSpeechSynthesisClient
}

type OpenAiSpeechSynthesisClient = {
  audio: {
    speech: {
      create(args: OpenAiSpeechSynthesisApiRequest): Promise<Response>
    }
  }
}

type OpenAiSpeechSynthesisApiRequest = SpeechCreateParams & {
  model: OpenAiSpeechSynthesisRequest['model']
}

type OpenAiSpeechSynthesisResponseFormat = NonNullable<SpeechCreateParams['response_format']>

type OpenAiApiErrorBody = {
  message: string
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('synthesizes speech with the Audio API client', async () => {
    const requests: OpenAiSpeechSynthesisApiRequest[] = []

    assert.deepEqual(
      await createOpenAiSpeechSynthesisModel({
        apiKey: 'test-api-key',
        client: {
          audio: {
            speech: {
              async create(args) {
                requests.push(args)

                return new Response(Uint8Array.from([1, 2, 3]))
              },
            },
          },
        },
      }).synthesizeSpeech({
        model: 'gpt-4o-mini-tts',
        text: '안녕하세요.',
        voiceName: 'marin',
        instructions: 'Speak warmly and clearly.',
        outputAudio: {
          mimeType: 'audio/pcm',
          sampleRateHertz: 24000,
        },
        speechSpeed: 1.1,
      }),
      {
        audio: {
          data: Uint8Array.from([1, 2, 3]),
          mimeType: 'audio/pcm;rate=24000',
        },
      },
    )
    assert.deepEqual(requests, [
      {
        input: '안녕하세요.',
        model: 'gpt-4o-mini-tts',
        voice: 'marin',
        response_format: 'pcm',
        instructions: 'Speak warmly and clearly.',
        speed: 1.1,
      },
    ])
  })

  test('reports unsupported OpenAI speech synthesis audio formats', async () => {
    await assert.rejects(
      async () =>
        await createOpenAiSpeechSynthesisModel({
          apiKey: 'test-api-key',
          client: {
            audio: {
              speech: {
                async create() {
                  return new Response()
                },
              },
            },
          },
        }).synthesizeSpeech({
          model: 'gpt-4o-mini-tts',
          text: 'Hello.',
          voiceName: 'marin',
          outputAudio: {
            mimeType: 'audio/webm',
          },
        }),
      { message: 'Unsupported OpenAI speech synthesis audio format "audio/webm".' },
    )
  })

  test('reports unsupported OpenAI speech synthesis speeds', async () => {
    await assert.rejects(
      async () =>
        await createOpenAiSpeechSynthesisModel({
          apiKey: 'test-api-key',
          client: {
            audio: {
              speech: {
                async create() {
                  return new Response()
                },
              },
            },
          },
        }).synthesizeSpeech({
          model: 'gpt-4o-mini-tts',
          text: 'Hello.',
          voiceName: 'marin',
          speechSpeed: 4.1,
        }),
      { message: 'OpenAI speech synthesis speed must be between 0.25 and 4.' },
    )
  })

  test('reports OpenAI speech synthesis API failures', async () => {
    await assert.rejects(
      async () =>
        await createOpenAiSpeechSynthesisModel({
          apiKey: 'test-api-key',
          client: {
            audio: {
              speech: {
                async create() {
                  throw APIError.generate(401, { error: { message: 'Invalid API key.' } }, 'Unauthorized', new Headers())
                },
              },
            },
          },
        }).synthesizeSpeech({
          model: 'gpt-4o-mini-tts',
          text: 'Hello.',
          voiceName: 'marin',
        }),
      { message: 'OpenAI speech synthesis request failed (401): Invalid API key.' },
    )
  })
})
