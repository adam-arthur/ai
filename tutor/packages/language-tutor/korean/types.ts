export {
  defaultKoreanTutorMistakeDetectionModel,
  defaultKoreanTutorTurnModelConfiguration,
  koreanTutorTurnSpeechSynthesisModels,
  koreanTutorTurnTextModels,
  koreanTutorTurnTranscriptionModels,
  type KoreanTutorLevel,
  type KoreanTutorMistakeDetectionModel,
  type KoreanTutorModel,
  type KoreanTutorTurnModelConfiguration,
  type KoreanTutorTurnSpeechSynthesisModel,
  type KoreanTutorTurnTextModel,
  type KoreanTutorTurnTranscriptionModel,
  type KoreanTutorTurnVoiceSessionAudioInput,
  type KoreanTutorTurnVoiceSessionClient,
  type KoreanTutorTurnVoiceSessionClientEvent,
  type KoreanTutorTurnVoiceSessionStartOptions,
  type KoreanTutorTurnVoiceSessionTurnMistakesEvent,
  type KoreanTutorVoiceSessionAudioInput,
  type KoreanTutorVoiceSessionClient,
  type KoreanTutorVoiceSessionClientEvent,
  type KoreanTutorVoiceSessionStartOptions,
  type KoreanTutorVoiceSessionTurnMistakesEvent,
}

import type { AiSpeechModel, AiSpeechSynthesisModel, AiTextModel } from '@ai/llm'
import type {
  TurnVoiceSessionAudioInput,
  TurnVoiceSessionClient,
  TurnVoiceSessionClientEvent,
  VoiceSessionAudioInput,
  VoiceSessionClient,
  VoiceSessionClientEvent,
} from '@ai/voice-server/types.ts'

import type { KoreanTutorVoiceTurnMistake } from '#language-tutor/korean/analyzeKoreanTutorVoiceTurn.ts'

const defaultKoreanTutorMistakeDetectionModel = 'gemini-flash-lite'
const koreanTutorTurnTextModels = ['gemini-3.1-flash-lite', 'gemini-3.5-flash', 'gpt-5.5'] as const satisfies readonly AiTextModel[]
const koreanTutorTurnTranscriptionModels = ['gpt-4o-mini-transcribe', 'gpt-4o-transcribe'] as const satisfies readonly AiSpeechModel[]
const koreanTutorTurnSpeechSynthesisModels = ['gpt-4o-mini-tts'] as const satisfies readonly AiSpeechSynthesisModel[]
const defaultKoreanTutorTurnModelConfiguration = {
  mistakeDetection: 'gemini-3.1-flash-lite',
  reply: 'gemini-3.1-flash-lite',
  speechSynthesis: 'gpt-4o-mini-tts',
  transcription: 'gpt-4o-mini-transcribe',
} satisfies KoreanTutorTurnModelConfiguration

type KoreanTutorLevel = 'A1' | 'A2'
type KoreanTutorModel = 'gemini' | 'gpt'
type KoreanTutorMistakeDetectionModel = typeof defaultKoreanTutorMistakeDetectionModel | 'gemini-flash'

type KoreanTutorVoiceSessionStartOptions = {
  level: KoreanTutorLevel
  mistakeDetectionModel?: KoreanTutorMistakeDetectionModel
  model: KoreanTutorModel
}

type KoreanTutorTurnTextModel = (typeof koreanTutorTurnTextModels)[number]

type KoreanTutorTurnTranscriptionModel = (typeof koreanTutorTurnTranscriptionModels)[number]

type KoreanTutorTurnSpeechSynthesisModel = (typeof koreanTutorTurnSpeechSynthesisModels)[number]

type KoreanTutorTurnModelConfiguration = {
  mistakeDetection: KoreanTutorTurnTextModel
  reply: KoreanTutorTurnTextModel
  speechSynthesis: KoreanTutorTurnSpeechSynthesisModel
  transcription: KoreanTutorTurnTranscriptionModel
}

type KoreanTutorTurnVoiceSessionStartOptions = {
  level: KoreanTutorLevel
  mistakeDetectionModel?: KoreanTutorMistakeDetectionModel
  model?: KoreanTutorModel
  modelConfiguration?: KoreanTutorTurnModelConfiguration
}

type KoreanTutorVoiceSessionTurnMistakesEvent = {
  type: 'turn-mistakes'
  inputId?: string
  mistakes: KoreanTutorVoiceTurnMistake[]
}

type KoreanTutorTurnVoiceSessionTurnMistakesEvent = KoreanTutorVoiceSessionTurnMistakesEvent

type KoreanTutorVoiceSessionClientEvent = VoiceSessionClientEvent<KoreanTutorVoiceSessionTurnMistakesEvent>

type KoreanTutorTurnVoiceSessionClientEvent = TurnVoiceSessionClientEvent<KoreanTutorTurnVoiceSessionTurnMistakesEvent>

type KoreanTutorVoiceSessionClient = VoiceSessionClient<KoreanTutorVoiceSessionStartOptions, KoreanTutorVoiceSessionTurnMistakesEvent>

type KoreanTutorTurnVoiceSessionClient = TurnVoiceSessionClient<
  KoreanTutorTurnVoiceSessionStartOptions,
  KoreanTutorTurnVoiceSessionTurnMistakesEvent
>

type KoreanTutorVoiceSessionAudioInput = VoiceSessionAudioInput

type KoreanTutorTurnVoiceSessionAudioInput = TurnVoiceSessionAudioInput
