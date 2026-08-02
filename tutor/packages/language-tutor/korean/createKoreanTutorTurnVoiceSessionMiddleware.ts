export { createKoreanTutorTurnVoiceSessionMiddleware }

import { createTurnVoiceSessionMiddleware } from '@ai/voice-server/createTurnVoiceSessionMiddleware.ts'
import type { TurnVoiceSessionMiddleware } from '@ai/voice-server/types.ts'

import { analyzeKoreanTutorVoiceTurn } from '#language-tutor/korean/analyzeKoreanTutorVoiceTurn.ts'
import type { KoreanTutorVoiceTurnMistake } from '#language-tutor/korean/analyzeKoreanTutorVoiceTurn.ts'
import { createKoreanTutorVoiceTurnResponsePrompt } from '#language-tutor/korean/createKoreanTutorVoiceTurnResponsePrompt.ts'
import { getKoreanSystemPrompt } from '#language-tutor/korean/getKoreanSystemPrompt.ts'
import { isKoreanTutorLevel } from '#language-tutor/korean/isKoreanTutorLevel.ts'
import { isKoreanTutorMistakeDetectionModel } from '#language-tutor/korean/isKoreanTutorMistakeDetectionModel.ts'
import { isKoreanTutorModel } from '#language-tutor/korean/isKoreanTutorModel.ts'
import { isKoreanTutorTurnModelConfiguration } from '#language-tutor/korean/isKoreanTutorTurnModelConfiguration.ts'
import type {
  KoreanTutorMistakeDetectionModel,
  KoreanTutorModel,
  KoreanTutorTurnModelConfiguration,
  KoreanTutorTurnTextModel,
  KoreanTutorTurnVoiceSessionStartOptions,
  KoreanTutorTurnVoiceSessionTurnMistakesEvent,
} from '#language-tutor/korean/types.ts'
import { defaultKoreanTutorTurnModelConfiguration } from '#language-tutor/korean/types.ts'

function createKoreanTutorTurnVoiceSessionMiddleware(): TurnVoiceSessionMiddleware {
  return createTurnVoiceSessionMiddleware<KoreanTutorTurnVoiceSessionStartOptions, KoreanTutorTurnVoiceSessionTurnMistakesEvent>({
    createResponsePrompt(args) {
      return createKoreanTutorVoiceTurnResponsePrompt({
        conversation: args.conversation,
        ...(args.responseInstructions ? { responseInstructions: args.responseInstructions } : {}),
        transcription: args.transcription,
      })
    },
    createSessionRequest(args) {
      if (!isKoreanTutorTurnVoiceSessionStartRequest(args.body)) {
        return undefined
      }

      const modelConfiguration = toKoreanTutorTurnModelConfiguration({
        ...(args.body.mistakeDetectionModel ? { mistakeDetectionModel: args.body.mistakeDetectionModel } : {}),
        ...(args.body.model ? { model: args.body.model } : {}),
        ...(args.body.modelConfiguration ? { modelConfiguration: args.body.modelConfiguration } : {}),
      })

      return {
        options: {
          level: args.body.level,
          modelConfiguration,
        },
        request: {
          response: {
            maxOutputTokens: 350,
            model: modelConfiguration.reply,
            systemPrompt: getKoreanSystemPrompt(args.body.level),
            thinkingLevel: 'low',
          },
          synthesis: {
            instructions: 'Speak clearly and warmly for a Korean language learner.',
            model: modelConfiguration.speechSynthesis,
            outputAudio: {
              mimeType: 'audio/pcm',
              sampleRateHertz: 24000,
            },
            voiceName: 'marin',
          },
          transcription: {
            model: modelConfiguration.transcription,
            prompt: 'The audio may contain Korean learner speech, Hangul, romanized Korean, and occasional English.',
          },
        },
      }
    },
    invalidStartRequestMessage: 'Level must be A1 or A2, and model configuration must use supported tutor turn models.',
    async prepareTurn(args) {
      try {
        const mistakes = await analyzeKoreanTutorVoiceTurn({
          level: args.options.level,
          model: args.options.modelConfiguration?.mistakeDetection ?? defaultKoreanTutorTurnModelConfiguration.mistakeDetection,
          ...(args.previousModelText ? { previousTutorText: args.previousModelText } : {}),
          transcription: {
            text: args.transcription,
          },
        })

        return mistakes.length
          ? {
              events: {
                type: 'turn-mistakes',
                ...(args.inputId ? { inputId: args.inputId } : {}),
                mistakes,
              },
              responseInstructions: toKoreanTutorTurnResponseInstructions(mistakes),
            }
          : undefined
      } catch (error) {
        return {
          events: {
            type: 'error',
            message: `Korean turn mistake detection failed: ${toErrorMessage(error)}`,
          },
        }
      }
    },
  })
}

function isKoreanTutorTurnVoiceSessionStartRequest(body: unknown): body is KoreanTutorTurnVoiceSessionStartOptions {
  return (
    typeof body === 'object' &&
    body !== null &&
    'level' in body &&
    isKoreanTutorLevel(body.level) &&
    (!('model' in body) || isKoreanTutorModel(body.model)) &&
    (!('mistakeDetectionModel' in body) || isKoreanTutorMistakeDetectionModel(body.mistakeDetectionModel)) &&
    (!('modelConfiguration' in body) || isKoreanTutorTurnModelConfiguration(body.modelConfiguration))
  )
}

function toKoreanTutorTurnModelConfiguration(args: KoreanTutorTurnModelConfigurationInput): KoreanTutorTurnModelConfiguration {
  return args.modelConfiguration
    ? { ...args.modelConfiguration }
    : {
        ...defaultKoreanTutorTurnModelConfiguration,
        ...(args.mistakeDetectionModel
          ? { mistakeDetection: koreanTutorLegacyTurnMistakeDetectionModels[args.mistakeDetectionModel] }
          : {}),
        ...(args.model ? { reply: koreanTutorLegacyTurnReplyModels[args.model] } : {}),
      }
}

const koreanTutorLegacyTurnMistakeDetectionModels = {
  'gemini-flash': 'gemini-3.5-flash',
  'gemini-flash-lite': 'gemini-3.1-flash-lite',
} satisfies Record<KoreanTutorMistakeDetectionModel, KoreanTutorTurnTextModel>

const koreanTutorLegacyTurnReplyModels = {
  gemini: 'gemini-3.1-flash-lite',
  gpt: 'gpt-5.5',
} satisfies Record<KoreanTutorModel, KoreanTutorTurnTextModel>

function toKoreanTutorTurnResponseInstructions(mistakes: KoreanTutorVoiceTurnMistake[]): string {
  return `The learner made these notable Korean mistakes. Briefly correct them if helpful before continuing the conversation:
${mistakes.map(mistake => `- ${mistake.original} -> ${mistake.correction}: ${mistake.explanation}`).join('\n')}`
}

function toErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  return String(error)
}

type KoreanTutorTurnModelConfigurationInput = {
  mistakeDetectionModel?: KoreanTutorMistakeDetectionModel
  model?: KoreanTutorModel
  modelConfiguration?: KoreanTutorTurnModelConfiguration
}
