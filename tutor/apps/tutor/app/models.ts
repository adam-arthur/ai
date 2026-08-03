import type { ModelConfiguration, SpeechSynthesisModel, TextModel, TranscriptionModel } from '#tutor/app/generated/api.ts'

export const textModels = ['gemini-3.5-flash-lite', 'gemini-3.6-flash', 'gpt-5.6-terra'] as const satisfies readonly TextModel[]
export const transcriptionModels = ['gpt-transcribe'] as const satisfies readonly TranscriptionModel[]
export const speechSynthesisModels = ['gpt-4o-mini-tts'] as const satisfies readonly SpeechSynthesisModel[]

export const defaultModelConfiguration: ModelConfiguration = {
  mistakeDetection: 'gemini-3.5-flash-lite',
  reply: 'gemini-3.5-flash-lite',
  speechSynthesis: 'gpt-4o-mini-tts',
  transcription: 'gpt-transcribe',
}
