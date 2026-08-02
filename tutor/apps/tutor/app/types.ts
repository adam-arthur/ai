export { type InputTranscription, type MicrophoneStatus, type SessionStatus, type TranscriptMessage, type TurnMistakes }

import type { KoreanTutorTurnVoiceSessionClientEvent } from '@ai/language-tutor/korean/types.ts'

type SessionStatus = 'idle' | 'starting' | 'active' | 'stopping'

type MicrophoneStatus = 'idle' | 'listening' | 'paused'

type InputTranscription = Extract<KoreanTutorTurnVoiceSessionClientEvent, { type: 'input-transcription' }>['transcription']

type TurnMistakes = Extract<KoreanTutorTurnVoiceSessionClientEvent, { type: 'turn-mistakes' }>

type TranscriptMessage = {
  id: string
  mistakes?: TurnMistakes['mistakes']
  speaker: 'agent' | 'learner'
  text: string
  time: string
}
