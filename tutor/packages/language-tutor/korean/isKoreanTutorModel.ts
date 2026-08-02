export { isKoreanTutorModel }

import type { KoreanTutorModel } from '#language-tutor/korean/types.ts'

function isKoreanTutorModel(value: unknown): value is KoreanTutorModel {
  return value === 'gemini' || value === 'gpt'
}
