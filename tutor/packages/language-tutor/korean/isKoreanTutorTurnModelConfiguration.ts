export { isKoreanTutorTurnModelConfiguration }

import {
  koreanTutorTurnSpeechSynthesisModels,
  koreanTutorTurnTextModels,
  koreanTutorTurnTranscriptionModels,
} from '#language-tutor/korean/types.ts'
import type {
  KoreanTutorTurnModelConfiguration,
  KoreanTutorTurnSpeechSynthesisModel,
  KoreanTutorTurnTextModel,
  KoreanTutorTurnTranscriptionModel,
} from '#language-tutor/korean/types.ts'

function isKoreanTutorTurnModelConfiguration(value: unknown): value is KoreanTutorTurnModelConfiguration {
  return (
    typeof value === 'object' &&
    value !== null &&
    'mistakeDetection' in value &&
    'reply' in value &&
    'speechSynthesis' in value &&
    'transcription' in value &&
    koreanTutorTurnTextModels.includes(value.mistakeDetection as KoreanTutorTurnTextModel) &&
    koreanTutorTurnTextModels.includes(value.reply as KoreanTutorTurnTextModel) &&
    koreanTutorTurnSpeechSynthesisModels.includes(value.speechSynthesis as KoreanTutorTurnSpeechSynthesisModel) &&
    koreanTutorTurnTranscriptionModels.includes(value.transcription as KoreanTutorTurnTranscriptionModel)
  )
}
