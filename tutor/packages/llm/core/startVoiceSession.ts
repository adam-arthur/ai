export {
  startVoiceSession,
  type VoiceAudioChunk,
  type VoiceAudioFormat,
  type VoiceEvent,
  type VoiceAudioInput,
  type VoiceAudioTurnEnd,
  type VoiceInputTranscription,
  type VoiceSession,
  type VoiceSessionRequest,
  type VoiceTranscriptionConfig,
  type VoiceTurnInput,
  type VoiceTurnGuidance,
}

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import { getAiModelFamily } from '#llm/core/getAiModelFamily.ts'
import type { AiVoiceModel } from '#llm/core/getAiModelFamily.ts'
import type {
  LlmAudioChunk,
  LlmAudioFormat,
  LlmTurnInput,
  LlmVoiceEvent,
  LlmVoiceEventHandler,
  LlmVoiceAudioInput,
  LlmVoiceAudioTurnEnd,
  LlmVoiceInputTranscription,
  LlmVoiceModel,
  LlmVoiceSession,
  LlmVoiceSessionConfig,
  LlmVoiceTurnGuidance,
  LlmVoiceTranscriptionConfig,
} from '#llm/core/types.ts'
import { createGoogleVoiceModel } from '#llm/google/createGoogleVoiceModel.ts'
import { getGoogleApiKey } from '#llm/google/getGoogleApiKey.ts'
import type { GoogleVoiceModel } from '#llm/google/types.ts'
import { createOpenAiVoiceModel } from '#llm/openai/createOpenAiVoiceModel.ts'
import { getOpenAiApiKey } from '#llm/openai/getOpenAiApiKey.ts'

let googleVoiceModel: GoogleVoiceModel | undefined
let openAiVoiceModel: LlmVoiceModel | undefined

async function startVoiceSession(args: VoiceSessionRequest): Promise<VoiceSession> {
  const { onEvent, ...config } = args

  return await getVoiceModel(config.model).startVoiceSession({
    config,
    onEvent,
  })
}

function getVoiceModel(model: AiVoiceModel): LlmVoiceModel {
  switch (getAiModelFamily(model)) {
    case 'gemini':
      return getGoogleVoiceModel()
    case 'openai':
      return getOpenAiVoiceModel()
  }
}

function getGoogleVoiceModel(): GoogleVoiceModel {
  return (googleVoiceModel ??= createGoogleVoiceModel({ apiKey: getGoogleApiKey({ action: 'starting Google voice sessions' }) }))
}

function getOpenAiVoiceModel(): LlmVoiceModel {
  return (openAiVoiceModel ??= createOpenAiVoiceModel({ apiKey: getOpenAiApiKey('starting OpenAI voice sessions') }))
}

type VoiceSessionRequest = LlmVoiceSessionConfig & {
  onEvent: LlmVoiceEventHandler
}

type VoiceSession = LlmVoiceSession

type VoiceEvent = LlmVoiceEvent

type VoiceTurnInput = LlmTurnInput

type VoiceTurnGuidance = LlmVoiceTurnGuidance

type VoiceAudioInput = LlmVoiceAudioInput

type VoiceAudioTurnEnd = LlmVoiceAudioTurnEnd

type VoiceAudioChunk = LlmAudioChunk

type VoiceAudioFormat = LlmAudioFormat

type VoiceInputTranscription = LlmVoiceInputTranscription

type VoiceTranscriptionConfig = LlmVoiceTranscriptionConfig

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('requires a Google API key for Gemini voice sessions', async () => {
    const geminiApiKey = process.env['GEMINI_API_KEY']
    const existingGoogleVoiceModel = googleVoiceModel

    delete process.env['GEMINI_API_KEY']
    googleVoiceModel = undefined

    try {
      await assert.rejects(
        async () =>
          await startVoiceSession({
            model: 'gemini-3.1-flash-live-preview',
            onEvent() {},
          }),
        { message: 'Set GEMINI_API_KEY in packages/llm/.env before starting Google voice sessions.' },
      )
    } finally {
      if (geminiApiKey === undefined) {
        delete process.env['GEMINI_API_KEY']
      } else {
        process.env['GEMINI_API_KEY'] = geminiApiKey
      }
      googleVoiceModel = existingGoogleVoiceModel
    }
  })

  test('requires an OpenAI API key for OpenAI voice sessions', async () => {
    const openAiApiKey = process.env['OPENAI_API_KEY']
    const existingOpenAiVoiceModel = openAiVoiceModel

    delete process.env['OPENAI_API_KEY']
    openAiVoiceModel = undefined

    try {
      await assert.rejects(
        async () =>
          await startVoiceSession({
            model: 'gpt-realtime-2',
            onEvent() {},
          }),
        { message: 'Set OPENAI_API_KEY in packages/llm/.env before starting OpenAI voice sessions.' },
      )
    } finally {
      if (openAiApiKey === undefined) {
        delete process.env['OPENAI_API_KEY']
      } else {
        process.env['OPENAI_API_KEY'] = openAiApiKey
      }
      openAiVoiceModel = existingOpenAiVoiceModel
    }
  })
})
