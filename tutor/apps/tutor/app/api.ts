import type { ClientEvent, CreateSessionRequest, CreateSessionResponse, SendAudioTurnRequest } from '#tutor/app/generated/api.ts'

export type AudioInput = {
  data: Uint8Array
  mimeType: string
}

export type TutorSessionClient = ReturnType<typeof createTutorSessionClient>

export function createTutorSessionClient(path = '/api/turn-voice-sessions') {
  let eventSource: EventSource | undefined
  let requestQueue = Promise.resolve()
  let sessionId: string | undefined

  return {
    async start(request: CreateSessionRequest & { onEvent(event: ClientEvent): void }): Promise<void> {
      const { onEvent, ...body } = request
      closeEventSource()
      requestQueue = Promise.resolve()
      const session = await requestJson<CreateSessionResponse>(path, {
        body: JSON.stringify(body),
        headers: { 'content-type': 'application/json' },
        method: 'POST',
      })
      sessionId = session.id
      eventSource = new EventSource(`${path}/${session.id}/events`)
      eventSource.addEventListener('message', event => onEvent(JSON.parse(event.data) as ClientEvent))
      eventSource.addEventListener('error', () => onEvent({ type: 'error', message: 'Lost the turn voice session event stream.' }))
    },

    async sendAudioTurn(audio: AudioInput, inputId: string): Promise<void> {
      const id = activeSessionId()
      const body: SendAudioTurnRequest = {
        audio: { data: encodeBase64(audio.data), mimeType: audio.mimeType },
        inputId,
      }
      await enqueue(() =>
        requestJson(`${path}/${id}/audio-turns`, {
          body: JSON.stringify(body),
          headers: { 'content-type': 'application/json' },
          method: 'POST',
        }),
      )
    },

    async stop(): Promise<void> {
      const id = sessionId
      closeEventSource()
      await requestQueue.catch(() => {})
      if (id) {
        await requestJson(`${path}/${id}`, { method: 'DELETE' })
      }
    },
  }

  function activeSessionId(): string {
    if (!sessionId) throw new Error('Start a tutor session before sending microphone audio.')
    return sessionId
  }

  function closeEventSource(): void {
    eventSource?.close()
    eventSource = undefined
    sessionId = undefined
  }

  function enqueue<T>(request: () => Promise<T>): Promise<T> {
    const queued = requestQueue.catch(() => {}).then(request)
    requestQueue = queued.then(
      () => undefined,
      () => undefined,
    )
    return queued
  }
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000))
  }
  return btoa(binary)
}

async function requestJson<T = unknown>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init)
  const body = (await response.json().catch(() => ({}))) as { message?: unknown }
  if (!response.ok) {
    throw new Error(typeof body.message === 'string' ? body.message : 'Tutor API request failed.')
  }
  return body as T
}
