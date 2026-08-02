export { prompt, type PromptFormat, type PromptRequest, type PromptResponse }

import { getAiModelFamily } from '#llm/core/getAiModelFamily.ts'
import type { AiTextModel } from '#llm/core/getAiModelFamily.ts'
import type { LlmTextFormat, LlmTextModel, LlmTextRequest, LlmTextResponse } from '#llm/core/types.ts'
import { createGoogleTextModel } from '#llm/google/createGoogleTextModel.ts'
import { getGoogleApiKey } from '#llm/google/getGoogleApiKey.ts'
import type { GoogleTextModel } from '#llm/google/types.ts'
import { createOpenAiTextModel } from '#llm/openai/createOpenAiTextModel.ts'
import { getOpenAiApiKey } from '#llm/openai/getOpenAiApiKey.ts'

let googleTextModel: GoogleTextModel | undefined
let openAiTextModel: LlmTextModel | undefined

async function prompt<TFormat extends PromptFormat | undefined = undefined>(
  args: PromptRequest<TFormat>,
): Promise<PromptResponse<TFormat>> {
  return getTextModel(args.model).generateText(args)
}

function getTextModel(model: AiTextModel): LlmTextModel {
  switch (getAiModelFamily(model)) {
    case 'gemini':
      return getGoogleTextModel()
    case 'openai':
      return getOpenAiTextModel()
  }
}

function getGoogleTextModel(): GoogleTextModel {
  return (googleTextModel ??= createGoogleTextModel({ apiKey: getGoogleApiKey({ action: 'prompting Google text models' }) }))
}

function getOpenAiTextModel(): LlmTextModel {
  return (openAiTextModel ??= createOpenAiTextModel({ apiKey: getOpenAiApiKey('prompting OpenAI text models') }))
}

type PromptFormat = LlmTextFormat

type PromptRequest<TFormat extends PromptFormat | undefined = undefined> = LlmTextRequest<TFormat>

type PromptResponse<TFormat extends PromptFormat | undefined = undefined> = LlmTextResponse<TFormat>
