import type { AiTextModel, AiVoiceModel } from '#llm/core/getAiModelFamily.ts'
import { prompt } from '#llm/core/prompt.ts'
import { startVoiceSession } from '#llm/core/startVoiceSession.ts'
import type { VoiceEvent, VoiceSession } from '#llm/core/startVoiceSession.ts'

loadPackageEnvironment()

try {
  await smokeTest()
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exitCode = 1
}

async function smokeTest(): Promise<void> {
  const modelFilters = process.argv.slice(2)
  let testedModels = 0

  for (const model of (
    [
      {
        family: 'Google',
        model: 'gemini-3.5-flash',
        token: 'GOOGLE_MODEL_QUERY_OK',
      },
      {
        family: 'OpenAI',
        model: 'gpt-5.5',
        token: 'OPENAI_MODEL_QUERY_OK',
      },
    ] satisfies SmokeTestTextModel[]
  ).filter(model => modelFilters.length === 0 || modelFilters.includes(model.model))) {
    testedModels += 1
    await smokeTestTextModel(model)
  }

  for (const model of (
    [
      {
        family: 'Google',
        model: 'gemini-3.1-flash-live-preview',
        prompt: 'Say the word ready.',
        timeoutMs: 30_000,
      },
      {
        family: 'OpenAI',
        model: 'gpt-realtime-2',
        prompt: 'Say the word ready.',
        timeoutMs: 30_000,
      },
    ] satisfies SmokeTestVoiceModel[]
  ).filter(model => modelFilters.length === 0 || modelFilters.includes(model.model))) {
    testedModels += 1
    await smokeTestVoiceModel(model)
  }

  if (testedModels === 0) {
    throw new Error(`No smoke test models matched: ${modelFilters.join(', ')}`)
  }
}

async function smokeTestTextModel(args: SmokeTestTextModel): Promise<void> {
  const response = await prompt({
    model: args.model,
    prompt: `Reply with exactly this token and no other text: ${args.token}`,
    maxOutputTokens: 32,
    temperature: 0,
  })

  if (response.text.trim() !== args.token) {
    throw new Error(`${args.family} model returned an unexpected response: ${response.text || '<empty>'}`)
  }

  console.log(`${args.family} model query succeeded with ${args.model}: ${response.text}`)
}

async function smokeTestVoiceModel(args: SmokeTestVoiceModel): Promise<void> {
  const response = createVoiceResponseTracker()
  let session: VoiceSession | undefined

  try {
    session = await withTimeout(
      startVoiceSession({
        model: args.model,
        systemPrompt: 'Give a very short spoken response.',
        onEvent: response.onEvent,
      }),
      args.timeoutMs,
      `${args.family} voice model did not start within ${args.timeoutMs}ms.`,
    )
    await session.send({
      type: 'text',
      text: args.prompt,
    })
    console.log(
      `${args.family} voice model query succeeded with ${args.model}: ${await withTimeout(
        response.audioBytes,
        args.timeoutMs,
        `${args.family} voice model did not return audio within ${args.timeoutMs}ms.`,
      )} audio bytes`,
    )
  } finally {
    await session?.close()
  }
}

function createVoiceResponseTracker(): VoiceResponseTracker {
  let audioBytes = 0
  let resolveAudioBytes: (audioBytes: number) => void = () => {}
  let rejectAudioBytes: (error: Error) => void = () => {}

  return {
    audioBytes: new Promise<number>((resolve, reject) => {
      resolveAudioBytes = resolve
      rejectAudioBytes = reject
    }),
    onEvent(args) {
      if (args.type === 'audio') {
        audioBytes += args.audio.data.byteLength

        if (audioBytes > 0) {
          resolveAudioBytes(audioBytes)
        }
      }

      if (args.type === 'error') {
        rejectAudioBytes(args.error)
      }
    },
  }
}

function loadPackageEnvironment(): void {
  for (const path of [`${import.meta.dirname}/../.env`, `${import.meta.dirname}/../.env.local`]) {
    try {
      process.loadEnvFile(path)
    } catch (error) {
      if (!isMissingEnvironmentFileError(error)) {
        throw error
      }
    }
  }
}

function isMissingEnvironmentFileError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && 'code' in error && error.code === 'ENOENT'
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined

  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timeout = setTimeout(() => reject(new Error(message)), timeoutMs)
      }),
    ])
  } finally {
    clearTimeout(timeout)
  }
}

type SmokeTestTextModel = {
  family: string
  model: AiTextModel
  token: string
}

type SmokeTestVoiceModel = {
  family: string
  model: AiVoiceModel
  prompt: string
  timeoutMs: number
}

type VoiceResponseTracker = {
  audioBytes: Promise<number>
  onEvent(args: VoiceEvent): void
}
