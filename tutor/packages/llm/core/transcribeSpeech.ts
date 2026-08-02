export { transcribeSpeech, type SpeechAudioInput, type SpeechRequest, type SpeechResponse }

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type { AiSpeechModel } from '#llm/core/getAiModelFamily.ts'
import { getAiModelFamily } from '#llm/core/getAiModelFamily.ts'
import type { LlmSpeechAudioInput, LlmSpeechModel, LlmSpeechRequest, LlmSpeechResponse } from '#llm/core/types.ts'
import { createOpenAiSpeechModel } from '#llm/openai/createOpenAiSpeechModel.ts'
import { getOpenAiApiKey } from '#llm/openai/getOpenAiApiKey.ts'

let openAiSpeechModel: LlmSpeechModel | undefined

async function transcribeSpeech(args: SpeechRequest): Promise<SpeechResponse> {
  return await getSpeechModel(args.model).transcribeSpeech(args)
}

function getSpeechModel(model: AiSpeechModel): LlmSpeechModel {
  switch (getAiModelFamily(model)) {
    case 'openai':
      return getOpenAiSpeechModel()
  }
}

function getOpenAiSpeechModel(): LlmSpeechModel {
  return (openAiSpeechModel ??= createOpenAiSpeechModel({ apiKey: getOpenAiApiKey('transcribing speech with OpenAI models') }))
}

type SpeechRequest = LlmSpeechRequest

type SpeechResponse = LlmSpeechResponse

type SpeechAudioInput = LlmSpeechAudioInput

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('requires an OpenAI API key for OpenAI speech transcriptions', async () => {
    const openAiApiKey = process.env['OPENAI_API_KEY']
    const existingOpenAiSpeechModel = openAiSpeechModel

    delete process.env['OPENAI_API_KEY']
    openAiSpeechModel = undefined

    try {
      await assert.rejects(
        async () =>
          await transcribeSpeech({
            model: 'gpt-4o-transcribe',
            audio: {
              data: new Uint8Array([1, 2, 3]),
              mimeType: 'audio/webm',
            },
          }),
        { message: 'Set OPENAI_API_KEY in packages/llm/.env before transcribing speech with OpenAI models.' },
      )
    } finally {
      if (openAiApiKey === undefined) {
        delete process.env['OPENAI_API_KEY']
      } else {
        process.env['OPENAI_API_KEY'] = openAiApiKey
      }
      openAiSpeechModel = existingOpenAiSpeechModel
    }
  })
})
