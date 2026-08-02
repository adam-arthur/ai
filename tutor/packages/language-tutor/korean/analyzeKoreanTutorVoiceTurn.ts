export { analyzeKoreanTutorVoiceTurn, type KoreanTutorVoiceTurnMistake, type KoreanTutorVoiceTurnMistakeKind }

import * as z from 'zod/v4'

import { prompt } from '@ai/llm'
import type { AiTextModel } from '@ai/llm'

import type { KoreanTutorLevel, KoreanTutorMistakeDetectionModel, KoreanTutorTurnTextModel } from '#language-tutor/korean/types.ts'

async function analyzeKoreanTutorVoiceTurn(args: KoreanTutorVoiceTurnMistakesInput): Promise<KoreanTutorVoiceTurnMistake[]> {
  const learnerText = args.transcription.text.trim()
  const previousTutorText = args.previousTutorText?.trim()

  if (!learnerText) {
    return []
  }

  return (
    await prompt({
      format: koreanTutorVoiceTurnMistakesFormat,
      maxOutputTokens: 300,
      model: toKoreanTutorMistakeDetectionTextModel(args.model),
      prompt: `Learner level: ${args.level}
${previousTutorText ? `\nPrevious tutor message:\n${previousTutorText}\n` : ''}

Learner transcript:
${learnerText}`,
      systemPrompt: koreanTutorVoiceTurnMistakesSystemPrompt,
    })
  ).object.mistakes
}

const koreanTutorVoiceTurnMistakesFormat = z.object({
  mistakes: z
    .array(
      z.object({
        kind: z.enum(['grammar', 'vocabulary', 'politeness', 'naturalness']),
        original: z.string().min(1),
        correction: z.string().min(1),
        explanation: z.string().min(1),
      }),
    )
    .max(2),
})

const koreanTutorVoiceTurnMistakesSystemPrompt = `You identify Korean language mistakes in a learner's spoken turn.

Identify clear grammar, vocabulary, politeness, or naturalness errors that a tutor should address to help the learner improve.
Focus on noticeable mistakes or distinctly unnatural phrasing. Do not flag minor spoken quirks (like commonly dropped particles) or slight conversational imperfections as long as the speech sounds reasonably natural.
Use the previous tutor message, when provided, only to understand what the learner is responding to.
Prioritize one or two issues most helpful for the learner's level.
If the transcript is highly communicable without obvious errors, or too unclear to correct confidently, return no mistakes.
Use the original field for the exact learner phrase when possible, the correction field for a natural Korean replacement, and the explanation field for a concise English explanation.
Do not write the tutor's full reply.
Return at most two mistakes.`

function toKoreanTutorMistakeDetectionTextModel(model: KoreanTutorVoiceTurnMistakeDetectionModel): AiTextModel {
  return koreanTutorMistakeDetectionTextModels[model]
}

const koreanTutorMistakeDetectionTextModels = {
  'gemini-3.1-flash-lite': 'gemini-3.1-flash-lite',
  'gemini-3.5-flash': 'gemini-3.5-flash',
  'gemini-flash': 'gemini-3.5-flash',
  'gemini-flash-lite': 'gemini-3.1-flash-lite',
  'gpt-5.5': 'gpt-5.5',
} satisfies Record<KoreanTutorVoiceTurnMistakeDetectionModel, AiTextModel>

type KoreanTutorVoiceTurnMistakesInput = {
  level: KoreanTutorLevel
  model: KoreanTutorVoiceTurnMistakeDetectionModel
  previousTutorText?: string
  transcription: KoreanTutorVoiceTurnTranscription
}

type KoreanTutorVoiceTurnTranscription = {
  text: string
}

type KoreanTutorVoiceTurnMistake = z.output<typeof koreanTutorVoiceTurnMistakesFormat>['mistakes'][number]

type KoreanTutorVoiceTurnMistakeKind = KoreanTutorVoiceTurnMistake['kind']

type KoreanTutorVoiceTurnMistakeDetectionModel = KoreanTutorMistakeDetectionModel | KoreanTutorTurnTextModel
