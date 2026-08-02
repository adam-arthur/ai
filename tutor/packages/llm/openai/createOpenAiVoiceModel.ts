export { createOpenAiVoiceModel }

import OpenAI from 'openai'
import { OpenAIRealtimeWebSocket } from 'openai/realtime/websocket'
import type {
  ConversationItemInputAudioTranscriptionCompletedEvent,
  ConversationItemInputAudioTranscriptionFailedEvent,
  InputAudioBufferCommittedEvent,
  RealtimeAudioFormats,
  RealtimeClientEvent,
  ResponseAudioDeltaEvent,
  ResponseAudioTranscriptDeltaEvent,
  ResponseDoneEvent,
  ResponseTextDeltaEvent,
  SessionCreatedEvent,
} from 'openai/resources/realtime/realtime'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import type {
  LlmAudioFormat,
  LlmTurnInput,
  LlmVoiceAudioInput,
  LlmVoiceEvent,
  LlmVoiceEventHandler,
  LlmVoiceModel,
  LlmVoiceSessionConfig,
  LlmVoiceSessionOptions,
} from '#llm/core/types.ts'
import { toOpenAiRealtimeReasoning } from '#llm/openai/toOpenAiRealtimeReasoning.ts'
import { decodeBase64, encodeBase64, toError } from '#llm/voice/utils.ts'

function createOpenAiVoiceModel(args: OpenAiVoiceModelOptions): OpenAiVoiceModel {
  const apiKey = args.apiKey
  const realtimeWebSocketFactory = args.realtimeWebSocketFactory ?? createOpenAiRealtimeWebSocket

  return {
    async startVoiceSession(args: OpenAiVoiceSessionOptions) {
      const session = await realtimeWebSocketFactory({
        apiKey,
        model: args.config.model,
      })
      let audioTurnActive = false
      let activeAudioInputId: string | undefined
      let activeAudioTurnGuidance = ''
      const inputIdsByConversationItemId = new Map<string, string>()
      const pendingInputIds: (string | undefined)[] = []

      session.on('response.output_audio.delta', event => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'audio',
            audio: {
              data: decodeBase64(event.delta),
              mimeType: args.config.outputAudio?.mimeType ?? 'audio/pcm;rate=24000',
            },
          },
        })
      })
      session.on('response.output_audio_transcript.delta', event => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'text',
            text: event.delta,
          },
        })
      })
      session.on('response.output_text.delta', event => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'text',
            text: event.delta,
          },
        })
      })
      session.on('input_audio_buffer.committed', event => {
        const inputId = pendingInputIds.shift()

        if (inputId) {
          inputIdsByConversationItemId.set(event.item_id, inputId)
        }
      })
      session.on('conversation.item.input_audio_transcription.completed', event => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'input-transcription',
            transcription: {
              inputId: inputIdsByConversationItemId.get(event.item_id) ?? event.item_id,
              text: event.transcript,
            },
          },
        })
        inputIdsByConversationItemId.delete(event.item_id)
      })
      session.on('conversation.item.input_audio_transcription.failed', event => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'error',
            error: new Error(event.error.message ?? 'OpenAI input audio transcription failed.'),
          },
        })
        inputIdsByConversationItemId.delete(event.item_id)
      })
      session.on('response.done', () => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'turn-complete',
          },
        })
      })
      await waitForOpenAiRealtimeSession(session)
      session.on('error', error => {
        emitOpenAiVoiceEvent({
          onEvent: args.onEvent,
          event: {
            type: 'error',
            error,
          },
        })
      })

      session.send({
        type: 'session.update',
        session: toOpenAiRealtimeSession(args.config),
      })

      return {
        async send(args: OpenAiVoiceTurnInput) {
          if (args.type === 'audio') {
            activeAudioInputId = sendOpenAiAudioChunk({
              activeAudioInputId,
              input: args,
              session,
            })
            audioTurnActive = true
            activeAudioTurnGuidance = args.guidance?.instructions ?? activeAudioTurnGuidance
            sendOpenAiAudioTurnEnd({
              activeAudioInputId,
              activeAudioTurnGuidance,
              audioTurnActive,
              pendingInputIds,
              session,
            })
            audioTurnActive = false
            activeAudioInputId = undefined
            activeAudioTurnGuidance = ''
            return
          }

          sendOpenAiTextTurnInput({
            session,
            input: args,
          })
        },
        async appendAudio(args) {
          activeAudioInputId = sendOpenAiAudioChunk({
            activeAudioInputId,
            input: args,
            session,
          })
          audioTurnActive = true
          activeAudioTurnGuidance = args.guidance?.instructions ?? activeAudioTurnGuidance
        },
        async endAudioTurn(args) {
          sendOpenAiAudioTurnEnd({
            activeAudioInputId: args?.id ?? activeAudioInputId,
            activeAudioTurnGuidance: args?.guidance?.instructions ?? activeAudioTurnGuidance,
            audioTurnActive,
            pendingInputIds,
            session,
          })
          audioTurnActive = false
          activeAudioInputId = undefined
          activeAudioTurnGuidance = ''
        },
        async close() {
          session.close()
        },
      }
    },
  }
}

async function waitForOpenAiRealtimeSession(session: OpenAiRealtimeWebSocketSession): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const handleCreated = (): void => {
      session.off('error', handleError)
      session.off('session.created', handleCreated)
      resolve()
    }
    const handleError = (error: Error): void => {
      session.off('error', handleError)
      session.off('session.created', handleCreated)
      reject(error)
    }

    session.on('error', handleError)
    session.on('session.created', handleCreated)
  })
}

async function createOpenAiRealtimeWebSocket(args: OpenAiRealtimeWebSocketFactoryOptions): Promise<OpenAiRealtimeWebSocketSession> {
  return await OpenAIRealtimeWebSocket.create(new OpenAI({ apiKey: args.apiKey }), {
    model: args.model,
  })
}

function sendOpenAiTextTurnInput(args: SendOpenAiTextTurnInputRequest): void {
  args.session.send({
    type: 'conversation.item.create',
    item: {
      ...(args.input.id ? { id: args.input.id } : {}),
      type: 'message',
      role: 'user',
      content: [
        {
          type: 'input_text',
          text: args.input.text,
        },
      ],
    },
  })
  args.session.send({
    type: 'response.create',
    response: {
      output_modalities: ['audio'],
      ...(args.input.guidance?.instructions ? { instructions: args.input.guidance.instructions } : {}),
    },
  })
}

function sendOpenAiAudioChunk(args: SendOpenAiAudioChunkRequest): string | undefined {
  args.session.send({
    type: 'input_audio_buffer.append',
    audio: encodeBase64(args.input.audio.data),
  })

  return args.input.id ?? args.activeAudioInputId
}

function sendOpenAiAudioTurnEnd(args: SendOpenAiAudioTurnEndRequest): void {
  if (!args.audioTurnActive) {
    return
  }

  args.pendingInputIds.push(args.activeAudioInputId)
  args.session.send({
    type: 'input_audio_buffer.commit',
  })
  args.session.send({
    type: 'response.create',
    response: {
      output_modalities: ['audio'],
      ...(args.activeAudioTurnGuidance ? { instructions: args.activeAudioTurnGuidance } : {}),
    },
  })
}

function emitOpenAiVoiceEvent(args: EmitOpenAiVoiceEventRequest): void {
  void Promise.resolve(args.onEvent(args.event)).catch((error: unknown) => {
    if (args.event.type !== 'error') {
      void args.onEvent({
        type: 'error',
        error: toError(error),
      })
    }
  })
}

function toOpenAiRealtimeSession(args: OpenAiVoiceSessionConfig): OpenAiRealtimeSessionConfig {
  return {
    type: 'realtime',
    model: args.model,
    output_modalities: ['audio'],
    ...(args.thinkingLevel ? { reasoning: toOpenAiRealtimeReasoning(args.thinkingLevel) } : {}),
    audio: {
      input: {
        format: toOpenAiAudioFormat(args.inputAudio),
        ...(args.inputTranscription
          ? {
              transcription: {
                ...(args.inputTranscription.model ? { model: args.inputTranscription.model } : {}),
                ...(args.inputLanguageCode ? { language: args.inputLanguageCode } : {}),
                ...(args.inputTranscription.prompt ? { prompt: args.inputTranscription.prompt } : {}),
              },
            }
          : {}),
        turn_detection: null,
      },
      output: {
        format: toOpenAiAudioFormat(args.outputAudio),
        ...(args.speechSpeed === undefined ? {} : { speed: toOpenAiSpeechSpeed(args.speechSpeed) }),
        ...(args.voiceName ? { voice: args.voiceName } : {}),
      },
    },
    ...(args.systemPrompt ? { instructions: args.systemPrompt } : {}),
  }
}

function toOpenAiSpeechSpeed(speechSpeed: number): number {
  if (!Number.isFinite(speechSpeed) || speechSpeed < 0.25 || speechSpeed > 1.5) {
    throw new Error('OpenAI Realtime speech speed must be between 0.25 and 1.5.')
  }

  return speechSpeed
}

function toOpenAiAudioFormat(args: OpenAiVoiceAudioFormat | undefined): RealtimeAudioFormats {
  switch (toOpenAiAudioFormatType(args?.mimeType ?? 'audio/pcm')) {
    case 'audio/pcm':
      return {
        type: 'audio/pcm',
        rate: 24000,
      }
    case 'audio/pcmu':
      return {
        type: 'audio/pcmu',
      }
    case 'audio/pcma':
      return {
        type: 'audio/pcma',
      }
  }
}

function toOpenAiAudioFormatType(mimeType: string): OpenAiAudioFormatType {
  const type = mimeType.split(';')[0]

  if (type === 'audio/pcm' || type === 'audio/pcmu' || type === 'audio/pcma') {
    return type
  }

  throw new Error(`Unsupported OpenAI Realtime audio format "${mimeType}".`)
}

type OpenAiVoiceModel = LlmVoiceModel

type OpenAiVoiceSessionOptions = LlmVoiceSessionOptions

type OpenAiVoiceSessionConfig = LlmVoiceSessionConfig

type OpenAiVoiceTurnInput = LlmTurnInput

type OpenAiVoiceAudioInput = LlmVoiceAudioInput

type OpenAiVoiceAudioFormat = LlmAudioFormat

type OpenAiVoiceEvent = LlmVoiceEvent

type OpenAiVoiceEventHandler = LlmVoiceEventHandler

type OpenAiVoiceModelOptions = {
  apiKey: string
  realtimeWebSocketFactory?: OpenAiRealtimeWebSocketFactory
}

type OpenAiRealtimeWebSocketFactory = (
  args: OpenAiRealtimeWebSocketFactoryOptions,
) => OpenAiRealtimeWebSocketSession | Promise<OpenAiRealtimeWebSocketSession>

type SendOpenAiTextTurnInputRequest = {
  session: OpenAiRealtimeWebSocketSession
  input: Extract<OpenAiVoiceTurnInput, { type: 'text' }>
}

type SendOpenAiAudioChunkRequest = {
  activeAudioInputId: string | undefined
  input: OpenAiVoiceAudioInput
  session: OpenAiRealtimeWebSocketSession
}

type SendOpenAiAudioTurnEndRequest = {
  activeAudioInputId: string | undefined
  activeAudioTurnGuidance: string
  audioTurnActive: boolean
  pendingInputIds: (string | undefined)[]
  session: OpenAiRealtimeWebSocketSession
}

type EmitOpenAiVoiceEventRequest = {
  onEvent: OpenAiVoiceEventHandler
  event: OpenAiVoiceEvent
}

type OpenAiRealtimeWebSocketFactoryOptions = {
  apiKey: string
  model: OpenAiVoiceSessionConfig['model']
}

type OpenAiRealtimeWebSocketSession = {
  close(): void
  off(event: 'error', listener: (error: Error) => unknown): OpenAiRealtimeWebSocketSession
  off(event: 'session.created', listener: (event: SessionCreatedEvent) => unknown): OpenAiRealtimeWebSocketSession
  on(event: 'error', listener: (error: Error) => unknown): OpenAiRealtimeWebSocketSession
  on(event: 'input_audio_buffer.committed', listener: (event: InputAudioBufferCommittedEvent) => unknown): OpenAiRealtimeWebSocketSession
  on(
    event: 'conversation.item.input_audio_transcription.completed',
    listener: (event: ConversationItemInputAudioTranscriptionCompletedEvent) => unknown,
  ): OpenAiRealtimeWebSocketSession
  on(
    event: 'conversation.item.input_audio_transcription.failed',
    listener: (event: ConversationItemInputAudioTranscriptionFailedEvent) => unknown,
  ): OpenAiRealtimeWebSocketSession
  on(event: 'response.done', listener: (event: ResponseDoneEvent) => unknown): OpenAiRealtimeWebSocketSession
  on(event: 'response.output_audio.delta', listener: (event: ResponseAudioDeltaEvent) => unknown): OpenAiRealtimeWebSocketSession
  on(
    event: 'response.output_audio_transcript.delta',
    listener: (event: ResponseAudioTranscriptDeltaEvent) => unknown,
  ): OpenAiRealtimeWebSocketSession
  on(event: 'response.output_text.delta', listener: (event: ResponseTextDeltaEvent) => unknown): OpenAiRealtimeWebSocketSession
  on(event: 'session.created', listener: (event: SessionCreatedEvent) => unknown): OpenAiRealtimeWebSocketSession
  send(args: RealtimeClientEvent): void
}

type OpenAiRealtimeSessionConfig = Extract<
  RealtimeClientEvent,
  {
    type: 'session.update'
  }
>['session']

type OpenAiAudioFormatType = 'audio/pcm' | 'audio/pcma' | 'audio/pcmu'

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  class TestOpenAiRealtimeWebSocketSession {
    readonly listeners: Record<string, ((event: unknown) => unknown)[]> = {}
    readonly sentEvents: RealtimeClientEvent[] = []
    sessionCreatedQueued = false

    close(): void {}

    emit(event: string, payload: unknown): void {
      for (const listener of this.listeners[event] ?? []) {
        listener(payload)
      }
    }

    off(event: 'error', listener: (error: Error) => unknown): TestOpenAiRealtimeWebSocketSession
    off(event: 'session.created', listener: (event: SessionCreatedEvent) => unknown): TestOpenAiRealtimeWebSocketSession
    off(event: string, listener: (event: never) => unknown): TestOpenAiRealtimeWebSocketSession {
      this.listeners[event] = (this.listeners[event] ?? []).filter(existingListener => existingListener !== listener)
      return this
    }

    on(event: 'error', listener: (error: Error) => unknown): TestOpenAiRealtimeWebSocketSession
    on(
      event: 'input_audio_buffer.committed',
      listener: (event: InputAudioBufferCommittedEvent) => unknown,
    ): TestOpenAiRealtimeWebSocketSession
    on(
      event: 'conversation.item.input_audio_transcription.completed',
      listener: (event: ConversationItemInputAudioTranscriptionCompletedEvent) => unknown,
    ): TestOpenAiRealtimeWebSocketSession
    on(
      event: 'conversation.item.input_audio_transcription.failed',
      listener: (event: ConversationItemInputAudioTranscriptionFailedEvent) => unknown,
    ): TestOpenAiRealtimeWebSocketSession
    on(event: 'response.done', listener: (event: ResponseDoneEvent) => unknown): TestOpenAiRealtimeWebSocketSession
    on(event: 'response.output_audio.delta', listener: (event: ResponseAudioDeltaEvent) => unknown): TestOpenAiRealtimeWebSocketSession
    on(
      event: 'response.output_audio_transcript.delta',
      listener: (event: ResponseAudioTranscriptDeltaEvent) => unknown,
    ): TestOpenAiRealtimeWebSocketSession
    on(event: 'response.output_text.delta', listener: (event: ResponseTextDeltaEvent) => unknown): TestOpenAiRealtimeWebSocketSession
    on(event: 'session.created', listener: (event: SessionCreatedEvent) => unknown): TestOpenAiRealtimeWebSocketSession
    on(event: string, listener: (event: never) => unknown): TestOpenAiRealtimeWebSocketSession {
      this.listeners[event] = [...(this.listeners[event] ?? []), listener as (event: unknown) => unknown]

      if (event === 'session.created' && !this.sessionCreatedQueued) {
        this.sessionCreatedQueued = true
        queueMicrotask(() => {
          this.emit(event, {
            event_id: 'event_123',
            type: 'session.created',
            session: {
              type: 'realtime',
            },
          })
        })
      }

      return this
    }

    send(args: RealtimeClientEvent): void {
      this.sentEvents.push(args)
    }
  }

  test('connects through the OpenAI Realtime SDK and sends text turns', async () => {
    const events: OpenAiVoiceEvent[] = []
    const factoryOptions: OpenAiRealtimeWebSocketFactoryOptions[] = []
    const realtimeSession = new TestOpenAiRealtimeWebSocketSession()

    const session = await createOpenAiVoiceModel({
      apiKey: 'test-api-key',
      realtimeWebSocketFactory(args) {
        factoryOptions.push(args)
        return realtimeSession
      },
    }).startVoiceSession({
      config: {
        model: 'gpt-realtime-2',
        speechSpeed: 1.25,
        systemPrompt: 'Speak briefly.',
        voiceName: 'marin',
      },
      onEvent(args) {
        events.push(args)
      },
    })

    await session.send({
      type: 'text',
      text: 'Say ready.',
    })
    realtimeSession.emit('response.output_audio.delta', { type: 'response.output_audio.delta', delta: 'AQID' })
    realtimeSession.emit('response.output_audio_transcript.delta', { type: 'response.output_audio_transcript.delta', delta: 'ready' })
    realtimeSession.emit('response.done', { type: 'response.done' })

    assert.deepEqual(factoryOptions, [
      {
        apiKey: 'test-api-key',
        model: 'gpt-realtime-2',
      },
    ])
    assert.deepEqual(realtimeSession.sentEvents, [
      {
        type: 'session.update',
        session: {
          type: 'realtime',
          model: 'gpt-realtime-2',
          output_modalities: ['audio'],
          audio: {
            input: {
              format: {
                type: 'audio/pcm',
                rate: 24000,
              },
              turn_detection: null,
            },
            output: {
              format: {
                type: 'audio/pcm',
                rate: 24000,
              },
              speed: 1.25,
              voice: 'marin',
            },
          },
          instructions: 'Speak briefly.',
        },
      },
      {
        type: 'conversation.item.create',
        item: {
          type: 'message',
          role: 'user',
          content: [
            {
              type: 'input_text',
              text: 'Say ready.',
            },
          ],
        },
      },
      {
        type: 'response.create',
        response: {
          output_modalities: ['audio'],
        },
      },
    ])
    assert.deepEqual(events, [
      {
        type: 'audio',
        audio: {
          data: Uint8Array.from([1, 2, 3]),
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      {
        type: 'text',
        text: 'ready',
      },
      {
        type: 'turn-complete',
      },
    ])
  })

  test('sends voice turn guidance as OpenAI response instructions', async () => {
    const realtimeSession = new TestOpenAiRealtimeWebSocketSession()

    const session = await createOpenAiVoiceModel({
      apiKey: 'test-api-key',
      realtimeWebSocketFactory() {
        return realtimeSession
      },
    }).startVoiceSession({
      config: {
        model: 'gpt-realtime-2',
      },
      onEvent() {},
    })

    await session.send({
      type: 'text',
      text: '저는 학교 가요.',
      guidance: {
        instructions: 'Internal tutor note. Correct the particle briefly before replying.',
      },
    })

    assert.deepEqual(realtimeSession.sentEvents.slice(1), [
      {
        type: 'conversation.item.create',
        item: {
          type: 'message',
          role: 'user',
          content: [
            {
              type: 'input_text',
              text: '저는 학교 가요.',
            },
          ],
        },
      },
      {
        type: 'response.create',
        response: {
          output_modalities: ['audio'],
          instructions: 'Internal tutor note. Correct the particle briefly before replying.',
        },
      },
    ])
  })

  test('sets OpenAI Realtime reasoning effort when thinking is requested', async () => {
    const realtimeSession = new TestOpenAiRealtimeWebSocketSession()

    await createOpenAiVoiceModel({
      apiKey: 'test-api-key',
      realtimeWebSocketFactory() {
        return realtimeSession
      },
    }).startVoiceSession({
      config: {
        model: 'gpt-realtime-2',
        thinkingLevel: 'medium',
      },
      onEvent() {},
    })

    assert.deepEqual(realtimeSession.sentEvents[0], {
      type: 'session.update',
      session: {
        type: 'realtime',
        model: 'gpt-realtime-2',
        output_modalities: ['audio'],
        reasoning: {
          effort: 'medium',
        },
        audio: {
          input: {
            format: {
              type: 'audio/pcm',
              rate: 24000,
            },
            turn_detection: null,
          },
          output: {
            format: {
              type: 'audio/pcm',
              rate: 24000,
            },
          },
        },
      },
    })
  })

  test('streams OpenAI audio chunks until the audio turn ends', async () => {
    const realtimeSession = new TestOpenAiRealtimeWebSocketSession()

    const session = await createOpenAiVoiceModel({
      apiKey: 'test-api-key',
      realtimeWebSocketFactory() {
        return realtimeSession
      },
    }).startVoiceSession({
      config: {
        model: 'gpt-realtime-2',
      },
      onEvent() {},
    })

    await session.appendAudio({
      id: 'input_test_123',
      audio: {
        data: Uint8Array.from([1, 2, 3]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    await session.appendAudio({
      id: 'input_test_123',
      audio: {
        data: Uint8Array.from([4, 5, 6]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })

    assert.deepEqual(realtimeSession.sentEvents.slice(1), [
      {
        type: 'input_audio_buffer.append',
        audio: 'AQID',
      },
      {
        type: 'input_audio_buffer.append',
        audio: 'BAUG',
      },
    ])

    await session.endAudioTurn({
      id: 'input_test_123',
    })

    assert.deepEqual(realtimeSession.sentEvents.slice(1), [
      {
        type: 'input_audio_buffer.append',
        audio: 'AQID',
      },
      {
        type: 'input_audio_buffer.append',
        audio: 'BAUG',
      },
      {
        type: 'input_audio_buffer.commit',
      },
      {
        type: 'response.create',
        response: {
          output_modalities: ['audio'],
        },
      },
    ])
  })

  test('sends audio turns through the input audio buffer', async () => {
    const events: OpenAiVoiceEvent[] = []
    const realtimeSession = new TestOpenAiRealtimeWebSocketSession()

    const session = await createOpenAiVoiceModel({
      apiKey: 'test-api-key',
      realtimeWebSocketFactory() {
        return realtimeSession
      },
    }).startVoiceSession({
      config: {
        model: 'gpt-realtime-2',
        inputLanguageCode: 'en',
        inputTranscription: {
          model: 'gpt-4o-mini-transcribe',
          prompt: 'Expect short tutoring questions.',
        },
        inputAudio: {
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      onEvent(args) {
        events.push(args)
      },
    })

    await session.send({
      id: 'input_test_123',
      type: 'audio',
      audio: {
        data: Uint8Array.from([4, 5, 6]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })

    assert.deepEqual(realtimeSession.sentEvents, [
      {
        type: 'session.update',
        session: {
          type: 'realtime',
          model: 'gpt-realtime-2',
          output_modalities: ['audio'],
          audio: {
            input: {
              format: {
                type: 'audio/pcm',
                rate: 24000,
              },
              transcription: {
                model: 'gpt-4o-mini-transcribe',
                language: 'en',
                prompt: 'Expect short tutoring questions.',
              },
              turn_detection: null,
            },
            output: {
              format: {
                type: 'audio/pcm',
                rate: 24000,
              },
            },
          },
        },
      },
      {
        type: 'input_audio_buffer.append',
        audio: 'BAUG',
      },
      {
        type: 'input_audio_buffer.commit',
      },
      {
        type: 'response.create',
        response: {
          output_modalities: ['audio'],
        },
      },
    ])
    realtimeSession.emit('input_audio_buffer.committed', {
      event_id: 'event_commit_123',
      item_id: 'item_openai_123',
      type: 'input_audio_buffer.committed',
    })
    realtimeSession.emit('conversation.item.input_audio_transcription.completed', {
      content_index: 0,
      event_id: 'event_transcription_123',
      item_id: 'item_openai_123',
      transcript: 'Can you explain fractions?',
      type: 'conversation.item.input_audio_transcription.completed',
      usage: {
        seconds: 1,
        type: 'duration',
      },
    })

    assert.deepEqual(events, [
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_123',
          text: 'Can you explain fractions?',
        },
      },
    ])
  })
})
