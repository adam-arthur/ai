export { createGoogleLlm, type GoogleLlm, type GoogleLlmOptions }

import { createGoogleTextModel } from '#llm/google/createGoogleTextModel.ts'
import { createGoogleVoiceModel } from '#llm/google/createGoogleVoiceModel.ts'
import type { GoogleLlm, GoogleLlmOptions } from '#llm/google/types.ts'

function createGoogleLlm(args: GoogleLlmOptions): GoogleLlm {
  return {
    text: createGoogleTextModel(args),
    voice: createGoogleVoiceModel(args),
  }
}
