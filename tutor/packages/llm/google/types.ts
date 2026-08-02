export type {
  GoogleLlm,
  GoogleLlmOptions,
  GoogleTextModel,
  GoogleTextModelOptions,
  GoogleTextRequest,
  GoogleTextResponse,
  GoogleVoiceAudioChunk,
  GoogleVoiceModel,
  GoogleVoiceModelEvent,
  GoogleVoiceModelEventHandler,
  GoogleVoiceModelOptions,
  GoogleVoiceModelSession,
  GoogleVoiceSessionConfig,
  GoogleVoiceSessionOptions,
  GoogleVoiceTurnInput,
}

import type {
  LlmAudioChunk,
  LlmProvider,
  LlmTextFormat,
  LlmTextModel,
  LlmTextRequest,
  LlmTextResponse,
  LlmTurnInput,
  LlmVoiceEvent,
  LlmVoiceEventHandler,
  LlmVoiceModel,
  LlmVoiceSession,
  LlmVoiceSessionConfig,
  LlmVoiceSessionOptions,
} from '#llm/core/types.ts'

type GoogleLlm = LlmProvider & {
  text: LlmTextModel
  voice: LlmVoiceModel
}

type GoogleLlmOptions = {
  apiKey: string
}

type GoogleVoiceModelOptions = GoogleLlmOptions

type GoogleTextModelOptions = GoogleLlmOptions

type GoogleTextModel = LlmTextModel

type GoogleTextRequest<TFormat extends LlmTextFormat | undefined = undefined> = LlmTextRequest<TFormat>

type GoogleTextResponse<TFormat extends LlmTextFormat | undefined = undefined> = LlmTextResponse<TFormat>

type GoogleVoiceModel = LlmVoiceModel

type GoogleVoiceModelSession = LlmVoiceSession

type GoogleVoiceModelEventHandler = LlmVoiceEventHandler

type GoogleVoiceModelEvent = LlmVoiceEvent

type GoogleVoiceSessionOptions = LlmVoiceSessionOptions

type GoogleVoiceSessionConfig = LlmVoiceSessionConfig

type GoogleVoiceTurnInput = LlmTurnInput

type GoogleVoiceAudioChunk = LlmAudioChunk
