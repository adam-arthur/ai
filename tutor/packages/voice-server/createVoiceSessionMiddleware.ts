export { createVoiceSessionMiddleware }

import { Buffer } from 'node:buffer'
import { randomUUID } from 'node:crypto'
import type { IncomingMessage, ServerResponse } from 'node:http'

import type { VoiceEvent, VoiceSession } from '@ai/llm'

import type {
  VoiceSessionClientEvent,
  VoiceSessionCustomEvent,
  VoiceSessionMiddleware,
  VoiceSessionMiddlewareNext,
  VoiceSessionMiddlewareOptions,
  VoiceSessionStartOptions,
} from '#voice-server/types.ts'

const defaultInputTranscriptionWaitMs = 400
const defaultPath = '/api/voice-sessions'

function createVoiceSessionMiddleware<
  TStartOptions extends VoiceSessionStartOptions = VoiceSessionStartOptions,
  TCustomEvent extends VoiceSessionCustomEvent = never,
>(args: VoiceSessionMiddlewareOptions<TStartOptions, TCustomEvent>): VoiceSessionMiddleware {
  const sessions = new Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>()

  return voiceSessionMiddleware

  async function voiceSessionMiddleware(
    request: IncomingMessage,
    response: ServerResponse,
    next: VoiceSessionMiddlewareNext,
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
        await handleStartVoiceSession(request, response, sessions)
        return
      }

      if (request.method === 'GET' && routeSegments.length === 2 && routeSegments[1] === 'events') {
        handleVoiceSessionEvents(response, request, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'POST' && routeSegments.length === 2 && routeSegments[1] === 'messages') {
        await handleSendVoiceSessionMessage(request, response, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'POST' && routeSegments.length === 2 && routeSegments[1] === 'audio') {
        await handleSendVoiceSessionAudio(request, response, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'POST' && routeSegments.length === 2 && routeSegments[1] === 'audio-chunks') {
        await handleSendVoiceSessionAudioChunk(request, response, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'POST' && routeSegments.length === 2 && routeSegments[1] === 'audio-turns') {
        await handleEndVoiceSessionAudioTurn(request, response, sessions, routeSegments[0]!)
        return
      }

      if (request.method === 'DELETE' && routeSegments.length === 1) {
        await handleDeleteVoiceSession(response, sessions, routeSegments[0]!)
        return
      }

      writeJson(response, 404, {
        message: 'Voice session route not found.',
      })
    } catch (error) {
      writeJson(response, 500, {
        message: toErrorMessage(error),
      })
    }
  }

  async function handleStartVoiceSession(
    request: IncomingMessage,
    response: ServerResponse,
    sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
  ): Promise<void> {
    const startRequest = await args.createSessionRequest({
      body: await readJsonBody<unknown>(request),
    })

    if (!startRequest) {
      writeJson(response, 400, {
        message: args.invalidStartRequestMessage ?? 'Voice session start request is invalid.',
      })
      return
    }

    const id = randomUUID()
    const serverSession: ServerVoiceSession<TStartOptions, TCustomEvent> = {
      conversation: [],
      eventResponses: new Set(),
      inputTranscriptionWaiters: new Map(),
      inputTranscriptions: new Map(),
      modelMessageOpen: false,
      options: startRequest.options,
      pendingEvents: [],
    }
    const { startVoiceSession } = await import('@ai/llm')

    serverSession.session = await startVoiceSession({
      ...startRequest.request,
      onEvent(event) {
        handleServerVoiceSessionEvent(serverSession, event)
      },
    })
    sessions.set(id, serverSession)
    writeJson(response, 200, {
      id,
    })
  }

  async function handleEndVoiceSessionAudioTurn(
    request: IncomingMessage,
    response: ServerResponse,
    sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
    sessionId: string,
  ): Promise<void> {
    const body = await readJsonBody<EndVoiceSessionAudioTurnRequest>(request)
    const session = sessions.get(sessionId)

    if (!session?.session) {
      writeJson(response, 404, {
        message: 'Voice session not found.',
      })
      return
    }

    const inputId = typeof body.inputId === 'string' && body.inputId !== '' ? body.inputId : undefined
    const inputTranscription = args.onAudioTurnEnd
      ? await getLatestInputTranscription({
          ...(inputId ? { inputId } : {}),
          session,
          waitMs: args.inputTranscriptionWaitMs ?? defaultInputTranscriptionWaitMs,
        })
      : undefined
    const previousModelText = args.onAudioTurnEnd ? getLatestModelConversationText(session) : undefined
    const events = await args.onAudioTurnEnd?.({
      ...(inputId ? { inputId } : {}),
      ...(inputTranscription ? { inputTranscription } : {}),
      options: session.options,
      ...(previousModelText ? { previousModelText } : {}),
    })

    for (const event of toEventArray(events)) {
      emitBrowserVoiceSessionEvent(session, event)
    }

    await session.session.endAudioTurn(inputId ? { id: inputId } : undefined)
    writeJson(response, 200, {
      ok: true,
    })
  }

  function getPath(): string {
    return args.path ?? defaultPath
  }
}

function handleVoiceSessionEvents<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  response: ServerResponse,
  request: IncomingMessage,
  sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
  sessionId: string,
): void {
  const session = sessions.get(sessionId)

  if (!session) {
    writeJson(response, 404, {
      message: 'Voice session not found.',
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
    writeVoiceSessionEvent(response, event)
  }

  request.on('close', () => {
    session.eventResponses.delete(response)
  })
}

async function handleSendVoiceSessionMessage<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  request: IncomingMessage,
  response: ServerResponse,
  sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
  sessionId: string,
): Promise<void> {
  const body = await readJsonBody<SendVoiceSessionMessageRequest>(request)
  const session = sessions.get(sessionId)

  if (!session?.session) {
    writeJson(response, 404, {
      message: 'Voice session not found.',
    })
    return
  }

  if (typeof body.text !== 'string' || body.text.trim() === '') {
    writeJson(response, 400, {
      message: 'Message text is required.',
    })
    return
  }

  recordLearnerConversationText({
    session,
    text: body.text,
  })
  await session.session.send({
    type: 'text',
    text: body.text,
  })
  writeJson(response, 200, {
    ok: true,
  })
}

async function handleSendVoiceSessionAudio<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  request: IncomingMessage,
  response: ServerResponse,
  sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
  sessionId: string,
): Promise<void> {
  const body = await readJsonBody<SendVoiceSessionAudioRequest>(request)
  const session = sessions.get(sessionId)

  if (!session?.session) {
    writeJson(response, 404, {
      message: 'Voice session not found.',
    })
    return
  }

  if (!isSendVoiceSessionAudioRequest(body)) {
    writeJson(response, 400, {
      message: 'Microphone audio is required.',
    })
    return
  }

  await session.session.send({
    ...(typeof body.inputId === 'string' && body.inputId !== '' ? { id: body.inputId } : {}),
    type: 'audio',
    audio: {
      data: Buffer.from(body.audio.data, 'base64'),
      mimeType: body.audio.mimeType,
    },
  })
  writeJson(response, 200, {
    ok: true,
  })
}

async function handleSendVoiceSessionAudioChunk<
  TStartOptions extends VoiceSessionStartOptions,
  TCustomEvent extends VoiceSessionCustomEvent,
>(
  request: IncomingMessage,
  response: ServerResponse,
  sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
  sessionId: string,
): Promise<void> {
  const body = await readJsonBody<SendVoiceSessionAudioRequest>(request)
  const session = sessions.get(sessionId)

  if (!session?.session) {
    writeJson(response, 404, {
      message: 'Voice session not found.',
    })
    return
  }

  if (!isSendVoiceSessionAudioRequest(body)) {
    writeJson(response, 400, {
      message: 'Microphone audio is required.',
    })
    return
  }

  await session.session.appendAudio({
    ...(typeof body.inputId === 'string' && body.inputId !== '' ? { id: body.inputId } : {}),
    audio: {
      data: Buffer.from(body.audio.data, 'base64'),
      mimeType: body.audio.mimeType,
    },
  })
  writeJson(response, 200, {
    ok: true,
  })
}

async function handleDeleteVoiceSession<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  response: ServerResponse,
  sessions: Map<string, ServerVoiceSession<TStartOptions, TCustomEvent>>,
  sessionId: string,
): Promise<void> {
  const session = sessions.get(sessionId)

  if (session) {
    sessions.delete(sessionId)

    for (const eventResponse of session.eventResponses) {
      eventResponse.end()
    }

    await session.session?.close()
  }

  writeJson(response, 200, {
    ok: true,
  })
}

function handleServerVoiceSessionEvent<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  session: ServerVoiceSession<TStartOptions, TCustomEvent>,
  event: VoiceEvent,
): void {
  if (event.type === 'input-transcription') {
    recordInputTranscription({
      session,
      transcription: event.transcription,
    })
  }

  if (event.type === 'text') {
    recordModelConversationText({
      session,
      text: event.text,
    })
  }

  if (event.type === 'turn-complete') {
    session.modelMessageOpen = false
  }

  emitVoiceSessionEvent(session, event)
}

function recordInputTranscription<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  args: RecordInputTranscriptionRequest<TStartOptions, TCustomEvent>,
): void {
  recordLearnerConversationText({
    ...(args.transcription.inputId ? { inputId: args.transcription.inputId } : {}),
    session: args.session,
    text: args.transcription.text,
  })

  if (!args.transcription.inputId) {
    return
  }

  args.session.inputTranscriptions.set(args.transcription.inputId, args.transcription.text)

  for (const waiter of args.session.inputTranscriptionWaiters.get(args.transcription.inputId) ?? []) {
    waiter(args.transcription.text)
  }

  args.session.inputTranscriptionWaiters.delete(args.transcription.inputId)
}

function recordLearnerConversationText<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  args: RecordLearnerConversationTextRequest<TStartOptions, TCustomEvent>,
): void {
  const text = args.text.trim()

  if (!text) {
    return
  }

  args.session.modelMessageOpen = false

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

function recordModelConversationText<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  args: RecordModelConversationTextRequest<TStartOptions, TCustomEvent>,
): void {
  if (!args.text) {
    return
  }

  if (args.session.modelMessageOpen && args.session.conversation[args.session.conversation.length - 1]?.role === 'model') {
    args.session.conversation[args.session.conversation.length - 1] = {
      role: 'model',
      text: `${args.session.conversation[args.session.conversation.length - 1]!.text}${args.text}`,
    }
  } else {
    args.session.conversation.push({
      role: 'model',
      text: args.text,
    })
  }

  args.session.modelMessageOpen = true
}

function getLatestModelConversationText<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  session: ServerVoiceSession<TStartOptions, TCustomEvent>,
): string | undefined {
  for (let index = session.conversation.length - 1; index >= 0; index--) {
    const message = session.conversation[index]!

    if (message.role === 'model') {
      return message.text.trim() || undefined
    }
  }

  return undefined
}

async function getLatestInputTranscription<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  args: GetLatestInputTranscriptionRequest<TStartOptions, TCustomEvent>,
): Promise<string | undefined> {
  if (!args.inputId) {
    return undefined
  }

  const inputId = args.inputId
  const transcription = args.session.inputTranscriptions.get(inputId)?.trim()

  if (transcription || !args.waitMs) {
    return transcription
  }

  return await new Promise(resolve => {
    let timeout: ReturnType<typeof setTimeout>
    const waiter: InputTranscriptionWaiter = text => {
      clearTimeout(timeout)
      resolve(text.trim() || undefined)
    }
    timeout = setTimeout(() => {
      args.session.inputTranscriptionWaiters.get(inputId)?.delete(waiter)

      if (args.session.inputTranscriptionWaiters.get(inputId)?.size === 0) {
        args.session.inputTranscriptionWaiters.delete(inputId)
      }

      resolve(undefined)
    }, args.waitMs)

    args.session.inputTranscriptionWaiters.set(inputId, (args.session.inputTranscriptionWaiters.get(inputId) ?? new Set()).add(waiter))
  })
}

function emitVoiceSessionEvent<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  session: ServerVoiceSession<TStartOptions, TCustomEvent>,
  event: VoiceEvent,
): void {
  const browserEvent = toBrowserVoiceSessionEvent(event)

  if (!browserEvent) {
    return
  }

  emitBrowserVoiceSessionEvent(session, browserEvent)
}

function emitBrowserVoiceSessionEvent<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent>(
  session: ServerVoiceSession<TStartOptions, TCustomEvent>,
  browserEvent: VoiceSessionClientEvent<TCustomEvent>,
): void {
  if (session.eventResponses.size === 0) {
    session.pendingEvents = [...session.pendingEvents, browserEvent].slice(-100)
    return
  }

  for (const response of session.eventResponses) {
    writeVoiceSessionEvent(response, browserEvent)
  }
}

function toBrowserVoiceSessionEvent(event: VoiceEvent): VoiceSessionClientEvent {
  switch (event.type) {
    case 'audio':
      return {
        type: 'audio',
        audio: {
          data: Buffer.from(event.audio.data).toString('base64'),
          mimeType: event.audio.mimeType,
        },
      }
    case 'error':
      return {
        type: 'error',
        message: event.error.message,
      }
    case 'input-transcription':
      return {
        type: 'input-transcription',
        transcription: event.transcription,
      }
    case 'text':
      return {
        type: 'text',
        text: event.text,
      }
    case 'turn-complete':
      return {
        type: 'turn-complete',
      }
  }
}

function writeVoiceSessionEvent<TCustomEvent extends VoiceSessionCustomEvent>(
  response: ServerResponse,
  event: VoiceSessionClientEvent<TCustomEvent>,
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

function toErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }

  return String(error)
}

function isSendVoiceSessionAudioRequest(body: SendVoiceSessionAudioRequest): body is ValidSendVoiceSessionAudioRequest {
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

function toEventArray<TCustomEvent extends VoiceSessionCustomEvent>(
  events: VoiceSessionClientEvent<TCustomEvent> | VoiceSessionClientEvent<TCustomEvent>[] | undefined,
): VoiceSessionClientEvent<TCustomEvent>[] {
  if (!events) {
    return []
  }

  return Array.isArray(events) ? events : [events]
}

type ServerVoiceSession<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent> = {
  session?: VoiceSession
  conversation: ServerVoiceSessionMessage[]
  eventResponses: Set<ServerResponse>
  inputTranscriptionWaiters: Map<string, Set<InputTranscriptionWaiter>>
  inputTranscriptions: Map<string, string>
  modelMessageOpen: boolean
  options: TStartOptions
  pendingEvents: VoiceSessionClientEvent<TCustomEvent>[]
}

type InputTranscriptionWaiter = (text: string) => void

type RecordInputTranscriptionRequest<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent> = {
  session: ServerVoiceSession<TStartOptions, TCustomEvent>
  transcription: Extract<VoiceEvent, { type: 'input-transcription' }>['transcription']
}

type RecordLearnerConversationTextRequest<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent> = {
  inputId?: string
  session: ServerVoiceSession<TStartOptions, TCustomEvent>
  text: string
}

type RecordModelConversationTextRequest<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent> = {
  session: ServerVoiceSession<TStartOptions, TCustomEvent>
  text: string
}

type GetLatestInputTranscriptionRequest<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent> = {
  inputId?: string
  session: ServerVoiceSession<TStartOptions, TCustomEvent>
  waitMs: number
}

type ServerVoiceSessionMessage =
  | {
      role: 'learner'
      inputId?: string
      text: string
    }
  | {
      role: 'model'
      text: string
    }

type SendVoiceSessionMessageRequest = {
  text?: unknown
}

type SendVoiceSessionAudioRequest = {
  audio?: unknown
  inputId?: unknown
}

type EndVoiceSessionAudioTurnRequest = {
  inputId?: unknown
}

type ValidSendVoiceSessionAudioRequest = {
  audio: {
    data: string
    mimeType: string
  }
  inputId?: unknown
}

type JsonObject = Record<string, unknown>
