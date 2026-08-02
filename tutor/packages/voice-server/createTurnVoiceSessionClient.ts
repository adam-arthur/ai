export { createTurnVoiceSessionClient }

import type {
  CreateTurnVoiceSessionClientOptions,
  TurnVoiceSessionAudioInput,
  TurnVoiceSessionClient,
  TurnVoiceSessionClientEvent,
  TurnVoiceSessionCustomEvent,
  TurnVoiceSessionStartOptions,
} from '#voice-server/types.ts'

const defaultPath = '/api/turn-voice-sessions'

function createTurnVoiceSessionClient<
  TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent = never,
>(args: CreateTurnVoiceSessionClientOptions = {}): TurnVoiceSessionClient<TStartOptions, TCustomEvent> {
  let eventSource: EventSource | undefined
  let requestQueue = Promise.resolve()
  let sessionId: string | undefined

  return {
    async start(args) {
      const { onEvent, ...startOptions } = args

      closeEventSource()
      requestQueue = Promise.resolve()

      const session = await requestJson<StartTurnVoiceSessionResponse>(getPath(), {
        body: JSON.stringify(startOptions),
        headers: {
          'content-type': 'application/json',
        },
        method: 'POST',
      })

      sessionId = session.id
      eventSource = new EventSource(`${getPath()}/${session.id}/events`)
      eventSource.addEventListener('message', event => {
        onEvent(JSON.parse(event.data) as TurnVoiceSessionClientEvent<TCustomEvent>)
      })
      eventSource.addEventListener('error', () => {
        onEvent({
          type: 'error',
          message: 'Lost the turn voice session event stream.',
        })
      })
    },
    async sendAudioTurn(audio: TurnVoiceSessionAudioInput, inputId: string) {
      const activeSessionId = getActiveSessionId()

      await enqueueRequest(() =>
        requestJson(`${getPath()}/${activeSessionId}/audio-turns`, {
          body: JSON.stringify({
            audio: {
              data: encodeBase64(audio.data),
              mimeType: audio.mimeType,
            },
            inputId,
          }),
          headers: {
            'content-type': 'application/json',
          },
          method: 'POST',
        }),
      )
    },
    async stop() {
      const activeSessionId = sessionId

      closeEventSource()
      await requestQueue.catch(() => {})

      if (activeSessionId) {
        await requestJson(`${getPath()}/${activeSessionId}`, {
          method: 'DELETE',
        })
      }
    },
  }

  function closeEventSource(): void {
    eventSource?.close()
    eventSource = undefined
    sessionId = undefined
  }

  function getActiveSessionId(): string {
    if (!sessionId) {
      throw new Error('Start a turn voice session before sending microphone audio.')
    }

    return sessionId
  }

  function enqueueRequest<TResponse>(request: () => Promise<TResponse>): Promise<TResponse> {
    const queuedRequest = requestQueue.catch(() => {}).then(request)

    requestQueue = queuedRequest.then(
      () => undefined,
      () => undefined,
    )

    return queuedRequest
  }

  function getPath(): string {
    return args.path ?? defaultPath
  }
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = ''

  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000))
  }

  return btoa(binary)
}

async function requestJson<TResponse = unknown>(url: string, init: RequestInit): Promise<TResponse> {
  const response = await fetch(url, init)
  const body = (await response.json().catch(() => ({}))) as TurnVoiceSessionClientResponse

  if (!response.ok) {
    throw new Error(typeof body.message === 'string' ? body.message : 'Turn voice session request failed.')
  }

  return body as TResponse
}

type StartTurnVoiceSessionResponse = {
  id: string
}

type TurnVoiceSessionClientResponse = {
  message?: unknown
}
