export { createOpenAiSpeechModel }

import OpenAI, { APIError, toFile } from 'openai'
import type { Transcription, TranscriptionCreateParamsNonStreaming } from 'openai/resources/audio/transcriptions'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { LlmSpeechAudioInput, LlmSpeechModel, LlmSpeechRequest, LlmSpeechResponse } from '#llm/core/types.ts'

function createOpenAiSpeechModel(args: OpenAiSpeechModelOptions): OpenAiSpeechModel {
  const client: OpenAiSpeechClient = args.client ?? new OpenAI({ apiKey: args.apiKey })

  return {
    async transcribeSpeech(args: OpenAiSpeechRequest): Promise<OpenAiSpeechResponse> {
      try {
        return {
          text: (
            await client.audio.transcriptions.create({
              model: args.model,
              file: await toFile(args.audio.data, toOpenAiAudioFileName(args.audio), { type: args.audio.mimeType }),
              response_format: 'json',
              ...(args.prompt ? { prompt: args.prompt } : {}),
              ...(args.languageCode ? { language: args.languageCode } : {}),
              ...(args.temperature === undefined ? {} : { temperature: args.temperature }),
            } satisfies OpenAiSpeechApiRequest)
          ).text,
        }
      } catch (error) {
        if (error instanceof APIError) {
          throw new Error(`OpenAI speech transcription request failed (${error.status ?? 'unknown'}): ${getOpenAiApiErrorMessage(error)}`)
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

function toOpenAiAudioFileName(args: OpenAiSpeechAudioInput): string {
  switch (args.mimeType.split(';')[0]) {
    case 'audio/flac':
    case 'audio/x-flac':
      return 'audio.flac'
    case 'audio/m4a':
    case 'audio/mp4':
    case 'audio/x-m4a':
      return 'audio.m4a'
    case 'audio/mpeg':
    case 'audio/mp3':
      return 'audio.mp3'
    case 'audio/mpga':
      return 'audio.mpga'
    case 'audio/ogg':
      return 'audio.ogg'
    case 'audio/wav':
    case 'audio/wave':
    case 'audio/x-wav':
      return 'audio.wav'
    case 'audio/webm':
      return 'audio.webm'
  }

  throw new Error(`Unsupported OpenAI speech transcription audio format "${args.mimeType}".`)
}

type OpenAiSpeechModel = LlmSpeechModel

type OpenAiSpeechRequest = LlmSpeechRequest

type OpenAiSpeechResponse = LlmSpeechResponse

type OpenAiSpeechAudioInput = LlmSpeechAudioInput

type OpenAiSpeechModelOptions = {
  apiKey: string
  client?: OpenAiSpeechClient
}

type OpenAiSpeechClient = {
  audio: {
    transcriptions: {
      create(args: OpenAiSpeechApiRequest): Promise<OpenAiSpeechApiResponse>
    }
  }
}

type OpenAiSpeechApiRequest = TranscriptionCreateParamsNonStreaming<'json'> & {
  model: OpenAiSpeechRequest['model']
}

type OpenAiSpeechApiResponse = Pick<Transcription, 'text'>

type OpenAiApiErrorBody = {
  message: string
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('transcribes speech with the Audio API client', async () => {
    const requests: OpenAiSpeechApiRequest[] = []

    assert.deepEqual(
      await createOpenAiSpeechModel({
        apiKey: 'test-api-key',
        client: {
          audio: {
            transcriptions: {
              async create(args) {
                requests.push(args)

                return { text: 'OPENAI_TRANSCRIPTION_OK' }
              },
            },
          },
        },
      }).transcribeSpeech({
        model: 'gpt-4o-transcribe',
        audio: {
          data: new Uint8Array([1, 2, 3]),
          mimeType: 'audio/webm;codecs=opus',
        },
        prompt: 'Technical vocabulary appears in the recording.',
        languageCode: 'en',
        temperature: 0,
      }),
      { text: 'OPENAI_TRANSCRIPTION_OK' },
    )
    assert.equal(requests[0]?.model, 'gpt-4o-transcribe')
    assert.equal(requests[0]?.response_format, 'json')
    assert.equal(requests[0]?.prompt, 'Technical vocabulary appears in the recording.')
    assert.equal(requests[0]?.language, 'en')
    assert.equal(requests[0]?.temperature, 0)
    assert.equal(requests[0]?.file instanceof File, true)
    assert.equal((requests[0]?.file as File | undefined)?.name, 'audio.webm')
    assert.equal((requests[0]?.file as File | undefined)?.type, 'audio/webm;codecs=opus')
  })

  test('reports unsupported OpenAI transcription audio formats', async () => {
    await assert.rejects(
      async () =>
        await createOpenAiSpeechModel({
          apiKey: 'test-api-key',
          client: {
            audio: {
              transcriptions: {
                async create() {
                  return { text: '' }
                },
              },
            },
          },
        }).transcribeSpeech({
          model: 'gpt-4o-transcribe',
          audio: {
            data: new Uint8Array([1, 2, 3]),
            mimeType: 'audio/pcm',
          },
        }),
      { message: 'Unsupported OpenAI speech transcription audio format "audio/pcm".' },
    )
  })

  test('reports OpenAI API failures', async () => {
    await assert.rejects(
      async () =>
        await createOpenAiSpeechModel({
          apiKey: 'test-api-key',
          client: {
            audio: {
              transcriptions: {
                async create() {
                  throw APIError.generate(401, { error: { message: 'Invalid API key.' } }, 'Unauthorized', new Headers())
                },
              },
            },
          },
        }).transcribeSpeech({
          model: 'gpt-4o-transcribe',
          audio: {
            data: new Uint8Array([1, 2, 3]),
            mimeType: 'audio/webm',
          },
        }),
      { message: 'OpenAI speech transcription request failed (401): Invalid API key.' },
    )
  })
})
