export { synthesizeSpeech, type SpeechSynthesisAudioOutput, type SpeechSynthesisRequest, type SpeechSynthesisResponse }

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { AiSpeechSynthesisModel } from '#llm/core/getAiModelFamily.ts'
import { getAiModelFamily } from '#llm/core/getAiModelFamily.ts'
import type { LlmAudioChunk, LlmSpeechSynthesisModel, LlmSpeechSynthesisRequest, LlmSpeechSynthesisResponse } from '#llm/core/types.ts'
import { createOpenAiSpeechSynthesisModel } from '#llm/openai/createOpenAiSpeechSynthesisModel.ts'
import { getOpenAiApiKey } from '#llm/openai/getOpenAiApiKey.ts'

let openAiSpeechSynthesisModel: LlmSpeechSynthesisModel | undefined

async function synthesizeSpeech(args: SpeechSynthesisRequest): Promise<SpeechSynthesisResponse> {
  return await getSpeechSynthesisModel(args.model).synthesizeSpeech(args)
}

function getSpeechSynthesisModel(model: AiSpeechSynthesisModel): LlmSpeechSynthesisModel {
  switch (getAiModelFamily(model)) {
    case 'openai':
      return getOpenAiSpeechSynthesisModel()
  }
}

function getOpenAiSpeechSynthesisModel(): LlmSpeechSynthesisModel {
  return (openAiSpeechSynthesisModel ??= createOpenAiSpeechSynthesisModel({
    apiKey: getOpenAiApiKey('synthesizing speech with OpenAI models'),
  }))
}

type SpeechSynthesisRequest = LlmSpeechSynthesisRequest

type SpeechSynthesisResponse = LlmSpeechSynthesisResponse

type SpeechSynthesisAudioOutput = LlmAudioChunk

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('requires an OpenAI API key for OpenAI speech synthesis', async () => {
    const openAiApiKey = process.env['OPENAI_API_KEY']
    const existingOpenAiSpeechSynthesisModel = openAiSpeechSynthesisModel

    delete process.env['OPENAI_API_KEY']
    openAiSpeechSynthesisModel = undefined

    try {
      await assert.rejects(
        async () =>
          await synthesizeSpeech({
            model: 'gpt-4o-mini-tts',
            text: 'Hello.',
            voiceName: 'marin',
          }),
        { message: 'Set OPENAI_API_KEY in packages/llm/.env before synthesizing speech with OpenAI models.' },
      )
    } finally {
      if (openAiApiKey === undefined) {
        delete process.env['OPENAI_API_KEY']
      } else {
        process.env['OPENAI_API_KEY'] = openAiApiKey
      }
      openAiSpeechSynthesisModel = existingOpenAiSpeechSynthesisModel
    }
  })
})
