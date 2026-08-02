export { createKoreanTutorVoiceTurnResponsePrompt }

import type { TurnVoiceSessionMessage } from '@ai/voice-server/types.ts'

function createKoreanTutorVoiceTurnResponsePrompt(args: KoreanTutorVoiceTurnResponsePromptInput): string {
  const previousConversation = args.conversation.slice(0, -1).slice(-8)

  return `${args.responseInstructions ? `Internal tutor note:\n${args.responseInstructions}\n\n` : ''}${
    previousConversation.length
      ? `Previous conversation:\n${previousConversation
          .map(message => `${message.role === 'learner' ? 'Learner' : 'Tutor'}: ${message.text}`)
          .join('\n')}\n\n`
      : ''
  }Learner transcript:
${args.transcription || '[No clear transcription]'}

Write the tutor's next spoken response.`
}

type KoreanTutorVoiceTurnResponsePromptInput = {
  conversation: readonly TurnVoiceSessionMessage[]
  responseInstructions?: string
  transcription: string
}
