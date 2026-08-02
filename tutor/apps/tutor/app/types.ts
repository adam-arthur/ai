export { type InputTranscription, type MicrophoneStatus, type SessionStatus, type TranscriptMessage, type TurnMistakes }

import type { ClientEvent } from '#tutor/app/generated/api.ts'

type SessionStatus = 'idle' | 'starting' | 'active' | 'stopping'

type MicrophoneStatus = 'idle' | 'listening' | 'paused'

type InputTranscription = Extract<ClientEvent, { type: 'input-transcription' }>['transcription']

type TurnMistakes = Extract<ClientEvent, { type: 'turn-mistakes' }>

type TranscriptMessage = {
  id: string
  mistakes?: TurnMistakes['mistakes']
  speaker: 'agent' | 'learner'
  text: string
  time: string
}
