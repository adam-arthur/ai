export { createKoreanTutorVoiceSessionClient }

import { createVoiceSessionClient } from '@ai/voice-server/createVoiceSessionClient.ts'

import type {
  KoreanTutorVoiceSessionClient,
  KoreanTutorVoiceSessionStartOptions,
  KoreanTutorVoiceSessionTurnMistakesEvent,
} from '#language-tutor/korean/types.ts'

function createKoreanTutorVoiceSessionClient(): KoreanTutorVoiceSessionClient {
  return createVoiceSessionClient<KoreanTutorVoiceSessionStartOptions, KoreanTutorVoiceSessionTurnMistakesEvent>()
}
