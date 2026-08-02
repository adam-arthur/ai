export type {
  AiModel,
  AiSpeechModel,
  AiSpeechSynthesisModel,
  AiTextModel,
  AiVoiceModel,
  LlmAudioChunk,
  LlmAudioFormat,
  LlmProvider,
  LlmSpeechAudioInput,
  LlmSpeechModel,
  LlmSpeechRequest,
  LlmSpeechResponse,
  LlmSpeechSynthesisModel,
  LlmSpeechSynthesisRequest,
  LlmSpeechSynthesisResponse,
  LlmTextModel,
  LlmTextFormat,
  LlmTextRequest,
  LlmTextResponse,
  LlmThinkingLevel,
  LlmVoiceAudioInput,
  LlmVoiceAudioTurnEnd,
  LlmTurnInput,
  LlmVoiceEvent,
  LlmVoiceEventHandler,
  LlmVoiceInputTranscription,
  LlmVoiceModel,
  LlmVoiceSession,
  LlmVoiceSessionConfig,
  LlmVoiceSessionOptions,
  LlmVoiceTurnGuidance,
  LlmVoiceTranscriptionConfig,
}

import type * as z from 'zod/v4'

import type { AiModel, AiSpeechModel, AiSpeechSynthesisModel, AiTextModel, AiVoiceModel } from '#llm/core/getAiModelFamily.ts'

type LlmProvider = {
  speech?: LlmSpeechModel
  speechSynthesis?: LlmSpeechSynthesisModel
  text?: LlmTextModel
  voice?: LlmVoiceModel
}

type LlmTextModel = {
  generateText<TFormat extends LlmTextFormat | undefined = undefined>(args: LlmTextRequest<TFormat>): Promise<LlmTextResponse<TFormat>>
}

type LlmTextRequest<TFormat extends LlmTextFormat | undefined = undefined> = {
  model: AiTextModel
  systemPrompt?: string
  prompt: string
  format?: TFormat
  thinkingLevel?: LlmThinkingLevel
  temperature?: number
  maxOutputTokens?: number
}

type LlmTextResponse<TFormat extends LlmTextFormat | undefined = undefined> = {
  text: string
} & ([TFormat] extends [LlmTextFormat] ? { object: z.output<TFormat> } : {})

type LlmTextFormat = z.ZodObject

type LlmThinkingLevel = 'low' | 'medium' | 'high'

type LlmSpeechModel = {
  transcribeSpeech(args: LlmSpeechRequest): Promise<LlmSpeechResponse>
}

type LlmSpeechRequest = {
  model: AiSpeechModel
  audio: LlmSpeechAudioInput
  prompt?: string
  languageCode?: string
  temperature?: number
}

type LlmSpeechResponse = {
  text: string
}

type LlmSpeechAudioInput = LlmAudioChunk

type LlmSpeechSynthesisModel = {
  synthesizeSpeech(args: LlmSpeechSynthesisRequest): Promise<LlmSpeechSynthesisResponse>
}

type LlmSpeechSynthesisRequest = {
  model: AiSpeechSynthesisModel
  text: string
  voiceName: string
  instructions?: string
  outputAudio?: LlmAudioFormat
  speechSpeed?: number
}

type LlmSpeechSynthesisResponse = {
  audio: LlmAudioChunk
}

type LlmVoiceModel = {
  startVoiceSession(args: LlmVoiceSessionOptions): Promise<LlmVoiceSession>
}

type LlmVoiceSession = {
  send(args: LlmTurnInput): Promise<void>
  appendAudio(args: LlmVoiceAudioInput): Promise<void>
  endAudioTurn(args?: LlmVoiceAudioTurnEnd): Promise<void>
  close(): Promise<void>
}

type LlmVoiceEventHandler = (args: LlmVoiceEvent) => void | Promise<void>

type LlmVoiceEvent =
  | {
      type: 'audio'
      audio: LlmAudioChunk
    }
  | {
      type: 'text'
      text: string
    }
  | {
      type: 'input-transcription'
      transcription: LlmVoiceInputTranscription
    }
  | {
      type: 'turn-complete'
    }
  | {
      type: 'error'
      error: Error
    }

type LlmVoiceSessionOptions = {
  config: LlmVoiceSessionConfig
  onEvent: LlmVoiceEventHandler
}

type LlmVoiceSessionConfig = {
  model: AiVoiceModel
  systemPrompt?: string
  thinkingLevel?: LlmThinkingLevel
  speechSpeed?: number
  inputLanguageCode?: string
  inputTranscription?: LlmVoiceTranscriptionConfig
  outputTranscription?: LlmVoiceTranscriptionConfig
  outputLanguageCode?: string
  voiceName?: string
  inputAudio?: LlmAudioFormat
  outputAudio?: LlmAudioFormat
}

type LlmTurnInput =
  | {
      id?: string
      guidance?: LlmVoiceTurnGuidance
      type: 'audio'
      audio: LlmAudioChunk
    }
  | {
      id?: string
      guidance?: LlmVoiceTurnGuidance
      type: 'text'
      text: string
    }

type LlmVoiceAudioInput = {
  id?: string
  guidance?: LlmVoiceTurnGuidance
  audio: LlmAudioChunk
}

type LlmVoiceAudioTurnEnd = {
  id?: string
  guidance?: LlmVoiceTurnGuidance
}

type LlmAudioChunk = {
  data: Uint8Array
  mimeType: string
}

type LlmAudioFormat = {
  mimeType: string
  sampleRateHertz?: number
}

type LlmVoiceInputTranscription = {
  inputId?: string
  text: string
}

type LlmVoiceTurnGuidance = {
  instructions: string
}

type LlmVoiceTranscriptionConfig = {
  model?: string
  prompt?: string
}
