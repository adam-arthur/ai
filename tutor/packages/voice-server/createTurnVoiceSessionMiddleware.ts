export { createTurnVoiceSessionMiddleware }

import { Buffer } from 'node:buffer'
import { randomUUID } from 'node:crypto'
import type { IncomingMessage, ServerResponse } from 'node:http'

import type {
  TurnVoiceSessionAudioInput,
  TurnVoiceSessionAudioOutput,
  TurnVoiceSessionClientEvent,
  TurnVoiceSessionCustomEvent,
  TurnVoiceSessionMessage,
  TurnVoiceSessionMiddleware,
  TurnVoiceSessionMiddlewareNext,
  TurnVoiceSessionMiddlewareOptions,
  TurnVoiceSessionStartOptions,
  TurnVoiceSessionStartResult,
} from '#voice-server/types.ts'

const defaultPath = '/api/turn-voice-sessions'

function createTurnVoiceSessionMiddleware<
  TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent = never,
>(args: TurnVoiceSessionMiddlewareOptions<TStartOptions, TCustomEvent>): TurnVoiceSessionMiddleware {
  const middlewareOptions = args
  const sessions = new Map<string, ServerTurnVoiceSession<TStartOptions, TCustomEvent>>()

  return turnVoiceSessionMiddleware

  async function turnVoiceSessionMiddleware(
    request: IncomingMessage,
    response: ServerResponse,
    next: TurnVoiceSessionMiddlewareNext,
  ): Promise<void> {
    const requestUrl = new URL(request.url ?? '/', 'http://localhost')
    const pathSegments = requestUrl.pathname.split('/').filter(Boolean)
    const middlewarePathSegments = getPath().split('/').filter(Boolean)

    if (!matchesPath(pathSegments, middlewarePathSegments)) {
      next()
      return
    }

    try {
      const routeSegments = pathSegments.slice(middlewarePathSegments.length)

      if (request.method === 'POST' && routeSegments.length === 0) {
        await handleStartTurnVoiceSession(request, response, sessions)
        return
      }

      if (request.method === 'GET' && routeSegments.length === 2 && routeSegments[1] === 'events') {
        handleTurnVoiceSessionEvents(response, request, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'POST' && routeSegments.length === 2 && routeSegments[1] === 'audio-turns') {
        await handleSendTurnVoiceSessionAudioTurn(request, response, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'DELETE' && routeSegments.length === 1) {
        await handleDeleteTurnVoiceSession(response, sessions, routeSegments[0]!)
        return
      }

      writeJson(response, 404, {
        message: 'Turn voice session route not found.',
      })
    } catch (error) {
      writeJson(response, 500, {
        message: toErrorMessage(error),
      })
    }
  }

  async function handleStartTurnVoiceSession(
    request: IncomingMessage,
    response: ServerResponse,
    sessions: Map<string, ServerTurnVoiceSession<TStartOptions, TCustomEvent>>,
  ): Promise<void> {
    const startRequest = await middlewareOptions.createSessionRequest({
      body: await readJsonBody<unknown>(request),
    })

    if (!startRequest) {
      writeJson(response, 400, {
        message: middlewareOptions.invalidStartRequestMessage ?? 'Turn voice session start request is invalid.',
      })
      return
    }

    const id = randomUUID()

    sessions.set(id, {
      conversation: [],
      eventResponses: new Set(),
      options: startRequest.options,
      pendingEvents: [],
      processingQueue: Promise.resolve(),
      request: startRequest.request,
    })
    writeJson(response, 200, {
      id,
    })
  }

  async function handleSendTurnVoiceSessionAudioTurn(
    request: IncomingMessage,
    response: ServerResponse,
    sessions: Map<string, ServerTurnVoiceSession<TStartOptions, TCustomEvent>>,
    sessionId: string,
  ): Promise<void> {
    const body = await readJsonBody<SendTurnVoiceSessionAudioTurnRequest>(request)
    const session = sessions.get(sessionId)

    if (!session) {
      writeJson(response, 404, {
        message: 'Turn voice session not found.',
      })
      return
    }

    if (!isSendTurnVoiceSessionAudioTurnRequest(body)) {
      writeJson(response, 400, {
        message: 'Microphone audio is required.',
      })
      return
    }

    try {
      await enqueueTurnVoiceSessionProcessing(
        session,
        async () =>
          await processAudioTurn({
            audio: {
              data: Buffer.from(body.audio.data, 'base64'),
              mimeType: body.audio.mimeType,
            },
            ...(typeof body.inputId === 'string' && body.inputId !== '' ? { inputId: body.inputId } : {}),
            session,
          }),
      )
      writeJson(response, 200, {
        ok: true,
      })
    } catch (error) {
      emitBrowserTurnVoiceSessionEvent(session, {
        type: 'error',
        message: toErrorMessage(error),
      })
      writeJson(response, 500, {
        message: toErrorMessage(error),
      })
    }
  }

  async function processAudioTurn(args: ProcessTurnAudioArgs<TStartOptions, TCustomEvent>): Promise<void> {
    const { prompt, synthesizeSpeech, transcribeSpeech } = await import('@ai/llm')
    const previousModelText = getLatestModelConversationText(args.session)
    const transcription = (
      await transcribeSpeech({
        ...args.session.request.transcription,
        audio: toTranscriptionAudio(args.audio),
      })
    ).text.trim()

    recordLearnerConversationText({
      ...(args.inputId ? { inputId: args.inputId } : {}),
      session: args.session,
      text: transcription,
    })
    emitBrowserTurnVoiceSessionEvent(args.session, {
      type: 'input-transcription',
      transcription: {
        ...(args.inputId ? { inputId: args.inputId } : {}),
        text: transcription,
      },
    })

    const preparation = await middlewareOptions.prepareTurn?.({
      conversation: args.session.conversation,
      ...(args.inputId ? { inputId: args.inputId } : {}),
      options: args.session.options,
      ...(previousModelText ? { previousModelText } : {}),
      transcription,
    })

    for (const event of toEventArray(preparation?.events)) {
      emitBrowserTurnVoiceSessionEvent(args.session, event)
    }

    const responseText = (
      await prompt({
        ...args.session.request.response,
        prompt: await middlewareOptions.createResponsePrompt({
          conversation: args.session.conversation,
          ...(args.inputId ? { inputId: args.inputId } : {}),
          options: args.session.options,
          ...(previousModelText ? { previousModelText } : {}),
          ...(preparation?.responseInstructions ? { responseInstructions: preparation.responseInstructions } : {}),
          transcription,
        }),
      })
    ).text.trim()

    if (!responseText) {
      throw new Error('Turn voice session response text was empty.')
    }

    recordModelConversationText({
      session: args.session,
      text: responseText,
    })
    emitBrowserTurnVoiceSessionEvent(args.session, {
      type: 'text',
      text: responseText,
    })
    emitBrowserTurnVoiceSessionEvent(args.session, {
      type: 'audio',
      audio: toBrowserAudioOutput(
        (
          await synthesizeSpeech({
            ...args.session.request.synthesis,
            text: responseText,
          })
        ).audio,
      ),
    })
    emitBrowserTurnVoiceSessionEvent(args.session, {
      type: 'turn-complete',
    })
  }

  function getPath(): string {
    return middlewareOptions.path ?? defaultPath
  }
}

function handleTurnVoiceSessionEvents<TStartOptions extends TurnVoiceSessionStartOptions, TCustomEvent extends TurnVoiceSessionCustomEvent>(
  response: ServerResponse,
  request: IncomingMessage,
  sessions: Map<string, ServerTurnVoiceSession<TStartOptions, TCustomEvent>>,
  sessionId: string,
): void {
  const session = sessions.get(sessionId)

  if (!session) {
    writeJson(response, 404, {
      message: 'Turn voice session not found.',
    })
    return
  }

  response.writeHead(200, {
    'cache-control': 'no-cache',
    connection: 'keep-alive',
    'content-type': 'text/event-stream',
  })
  response.write('\n')
  session.eventResponses.add(response)

  for (const event of session.pendingEvents.splice(0)) {
    writeTurnVoiceSessionEvent(response, event)
  }

  request.on('close', () => {
    session.eventResponses.delete(response)
  })
}

async function handleDeleteTurnVoiceSession<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
>(response: ServerResponse, sessions: Map<string, ServerTurnVoiceSession<TStartOptions, TCustomEvent>>, sessionId: string): Promise<void> {
  const session = sessions.get(sessionId)

  if (session) {
    sessions.delete(sessionId)

    for (const eventResponse of session.eventResponses) {
      eventResponse.end()
    }

    await session.processingQueue.catch(() => {})
  }

  writeJson(response, 200, {
    ok: true,
  })
}

function enqueueTurnVoiceSessionProcessing<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
>(session: ServerTurnVoiceSession<TStartOptions, TCustomEvent>, processTurn: () => Promise<void>): Promise<void> {
  const queuedProcessing = session.processingQueue.catch(() => {}).then(processTurn)

  session.processingQueue = queuedProcessing.then(
    () => undefined,
    () => undefined,
  )

  return queuedProcessing
}

function recordLearnerConversationText<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
>(args: RecordTurnVoiceSessionLearnerTextArgs<TStartOptions, TCustomEvent>): void {
  const text = args.text.trim()

  if (!text) {
    return
  }

  if (args.inputId) {
    const messageIndex = args.session.conversation.findIndex(message => message.role === 'learner' && message.inputId === args.inputId)

    if (messageIndex >= 0) {
      args.session.conversation[messageIndex] = {
        role: 'learner',
        inputId: args.inputId,
        text,
      }
      return
    }

    args.session.conversation.push({
      role: 'learner',
      inputId: args.inputId,
      text,
    })
    return
  }

  args.session.conversation.push({
    role: 'learner',
    text,
  })
}

function recordModelConversationText<TStartOptions extends TurnVoiceSessionStartOptions, TCustomEvent extends TurnVoiceSessionCustomEvent>(
  args: RecordTurnVoiceSessionModelTextArgs<TStartOptions, TCustomEvent>,
): void {
  const text = args.text.trim()

  if (!text) {
    return
  }

  args.session.conversation.push({
    role: 'model',
    text,
  })
}

function getLatestModelConversationText<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
>(session: ServerTurnVoiceSession<TStartOptions, TCustomEvent>): string | undefined {
  for (let index = session.conversation.length - 1; index >= 0; index--) {
    const message = session.conversation[index]!

    if (message.role === 'model') {
      return message.text.trim() || undefined
    }
  }

  return undefined
}

function emitBrowserTurnVoiceSessionEvent<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
>(session: ServerTurnVoiceSession<TStartOptions, TCustomEvent>, browserEvent: TurnVoiceSessionClientEvent<TCustomEvent>): void {
  if (session.eventResponses.size === 0) {
    session.pendingEvents = [...session.pendingEvents, browserEvent].slice(-100)
    return
  }

  for (const response of session.eventResponses) {
    writeTurnVoiceSessionEvent(response, browserEvent)
  }
}

function writeTurnVoiceSessionEvent<TCustomEvent extends TurnVoiceSessionCustomEvent>(
  response: ServerResponse,
  event: TurnVoiceSessionClientEvent<TCustomEvent>,
): void {
  response.write(`data: ${JSON.stringify(event)}\n\n`)
}

async function readJsonBody<TBody>(request: IncomingMessage): Promise<TBody> {
  const chunks: Buffer[] = []

  for await (const chunk of request) {
    chunks.push(typeof chunk === 'string' ? Buffer.from(chunk) : chunk)
  }

  return JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}') as TBody
}

function writeJson(response: ServerResponse, statusCode: number, body: JsonObject): void {
  response.writeHead(statusCode, {
    'content-type': 'application/json',
  })
  response.end(JSON.stringify(body))
}

function toTranscriptionAudio(audio: TurnVoiceSessionAudioInput): TurnVoiceSessionAudioInput {
  if (audio.mimeType.split(';')[0] !== 'audio/pcm') {
    return audio
  }

  return toWavAudio(audio)
}

function toWavAudio(audio: TurnVoiceSessionAudioInput): TurnVoiceSessionAudioInput {
  if (audio.data.length % 2) {
    throw new Error('PCM audio data must contain 16-bit samples.')
  }

  const sampleRateHertz = toPcmSampleRateHertz(audio.mimeType)
  const output = new Uint8Array(44 + audio.data.length)
  const view = new DataView(output.buffer)

  writeAscii(output, 0, 'RIFF')
  view.setUint32(4, 36 + audio.data.length, true)
  writeAscii(output, 8, 'WAVE')
  writeAscii(output, 12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, 1, true)
  view.setUint32(24, sampleRateHertz, true)
  view.setUint32(28, sampleRateHertz * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  writeAscii(output, 36, 'data')
  view.setUint32(40, audio.data.length, true)
  output.set(audio.data, 44)

  return {
    data: output,
    mimeType: 'audio/wav',
  }
}

function writeAscii(bytes: Uint8Array, offset: number, text: string): void {
  for (let index = 0; index < text.length; index += 1) {
    bytes[offset + index] = text.charCodeAt(index)
  }
}

function toPcmSampleRateHertz(mimeType: string): number {
  const sampleRateHertz = Number(mimeType.match(/(?:^|;)rate=(\d+)(?:;|$)/)?.[1])

  if (!Number.isInteger(sampleRateHertz) || sampleRateHertz <= 0) {
    throw new Error(`PCM audio mime type must include a valid sample rate: "${mimeType}".`)
  }

  return sampleRateHertz
}

function toBrowserAudioOutput(audio: TurnVoiceSessionAudioInput): TurnVoiceSessionAudioOutput {
  return {
    data: Buffer.from(audio.data).toString('base64'),
    mimeType: audio.mimeType,
  }
}

function toErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  return String(error)
}

function isSendTurnVoiceSessionAudioTurnRequest(
  body: SendTurnVoiceSessionAudioTurnRequest,
): body is ValidSendTurnVoiceSessionAudioTurnRequest {
  if (!body.audio || typeof body.audio !== 'object') {
    return false
  }

  return (
    'data' in body.audio &&
    'mimeType' in body.audio &&
    typeof body.audio.data === 'string' &&
    body.audio.data !== '' &&
    typeof body.audio.mimeType === 'string' &&
    body.audio.mimeType !== ''
  )
}

function matchesPath(pathSegments: string[], middlewarePathSegments: string[]): boolean {
  return middlewarePathSegments.every((segment, index) => pathSegments[index] === segment)
}

function toEventArray<TCustomEvent extends TurnVoiceSessionCustomEvent>(
  events: TurnVoiceSessionClientEvent<TCustomEvent> | TurnVoiceSessionClientEvent<TCustomEvent>[] | undefined,
): TurnVoiceSessionClientEvent<TCustomEvent>[] {
  if (!events) {
    return []
  }

  return Array.isArray(events) ? events : [events]
}

type ServerTurnVoiceSession<TStartOptions extends TurnVoiceSessionStartOptions, TCustomEvent extends TurnVoiceSessionCustomEvent> = {
  conversation: TurnVoiceSessionMessage[]
  eventResponses: Set<ServerResponse>
  options: TStartOptions
  pendingEvents: TurnVoiceSessionClientEvent<TCustomEvent>[]
  processingQueue: Promise<void>
  request: TurnVoiceSessionStartResult
}

type ProcessTurnAudioArgs<TStartOptions extends TurnVoiceSessionStartOptions, TCustomEvent extends TurnVoiceSessionCustomEvent> = {
  audio: TurnVoiceSessionAudioInput
  inputId?: string
  session: ServerTurnVoiceSession<TStartOptions, TCustomEvent>
}

type RecordTurnVoiceSessionLearnerTextArgs<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
> = {
  inputId?: string
  session: ServerTurnVoiceSession<TStartOptions, TCustomEvent>
  text: string
}

type RecordTurnVoiceSessionModelTextArgs<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
> = {
  session: ServerTurnVoiceSession<TStartOptions, TCustomEvent>
  text: string
}

type SendTurnVoiceSessionAudioTurnRequest = {
  audio?: {
    data?: unknown
    mimeType?: unknown
  }
  inputId?: unknown
}

type ValidSendTurnVoiceSessionAudioTurnRequest = {
  audio: {
    data: string
    mimeType: string
  }
  inputId?: string
}

type JsonObject = Record<string, unknown>
