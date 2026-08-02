export { createGoogleTextModel, type GoogleTextModelOptions }

import { GoogleGenAI } from '@google/genai'

import { parseLlmTextResponse } from '#llm/core/parseLlmTextResponse.ts'
import { toLlmTextResponseJsonSchema } from '#llm/core/toLlmTextResponseJsonSchema.ts'
import type { LlmTextFormat } from '#llm/core/types.ts'
import { toGoogleThinkingConfig } from '#llm/google/toGoogleThinkingConfig.ts'
import type { GoogleTextModel, GoogleTextModelOptions, GoogleTextRequest, GoogleTextResponse } from '#llm/google/types.ts'

function createGoogleTextModel(args: GoogleTextModelOptions): GoogleTextModel {
  const googleClient = new GoogleGenAI({ apiKey: args.apiKey })

  return {
    async generateText<TFormat extends LlmTextFormat | undefined = undefined>(
      args: GoogleTextRequest<TFormat>,
    ): Promise<GoogleTextResponse<TFormat>> {
      return parseLlmTextResponse({
        text:
          (
            await googleClient.models.generateContent({
              model: args.model,
              contents: args.prompt,
              config: {
                thinkingConfig: toGoogleThinkingConfig(args.thinkingLevel),
                ...(args.systemPrompt ? { systemInstruction: args.systemPrompt } : {}),
                ...(args.maxOutputTokens === undefined ? {} : { maxOutputTokens: args.maxOutputTokens }),
                ...(args.format
                  ? {
                      responseJsonSchema: toLlmTextResponseJsonSchema({ format: args.format }),
                      responseMimeType: 'application/json',
                    }
                  : {}),
                ...(args.temperature === undefined ? {} : { temperature: args.temperature }),
              },
            })
          ).text ?? '',
        format: args.format,
      })
    },
  }
}
