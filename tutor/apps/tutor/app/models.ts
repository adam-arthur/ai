import type { ModelConfiguration, SpeechSynthesisModel, TextModel, TranscriptionModel } from '#tutor/app/generated/api.ts'

export const textModels = ['gemini-3.1-flash-lite', 'gemini-3.5-flash', 'gpt-5.5'] as const satisfies readonly TextModel[]
export const transcriptionModels = ['gpt-4o-mini-transcribe', 'gpt-4o-transcribe'] as const satisfies readonly TranscriptionModel[]
export const speechSynthesisModels = ['tts-1'] as const satisfies readonly SpeechSynthesisModel[]

export const defaultModelConfiguration: ModelConfiguration = {
  mistakeDetection: 'gemini-3.1-flash-lite',
  reply: 'gemini-3.1-flash-lite',
  speechSynthesis: 'tts-1',
  transcription: 'gpt-4o-mini-transcribe',
}
