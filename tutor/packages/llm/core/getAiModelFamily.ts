export {
  getAiModelFamily,
  type AiModel,
  type AiModelFamily,
  type AiSpeechModel,
  type AiSpeechSynthesisModel,
  type AiTextModel,
  type AiVoiceModel,
}

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

function getAiModelFamily<TModel extends AiModel>(model: TModel): AiModelFamily<TModel> {
  if (model in aiModelFamilies) {
    return aiModelFamilies[model]
  }

  throw new Error(`Unsupported AI model "${model}". Supported models: ${Object.keys(aiModelFamilies).join(', ')}.`)
}

const aiModelFamilies = {
  'gemini-3.1-flash-lite': 'gemini',
  'gemini-3.5-flash': 'gemini',
  'gemini-3.1-flash-live-preview': 'gemini',
  'gpt-5.5': 'openai',
  'gpt-4o-mini-transcribe': 'openai',
  'gpt-4o-transcribe': 'openai',
  'gpt-4o-mini-tts': 'openai',
  'gpt-realtime-2': 'openai',
} as const

const aiTextModels = {
  'gemini-3.1-flash-lite': true,
  'gemini-3.5-flash': true,
  'gpt-5.5': true,
} as const

const aiVoiceModels = {
  'gemini-3.1-flash-live-preview': true,
  'gpt-realtime-2': true,
} as const

const aiSpeechModels = {
  'gpt-4o-mini-transcribe': true,
  'gpt-4o-transcribe': true,
} as const

const aiSpeechSynthesisModels = {
  'gpt-4o-mini-tts': true,
} as const

type AiModel = AiTextModel | AiVoiceModel | AiSpeechModel | AiSpeechSynthesisModel

type AiTextModel = keyof typeof aiTextModels

type AiVoiceModel = keyof typeof aiVoiceModels

type AiSpeechModel = keyof typeof aiSpeechModels

type AiSpeechSynthesisModel = keyof typeof aiSpeechSynthesisModels

type AiModelFamily<TModel extends AiModel = AiModel> = (typeof aiModelFamilies)[TModel]

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  test('gets the model family for supported AI models', () => {
    assert.equal(getAiModelFamily('gemini-3.1-flash-lite'), 'gemini')
    assert.equal(getAiModelFamily('gemini-3.5-flash'), 'gemini')
    assert.equal(getAiModelFamily('gemini-3.1-flash-live-preview'), 'gemini')
    assert.equal(getAiModelFamily('gpt-5.5'), 'openai')
    assert.equal(getAiModelFamily('gpt-4o-mini-transcribe'), 'openai')
    assert.equal(getAiModelFamily('gpt-4o-transcribe'), 'openai')
    assert.equal(getAiModelFamily('gpt-4o-mini-tts'), 'openai')
    assert.equal(getAiModelFamily('gpt-realtime-2'), 'openai')
  })

  test('rejects unsupported AI models', () => {
    assert.throws(
      () => {
        getAiModelFamily('unsupported-model' as AiModel)
      },
      {
        message:
          'Unsupported AI model "unsupported-model". Supported models: gemini-3.1-flash-lite, gemini-3.5-flash, gemini-3.1-flash-live-preview, gpt-5.5, gpt-4o-mini-transcribe, gpt-4o-transcribe, gpt-4o-mini-tts, gpt-realtime-2.',
      },
    )
  })
})
