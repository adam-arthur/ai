export { isKoreanTutorMistakeDetectionModel }

import type { KoreanTutorMistakeDetectionModel } from '#language-tutor/korean/types.ts'

function isKoreanTutorMistakeDetectionModel(value: unknown): value is KoreanTutorMistakeDetectionModel {
  return value === 'gemini-flash-lite' || value === 'gemini-flash'
}
