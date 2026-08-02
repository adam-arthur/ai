export { createKoreanTutorVoiceSessionMiddleware }

import type { AiVoiceModel } from '@ai/llm'
import { createVoiceSessionMiddleware } from '@ai/voice-server/createVoiceSessionMiddleware.ts'
import type { VoiceSessionMiddleware } from '@ai/voice-server/types.ts'

import { analyzeKoreanTutorVoiceTurn } from '#language-tutor/korean/analyzeKoreanTutorVoiceTurn.ts'
import { getKoreanSystemPrompt } from '#language-tutor/korean/getKoreanSystemPrompt.ts'
import { isKoreanTutorLevel } from '#language-tutor/korean/isKoreanTutorLevel.ts'
import { isKoreanTutorMistakeDetectionModel } from '#language-tutor/korean/isKoreanTutorMistakeDetectionModel.ts'
import { isKoreanTutorModel } from '#language-tutor/korean/isKoreanTutorModel.ts'
import type {
  KoreanTutorModel,
  KoreanTutorVoiceSessionStartOptions,
  KoreanTutorVoiceSessionTurnMistakesEvent,
} from '#language-tutor/korean/types.ts'
import { defaultKoreanTutorMistakeDetectionModel } from '#language-tutor/korean/types.ts'

function createKoreanTutorVoiceSessionMiddleware(): VoiceSessionMiddleware {
  return createVoiceSessionMiddleware<KoreanTutorVoiceSessionStartOptions, KoreanTutorVoiceSessionTurnMistakesEvent>({
    createSessionRequest(args) {
      if (!isKoreanTutorVoiceSessionStartRequest(args.body)) {
        return undefined
      }

      return {
        options: {
          level: args.body.level,
          mistakeDetectionModel: args.body.mistakeDetectionModel || defaultKoreanTutorMistakeDetectionModel,
          model: args.body.model,
        },
        request: {
          model: toKoreanTutorVoiceSessionModel(args.body.model),
          thinkingLevel: 'low',
          inputAudio: {
            mimeType: 'audio/pcm;rate=24000',
          },
          inputTranscription: {
            model: 'gpt-4o-mini-transcribe',
          },
          outputAudio: {
            mimeType: 'audio/pcm;rate=24000',
          },
          outputTranscription: {},
          systemPrompt: getKoreanSystemPrompt(args.body.level),
          ...(args.body.model === 'gpt' ? { voiceName: 'marin' } : {}),
        },
      }
    },
    invalidStartRequestMessage:
      'Level must be A1 or A2, model must be Gemini or GPT, and mistake detection model must be Gemini Flash-Lite or Gemini Flash.',
    async onAudioTurnEnd(args) {
      if (!args.inputTranscription) {
        return undefined
      }

      try {
        const mistakes = await analyzeKoreanTutorVoiceTurn({
          level: args.options.level,
          model: args.options.mistakeDetectionModel || defaultKoreanTutorMistakeDetectionModel,
          ...(args.previousModelText ? { previousTutorText: args.previousModelText } : {}),
          transcription: {
            text: args.inputTranscription,
          },
        })

        return mistakes.length
          ? {
              type: 'turn-mistakes',
              ...(args.inputId ? { inputId: args.inputId } : {}),
              mistakes,
            }
          : undefined
      } catch (error) {
        return {
          type: 'error',
          message: `Korean turn mistake detection failed: ${toErrorMessage(error)}`,
        }
      }
    },
  })
}

function toKoreanTutorVoiceSessionModel(model: KoreanTutorModel): AiVoiceModel {
  return koreanTutorVoiceSessionModels[model]
}

const koreanTutorVoiceSessionModels = {
  gemini: 'gemini-3.1-flash-live-preview',
  gpt: 'gpt-realtime-2',
} satisfies Record<KoreanTutorModel, AiVoiceModel>

function isKoreanTutorVoiceSessionStartRequest(body: unknown): body is KoreanTutorVoiceSessionStartOptions {
  return (
    typeof body === 'object' &&
    body !== null &&
    'level' in body &&
    'model' in body &&
    isKoreanTutorLevel(body.level) &&
    isKoreanTutorModel(body.model) &&
    (!('mistakeDetectionModel' in body) || isKoreanTutorMistakeDetectionModel(body.mistakeDetectionModel))
  )
}

function toErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  return String(error)
}
