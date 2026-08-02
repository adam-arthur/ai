export {
  prompt,
  startVoiceSession,
  synthesizeSpeech,
  transcribeSpeech,
  type AiModel,
  type AiSpeechModel,
  type AiSpeechSynthesisModel,
  type AiTextModel,
  type AiVoiceModel,
  type PromptFormat,
  type PromptRequest,
  type PromptResponse,
  type SpeechAudioInput,
  type SpeechRequest,
  type SpeechResponse,
  type SpeechSynthesisAudioOutput,
  type SpeechSynthesisRequest,
  type SpeechSynthesisResponse,
  type ThinkingLevel,
  type VoiceAudioChunk,
  type VoiceAudioFormat,
  type VoiceEvent,
  type VoiceInputTranscription,
  type VoiceSession,
  type VoiceSessionRequest,
  type VoiceTranscriptionConfig,
  type VoiceTurnInput,
  type VoiceTurnGuidance,
}
process.loadEnvFile(`${import.meta.dirname}/.env`)

import { prompt } from '#llm/core/prompt.ts'
import type { PromptFormat, PromptRequest, PromptResponse } from '#llm/core/prompt.ts'
import { startVoiceSession } from '#llm/core/startVoiceSession.ts'
import type {
  VoiceAudioChunk,
  VoiceAudioFormat,
  VoiceEvent,
  VoiceInputTranscription,
  VoiceSession,
  VoiceSessionRequest,
  VoiceTranscriptionConfig,
  VoiceTurnInput,
  VoiceTurnGuidance,
} from '#llm/core/startVoiceSession.ts'
import { synthesizeSpeech } from '#llm/core/synthesizeSpeech.ts'
import type { SpeechSynthesisAudioOutput, SpeechSynthesisRequest, SpeechSynthesisResponse } from '#llm/core/synthesizeSpeech.ts'
import { transcribeSpeech } from '#llm/core/transcribeSpeech.ts'
import type { SpeechAudioInput, SpeechRequest, SpeechResponse } from '#llm/core/transcribeSpeech.ts'
import type {
  AiModel,
  AiSpeechModel,
  AiSpeechSynthesisModel,
  AiTextModel,
  AiVoiceModel,
  LlmThinkingLevel as ThinkingLevel,
} from '#llm/core/types.ts'
