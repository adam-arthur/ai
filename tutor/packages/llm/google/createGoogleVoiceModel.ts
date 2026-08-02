export { createGoogleVoiceModel, type GoogleVoiceModelOptions }

import { ActivityHandling, GoogleGenAI, Modality, ThinkingLevel } from '@google/genai'
import type { LiveConnectConfig, LiveSendClientContentParameters, LiveSendRealtimeInputParameters, LiveServerMessage } from '@google/genai'

import { sourceTests } from '@ai/testing'
import type { SourceTestContext } from '@ai/testing'

import { toGoogleThinkingConfig } from '#llm/google/toGoogleThinkingConfig.ts'
import type {
  GoogleVoiceModel,
  GoogleVoiceAudioChunk,
  GoogleVoiceModelEvent,
  GoogleVoiceModelEventHandler,
  GoogleVoiceSessionConfig,
  GoogleVoiceSessionOptions,
  GoogleVoiceTurnInput,
} from '#llm/google/types.ts'
import { decodeBase64, encodeBase64, toError } from '#llm/voice/utils.ts'

function createGoogleVoiceModel(args: GoogleVoiceModelOptions): GoogleVoiceModel {
  const apiKey = args.apiKey
  const liveConnectFactory = args.liveConnectFactory ?? createGoogleLiveConnection

  return {
    async startVoiceSession(args: GoogleVoiceSessionOptions) {
      if (args.config.speechSpeed !== undefined) {
        throw new Error('Gemini voice sessions do not support speech speed yet.')
      }

      const pendingInputs: PendingGoogleVoiceInput[] = []
      const session = await liveConnectFactory({
        apiKey,
        model: args.config.model,
        config: toLiveConnectConfig(args.config),
        callbacks: {
          onmessage(message) {
            emitGoogleVoiceMessage({
              sessionOptions: args,
              message,
              pendingInputs,
            })
          },
          onerror(error) {
            emitGoogleVoiceEvent({
              onEvent: args.onEvent,
              event: {
                type: 'error',
                error: toError(error),
              },
            })
          },
        },
      })
      let activeAudioInputId: string | undefined
      let activeAudioTurnGuidance = ''
      let audioTurnActive = false

      return {
        async send(args: GoogleVoiceTurnInput) {
          if (args.type === 'audio') {
            activeAudioInputId = sendGoogleAudioChunk({
              activeAudioInputId,
              input: args,
              pendingInputs,
              session,
            })
            audioTurnActive = true
            activeAudioTurnGuidance = args.guidance?.instructions ?? activeAudioTurnGuidance
            sendGoogleAudioTurnEnd({
              activeAudioTurnGuidance,
              audioTurnActive,
              session,
            })
            activeAudioInputId = undefined
            activeAudioTurnGuidance = ''
            audioTurnActive = false
            return
          }

          sendGoogleTextTurnInput({
            input: args,
            session,
          })
        },
        async appendAudio(args) {
          activeAudioInputId = sendGoogleAudioChunk({
            activeAudioInputId,
            input: args,
            pendingInputs,
            session,
          })
          audioTurnActive = true
          activeAudioTurnGuidance = args.guidance?.instructions ?? activeAudioTurnGuidance
        },
        async endAudioTurn(args) {
          sendGoogleAudioTurnEnd({
            activeAudioTurnGuidance: args?.guidance?.instructions ?? activeAudioTurnGuidance,
            audioTurnActive,
            session,
          })
          activeAudioInputId = undefined
          activeAudioTurnGuidance = ''
          audioTurnActive = false
        },
        async close() {
          session.close()
        },
      }
    },
  }
}

function createGoogleLiveConnection(args: GoogleLiveConnectFactoryOptions): GoogleLiveSession | Promise<GoogleLiveSession> {
  return new GoogleGenAI({ apiKey: args.apiKey }).live.connect({
    model: args.model,
    config: args.config,
    callbacks: args.callbacks,
  })
}

function toLiveConnectConfig(args: GoogleVoiceSessionConfig): LiveConnectConfig {
  return {
    responseModalities: [Modality.AUDIO],
    thinkingConfig: toGoogleThinkingConfig(args.thinkingLevel),
    realtimeInputConfig: {
      activityHandling: ActivityHandling.START_OF_ACTIVITY_INTERRUPTS,
      automaticActivityDetection: {
        disabled: true,
      },
    },
    ...(args.systemPrompt ? { systemInstruction: args.systemPrompt } : {}),
    ...toInputAudioTranscriptionConfig(args),
    ...toOutputAudioTranscriptionConfig(args),
    ...toSpeechConfig(args),
  }
}

function toLiveRealtimeInput(args: GoogleVoiceAudioInput): LiveSendRealtimeInputParameters {
  return {
    audio: {
      data: encodeBase64(args.audio.data),
      mimeType: args.audio.mimeType,
    },
  }
}

function sendGoogleTextTurnInput(args: SendGoogleTextTurnInputRequest): void {
  if (args.input.guidance?.instructions) {
    sendGoogleTurnGuidance({
      instructions: args.input.guidance.instructions,
      session: args.session,
    })
  }

  args.session.sendClientContent({
    turns: {
      role: 'user',
      parts: [
        {
          text: args.input.text,
        },
      ],
    },
    turnComplete: true,
  })
}

function sendGoogleAudioChunk(args: SendGoogleAudioChunkRequest): string | undefined {
  if (!args.activeAudioInputId) {
    if (args.pendingInputs[0]?.turnCompleteReceived) {
      args.pendingInputs.shift()
    }

    args.pendingInputs.push({
      ...(args.input.id ? { id: args.input.id } : {}),
      transcriptionReceived: false,
      turnCompleteReceived: false,
    })

    args.session.sendRealtimeInput({
      activityStart: {},
    })
  }

  args.session.sendRealtimeInput(toLiveRealtimeInput({ audio: args.input.audio }))

  return args.input.id ?? args.activeAudioInputId
}

function sendGoogleAudioTurnEnd(args: SendGoogleAudioTurnEndRequest): void {
  if (!args.audioTurnActive) {
    return
  }

  if (args.activeAudioTurnGuidance) {
    sendGoogleTurnGuidance({
      instructions: args.activeAudioTurnGuidance,
      session: args.session,
    })
  }

  args.session.sendRealtimeInput({
    activityEnd: {},
  })
}

function sendGoogleTurnGuidance(args: SendGoogleTurnGuidanceRequest): void {
  args.session.sendClientContent({
    turns: {
      role: 'user',
      parts: [
        {
          text: args.instructions,
        },
      ],
    },
    turnComplete: false,
  })
}

function emitGoogleVoiceMessage(args: EmitGoogleVoiceMessageRequest): void {
  const pendingInput = args.pendingInputs[0]

  for (const part of args.message.serverContent?.modelTurn?.parts ?? []) {
    if (part.inlineData?.data) {
      emitGoogleVoiceEvent({
        onEvent: args.sessionOptions.onEvent,
        event: {
          type: 'audio',
          audio: {
            data: decodeBase64(part.inlineData.data),
            mimeType: part.inlineData.mimeType ?? args.sessionOptions.config.outputAudio?.mimeType ?? 'audio/pcm;rate=24000',
          },
        },
      })
    }

    if (part.text) {
      emitGoogleVoiceEvent({
        onEvent: args.sessionOptions.onEvent,
        event: {
          type: 'text',
          text: part.text,
        },
      })
    }
  }

  if (args.message.serverContent?.outputTranscription?.text) {
    emitGoogleVoiceEvent({
      onEvent: args.sessionOptions.onEvent,
      event: {
        type: 'text',
        text: args.message.serverContent.outputTranscription.text,
      },
    })
  }

  if (args.message.serverContent?.inputTranscription) {
    const transcription = args.message.serverContent.inputTranscription

    if (transcription.text) {
      if (pendingInput) {
        pendingInput.transcriptionReceived = true
      }

      emitGoogleVoiceEvent({
        onEvent: args.sessionOptions.onEvent,
        event: {
          type: 'input-transcription',
          transcription: {
            ...(pendingInput?.id ? { inputId: pendingInput.id } : {}),
            text: transcription.text,
          },
        },
      })
    }
  }

  if (args.message.serverContent?.turnComplete && pendingInput) {
    pendingInput.turnCompleteReceived = true
  }

  if (
    args.pendingInputs[0] === pendingInput &&
    (args.message.serverContent?.inputTranscription?.finished ||
      (pendingInput?.turnCompleteReceived && pendingInput.transcriptionReceived && args.pendingInputs.length > 1))
  ) {
    args.pendingInputs.shift()
  }

  if (args.message.serverContent?.turnComplete) {
    emitGoogleVoiceEvent({
      onEvent: args.sessionOptions.onEvent,
      event: {
        type: 'turn-complete',
      },
    })
  }
}

function emitGoogleVoiceEvent(args: EmitGoogleVoiceEventRequest): void {
  void Promise.resolve(args.onEvent(args.event)).catch((error: unknown) => {
    if (args.event.type !== 'error') {
      void args.onEvent({
        type: 'error',
        error: toError(error),
      })
    }
  })
}

function toInputAudioTranscriptionConfig(args: GoogleVoiceSessionConfig): Pick<LiveConnectConfig, 'inputAudioTranscription'> {
  if (!args.inputLanguageCode && !args.inputTranscription) {
    return {}
  }

  return {
    inputAudioTranscription: args.inputLanguageCode ? { languageCodes: [args.inputLanguageCode] } : {},
  }
}

function toOutputAudioTranscriptionConfig(args: GoogleVoiceSessionConfig): Pick<LiveConnectConfig, 'outputAudioTranscription'> {
  if (!args.outputLanguageCode && !args.outputTranscription) {
    return {}
  }

  return {
    outputAudioTranscription: args.outputLanguageCode ? { languageCodes: [args.outputLanguageCode] } : {},
  }
}

function toSpeechConfig(args: GoogleVoiceSessionConfig): Pick<LiveConnectConfig, 'speechConfig'> {
  if (!args.outputLanguageCode && !args.voiceName) {
    return {}
  }

  return {
    speechConfig: {
      ...(args.outputLanguageCode ? { languageCode: args.outputLanguageCode } : {}),
      ...(args.voiceName
        ? {
            voiceConfig: {
              prebuiltVoiceConfig: {
                voiceName: args.voiceName,
              },
            },
          }
        : {}),
    },
  }
}

type GoogleVoiceModelOptions = {
  apiKey: string
  liveConnectFactory?: GoogleLiveConnectFactory
}

type GoogleLiveConnectFactory = (args: GoogleLiveConnectFactoryOptions) => GoogleLiveSession | Promise<GoogleLiveSession>

type GoogleLiveConnectFactoryOptions = {
  apiKey: string
  model: GoogleVoiceSessionConfig['model']
  config: LiveConnectConfig
  callbacks: {
    onmessage(args: LiveServerMessage): void
    onerror(error: unknown): void
  }
}

type GoogleLiveSession = {
  sendClientContent(args: LiveSendClientContentParameters): void
  sendRealtimeInput(args: LiveSendRealtimeInputParameters): void
  close(): void
}

type EmitGoogleVoiceMessageRequest = {
  sessionOptions: GoogleVoiceSessionOptions
  message: LiveServerMessage
  pendingInputs: PendingGoogleVoiceInput[]
}

type SendGoogleTextTurnInputRequest = {
  input: Extract<GoogleVoiceTurnInput, { type: 'text' }>
  session: GoogleLiveSession
}

type GoogleVoiceAudioInput = {
  audio: GoogleVoiceAudioChunk
}

type SendGoogleAudioChunkRequest = {
  activeAudioInputId: string | undefined
  input: {
    id?: string
    guidance?: GoogleVoiceTurnInput['guidance']
    audio: GoogleVoiceAudioChunk
  }
  pendingInputs: PendingGoogleVoiceInput[]
  session: GoogleLiveSession
}

type SendGoogleAudioTurnEndRequest = {
  activeAudioTurnGuidance: string
  audioTurnActive: boolean
  session: GoogleLiveSession
}

type SendGoogleTurnGuidanceRequest = {
  instructions: string
  session: GoogleLiveSession
}

type EmitGoogleVoiceEventRequest = {
  onEvent: GoogleVoiceModelEventHandler
  event: GoogleVoiceModelEvent
}

type PendingGoogleVoiceInput = {
  id?: string
  transcriptionReceived: boolean
  turnCompleteReceived: boolean
}

sourceTests(import.meta, ({ test, assert: sourceAssert }: SourceTestContext) => {
  const assert: SourceTestContext['assert'] = sourceAssert

  class TestGoogleLiveSession {
    readonly sentClientContents: LiveSendClientContentParameters[] = []
    readonly sentInputs: LiveSendRealtimeInputParameters[] = []
    readonly sentOperationTypes: string[] = []

    callbacks: GoogleLiveConnectFactoryOptions['callbacks'] | undefined

    close(): void {}

    sendClientContent(args: LiveSendClientContentParameters): void {
      this.sentOperationTypes.push('clientContent')
      this.sentClientContents.push(args)
    }

    sendRealtimeInput(args: LiveSendRealtimeInputParameters): void {
      this.sentOperationTypes.push('realtimeInput')
      this.sentInputs.push(args)
    }
  }

  test('rejects speech speed until Gemini supports it', async () => {
    await assert.rejects(
      async () =>
        await createGoogleVoiceModel({ apiKey: 'test-api-key' }).startVoiceSession({
          config: {
            model: 'gemini-3.1-flash-live-preview',
            speechSpeed: 1,
          },
          onEvent() {},
        }),
      { message: 'Gemini voice sessions do not support speech speed yet.' },
    )
  })

  test('connects through the Google Live SDK and sends finite audio turns', async () => {
    const factoryOptions: GoogleLiveConnectFactoryOptions[] = []
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        factoryOptions.push(args)
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        inputTranscription: {},
        systemPrompt: 'Speak briefly.',
      },
      onEvent() {},
    })

    await session.send({
      id: 'input_test_123',
      type: 'audio',
      audio: {
        data: Uint8Array.from([1, 2, 3]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })

    assert.equal(factoryOptions.length, 1)
    assert.equal(factoryOptions[0]?.apiKey, 'test-api-key')
    assert.equal(factoryOptions[0]?.model, 'gemini-3.1-flash-live-preview')
    assert.deepEqual(factoryOptions[0]?.config.thinkingConfig, {
      thinkingBudget: 0,
    })
    assert.deepEqual(factoryOptions[0]?.config.inputAudioTranscription, {})
    assert.deepEqual(liveSession.sentInputs, [
      {
        activityStart: {},
      },
      {
        audio: {
          data: 'AQID',
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      {
        activityEnd: {},
      },
    ])
  })

  test('enables Gemini output transcription with language detection', async () => {
    const factoryOptions: GoogleLiveConnectFactoryOptions[] = []

    await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        factoryOptions.push(args)
        return new TestGoogleLiveSession()
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        outputTranscription: {},
      },
      onEvent() {},
    })

    assert.deepEqual(factoryOptions[0]?.config.outputAudioTranscription, {})
  })

  test('streams Gemini audio chunks until the audio turn ends', async () => {
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
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

    assert.deepEqual(liveSession.sentInputs, [
      {
        activityStart: {},
      },
      {
        audio: {
          data: 'AQID',
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      {
        audio: {
          data: 'BAUG',
          mimeType: 'audio/pcm;rate=24000',
        },
      },
    ])

    await session.endAudioTurn()

    assert.deepEqual(liveSession.sentInputs, [
      {
        activityStart: {},
      },
      {
        audio: {
          data: 'AQID',
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      {
        audio: {
          data: 'BAUG',
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      {
        activityEnd: {},
      },
    ])
  })

  test('sets Gemini voice thinking level when requested', async () => {
    const factoryOptions: GoogleLiveConnectFactoryOptions[] = []

    await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        factoryOptions.push(args)
        return new TestGoogleLiveSession()
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        thinkingLevel: 'high',
      },
      onEvent() {},
    })

    assert.deepEqual(factoryOptions[0]?.config.thinkingConfig, {
      thinkingLevel: ThinkingLevel.HIGH,
    })
  })

  test('sends streamed voice turn guidance as Gemini client content before ending audio input', async () => {
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
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
    await session.endAudioTurn({
      id: 'input_test_123',
      guidance: {
        instructions: 'Internal tutor note. Correct the particle briefly before replying.',
      },
    })

    assert.deepEqual(liveSession.sentClientContents, [
      {
        turns: {
          role: 'user',
          parts: [
            {
              text: 'Internal tutor note. Correct the particle briefly before replying.',
            },
          ],
        },
        turnComplete: false,
      },
    ])
    assert.deepEqual(liveSession.sentOperationTypes, ['realtimeInput', 'realtimeInput', 'clientContent', 'realtimeInput'])
    assert.deepEqual(liveSession.sentInputs, [
      {
        activityStart: {},
      },
      {
        audio: {
          data: 'AQID',
          mimeType: 'audio/pcm;rate=24000',
        },
      },
      {
        activityEnd: {},
      },
    ])
  })

  test('emits every Gemini output transcription text update', async () => {
    const events: GoogleVoiceModelEvent[] = []
    const liveSession = new TestGoogleLiveSession()

    await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        outputTranscription: {},
      },
      onEvent(args) {
        events.push(args)
      },
    })

    liveSession.callbacks?.onmessage({
      serverContent: {
        outputTranscription: {
          text: '아, 흐려요? ',
        },
      },
    } as LiveServerMessage)
    liveSession.callbacks?.onmessage({
      serverContent: {
        outputTranscription: {
          text: '비가 올 것 같아요.',
          finished: true,
          languageCode: 'ko-KR',
        },
      },
    } as LiveServerMessage)

    assert.deepEqual(events, [
      {
        type: 'text',
        text: '아, 흐려요? ',
      },
      {
        type: 'text',
        text: '비가 올 것 같아요.',
      },
    ])
  })

  test('emits Gemini input transcription with the matching audio input id', async () => {
    const events: GoogleVoiceModelEvent[] = []
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        inputTranscription: {},
      },
      onEvent(args) {
        events.push(args)
      },
    })

    await session.send({
      id: 'input_test_123',
      type: 'audio',
      audio: {
        data: Uint8Array.from([1, 2, 3]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'annyeonghaseyo',
          finished: true,
        },
      },
    } as LiveServerMessage)

    assert.deepEqual(events, [
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_123',
          text: 'annyeonghaseyo',
        },
      },
    ])
  })

  test('keeps Gemini input transcription updates on the same audio input until the turn ends', async () => {
    const events: GoogleVoiceModelEvent[] = []
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        inputTranscription: {},
      },
      onEvent(args) {
        events.push(args)
      },
    })

    await session.send({
      id: 'input_test_123',
      type: 'audio',
      audio: {
        data: Uint8Array.from([1, 2, 3]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'annyeong',
        },
      },
    } as LiveServerMessage)
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'annyeonghaseyo',
        },
      },
    } as LiveServerMessage)
    liveSession.callbacks?.onmessage({
      serverContent: {
        turnComplete: true,
      },
    } as LiveServerMessage)

    assert.deepEqual(events, [
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_123',
          text: 'annyeong',
        },
      },
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_123',
          text: 'annyeonghaseyo',
        },
      },
      {
        type: 'turn-complete',
      },
    ])
  })

  test('advances Gemini input ids when a transcript turn completes without a finished marker', async () => {
    const events: GoogleVoiceModelEvent[] = []
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        inputTranscription: {},
      },
      onEvent(args) {
        events.push(args)
      },
    })

    await session.send({
      id: 'input_test_123',
      type: 'audio',
      audio: {
        data: Uint8Array.from([1, 2, 3]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'first turn',
        },
        turnComplete: true,
      },
    } as LiveServerMessage)
    await session.send({
      id: 'input_test_456',
      type: 'audio',
      audio: {
        data: Uint8Array.from([4, 5, 6]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'second turn',
        },
        turnComplete: true,
      },
    } as LiveServerMessage)

    assert.deepEqual(events, [
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_123',
          text: 'first turn',
        },
      },
      {
        type: 'turn-complete',
      },
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_456',
          text: 'second turn',
        },
      },
      {
        type: 'turn-complete',
      },
    ])
  })

  test('maps delayed Gemini input transcription to the completed audio turn', async () => {
    const events: GoogleVoiceModelEvent[] = []
    const liveSession = new TestGoogleLiveSession()

    const session = await createGoogleVoiceModel({
      apiKey: 'test-api-key',
      liveConnectFactory(args) {
        liveSession.callbacks = args.callbacks
        return liveSession
      },
    }).startVoiceSession({
      config: {
        model: 'gemini-3.1-flash-live-preview',
        inputTranscription: {},
      },
      onEvent(args) {
        events.push(args)
      },
    })

    await session.send({
      id: 'input_test_123',
      type: 'audio',
      audio: {
        data: Uint8Array.from([1, 2, 3]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    liveSession.callbacks?.onmessage({
      serverContent: {
        turnComplete: true,
      },
    } as LiveServerMessage)
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'delayed first turn',
        },
      },
    } as LiveServerMessage)
    await session.send({
      id: 'input_test_456',
      type: 'audio',
      audio: {
        data: Uint8Array.from([4, 5, 6]),
        mimeType: 'audio/pcm;rate=24000',
      },
    })
    liveSession.callbacks?.onmessage({
      serverContent: {
        inputTranscription: {
          text: 'second turn',
        },
        turnComplete: true,
      },
    } as LiveServerMessage)

    assert.deepEqual(events, [
      {
        type: 'turn-complete',
      },
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_123',
          text: 'delayed first turn',
        },
      },
      {
        type: 'input-transcription',
        transcription: {
          inputId: 'input_test_456',
          text: 'second turn',
        },
      },
      {
        type: 'turn-complete',
      },
    ])
  })
})
