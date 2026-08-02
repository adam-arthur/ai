export { isKoreanTutorLevel }

import type { KoreanTutorLevel } from '#language-tutor/korean/types.ts'

function isKoreanTutorLevel(value: unknown): value is KoreanTutorLevel {
  return value === 'A1' || value === 'A2'
}
