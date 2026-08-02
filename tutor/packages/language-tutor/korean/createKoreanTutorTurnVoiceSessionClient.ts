export { createKoreanTutorTurnVoiceSessionClient }

import { createTurnVoiceSessionClient } from '@ai/voice-server/createTurnVoiceSessionClient.ts'

import type {
  KoreanTutorTurnVoiceSessionClient,
  KoreanTutorTurnVoiceSessionStartOptions,
  KoreanTutorTurnVoiceSessionTurnMistakesEvent,
} from '#language-tutor/korean/types.ts'

function createKoreanTutorTurnVoiceSessionClient(): KoreanTutorTurnVoiceSessionClient {
  return createTurnVoiceSessionClient<KoreanTutorTurnVoiceSessionStartOptions, KoreanTutorTurnVoiceSessionTurnMistakesEvent>()
}
