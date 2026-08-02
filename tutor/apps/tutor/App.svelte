<script lang="ts">
  import { onDestroy } from 'svelte'

  import sileroVadModelUrl from '@ricky0123/vad-web/dist/silero_vad_v5.onnx?url'
  import ErrorAlert from '#tutor/app/ErrorAlert.svelte'
  import MicrophoneControl from '#tutor/app/MicrophoneControl.svelte'
  import SessionHeader from '#tutor/app/SessionHeader.svelte'
  import TranscriptPanel from '#tutor/app/TranscriptPanel.svelte'
  import TutorSettings from '#tutor/app/TutorSettings.svelte'
  import { formatMessageTime, toErrorMessage } from '#tutor/app/utils.ts'
  import type { InputTranscription, MicrophoneStatus, SessionStatus, TranscriptMessage, TurnMistakes } from '#tutor/app/types.ts'
  import { createMicrophoneRecorder } from '@ai/browser-audio/createMicrophoneRecorder.ts'
  import { createPcmAudioPlayer } from '@ai/browser-audio/createPcmAudioPlayer.ts'
  import { createKoreanTutorTurnVoiceSessionClient } from '@ai/language-tutor/korean/createKoreanTutorTurnVoiceSessionClient.ts'
  import type {
    KoreanTutorLevel,
    KoreanTutorTurnModelConfiguration,
    KoreanTutorTurnVoiceSessionAudioInput,
    KoreanTutorTurnVoiceSessionClientEvent,
  } from '@ai/language-tutor/korean/types.ts'
  import { defaultKoreanTutorTurnModelConfiguration } from '@ai/language-tutor/korean/types.ts'
  import onnxRuntimeWasmModuleUrl from 'onnxruntime-web/ort-wasm-simd-threaded.mjs?url'
  import onnxRuntimeWasmUrl from 'onnxruntime-web/ort-wasm-simd-threaded.wasm?url'

  const prespeechChunkLimit = 4
  const microphoneRecorder = createMicrophoneRecorder()
  const pcmAudioPlayer = createPcmAudioPlayer()
  const turnVoiceSessionClient = createKoreanTutorTurnVoiceSessionClient()

  let activeAgentMessageId = $state<string | undefined>()
  let activeAudioChunks: KoreanTutorTurnVoiceSessionAudioInput[] = []
  let autoPauseAfterResponse = $state(false)
  let autoTurnSilenceMs = $state(1000)
  let currentInputId = $state<string | undefined>()
  let errorMessage = $state<string | undefined>()
  let microphoneStatus = $state<MicrophoneStatus>('idle')
  let modelConfiguration = $state<KoreanTutorTurnModelConfiguration>({ ...defaultKoreanTutorTurnModelConfiguration })
  let pendingAudioChunks: KoreanTutorTurnVoiceSessionAudioInput[] = []
  let playbackSpeed = $state(1)
  let silenceStartedAt: number | undefined
  let sessionStatus = $state<SessionStatus>('idle')
  let transcriptMessages = $state<TranscriptMessage[]>([])
  let tutorLevel = $state<KoreanTutorLevel>('A1')
  let turnProcessing = $state(false)

  const canControlMicrophone = $derived(sessionStatus === 'active')
  const microphoneActive = $derived(microphoneStatus === 'listening')
  const microphoneButtonLabel = $derived(microphoneStatus === 'paused' ? 'Resume listening' : 'Pause')
  const microphoneStatusLabel = $derived(
    turnProcessing
      ? 'Processing your turn'
      : microphoneStatus === 'listening'
      ? currentInputId
        ? 'Listening to your turn'
        : 'Listening for your voice'
      : microphoneStatus === 'paused'
        ? currentInputId
          ? 'Paused mid-turn'
          : 'Paused'
        : sessionStatus === 'active'
          ? 'Microphone starting'
          : 'Microphone idle',
  )
  const sessionActive = $derived(sessionStatus === 'active')
  const sessionBusy = $derived(sessionStatus === 'starting' || sessionStatus === 'stopping')
  const sessionControlDisabled = $derived(sessionBusy)
  const sessionSettingDisabled = $derived(sessionStatus !== 'idle')
  const sessionButtonLabel = $derived(
    sessionStatus === 'starting'
      ? 'Starting'
      : sessionStatus === 'stopping'
        ? 'Ending'
        : sessionStatus === 'active'
          ? 'End session'
          : 'Start voice session',
  )
  const sessionStatusLabel = $derived(
    sessionStatus === 'starting'
      ? 'Starting voice session'
      : sessionStatus === 'stopping'
        ? 'Ending voice session'
        : sessionStatus === 'active'
          ? 'Voice session active'
          : 'Session idle',
  )
  const turnVoiceSessionEventHandlers = {
    audio(event) {
      try {
        pcmAudioPlayer.play(event.audio)
      } catch (error) {
        errorMessage = toErrorMessage(error)
      }
    },
    error(event) {
      errorMessage = event.message
    },
    'input-transcription'(event) {
      updateInputTranscription(event.transcription)
    },
    text(event) {
      appendAgentText(event.text)
    },
    'turn-complete'() {
      activeAgentMessageId = undefined
      turnProcessing = false
      if (autoPauseAfterResponse && sessionStatus === 'active') {
        void pauseListening()
      }
    },
    'turn-mistakes'(event) {
      updateTurnMistakes(event)
    },
  } satisfies TurnVoiceSessionEventHandlerMap

  onDestroy(() => {
    void microphoneRecorder.cancel()
    void pcmAudioPlayer.close()
    void turnVoiceSessionClient.stop()
  })

  $effect(() => {
    pcmAudioPlayer.setPlaybackRate(playbackSpeed)
  })

  async function toggleSession(): Promise<void> {
    if (sessionActive) {
      await stopSession()
      return
    }

    await startSession()
  }

  async function startSession(): Promise<void> {
    activeAgentMessageId = undefined
    activeAudioChunks = []
    currentInputId = undefined
    errorMessage = undefined
    pendingAudioChunks = []
    silenceStartedAt = undefined
    transcriptMessages = []
    microphoneStatus = 'idle'
    sessionStatus = 'starting'
    turnProcessing = false

    try {
      await pcmAudioPlayer.prepare()
      await turnVoiceSessionClient.start({
        level: tutorLevel,
        modelConfiguration,
        onEvent: handleTurnVoiceSessionEvent,
      })
      sessionStatus = 'active'
      await resumeListening()
    } catch (error) {
      errorMessage = toErrorMessage(error)
      await microphoneRecorder.cancel()
      await turnVoiceSessionClient.stop().catch(() => {})
      microphoneStatus = 'idle'
      sessionStatus = 'idle'
    }
  }

  async function stopSession(): Promise<void> {
    sessionStatus = 'stopping'

    try {
      await microphoneRecorder.cancel()
      activeAudioChunks = []
      currentInputId = undefined
      microphoneStatus = 'idle'
      pendingAudioChunks = []
      pcmAudioPlayer.reset()
      silenceStartedAt = undefined
      turnProcessing = false
      await turnVoiceSessionClient.stop()
    } catch (error) {
      errorMessage = toErrorMessage(error)
    } finally {
      activeAgentMessageId = undefined
      sessionStatus = 'idle'
    }
  }

  function handleTurnVoiceSessionEvent(event: KoreanTutorTurnVoiceSessionClientEvent): void {
    const handleEvent = turnVoiceSessionEventHandlers[event.type] as (event: KoreanTutorTurnVoiceSessionClientEvent) => void

    handleEvent(event)
  }

  async function toggleMicrophone(): Promise<void> {
    if (microphoneActive) {
      await pauseListening()
      return
    }

    await resumeListening()
  }

  async function resumeListening(): Promise<void> {
    if (!sessionActive || microphoneStatus === 'listening') {
      return
    }

    errorMessage = undefined
    pendingAudioChunks = []
    silenceStartedAt = undefined

    try {
      await pcmAudioPlayer.prepare()
      await microphoneRecorder.startStreaming({
        voiceDetection: {
          minSpeechMs: 160,
          modelUrl: sileroVadModelUrl,
          negativeSpeechThreshold: 0.15,
          onnxWasmPaths: {
            mjs: onnxRuntimeWasmModuleUrl,
            wasm: onnxRuntimeWasmUrl,
          },
          positiveSpeechThreshold: 0.25,
          preSpeechPadMs: 320,
          redemptionMs: 500,
        },
        onAudio: handleMicrophoneAudio,
        onError(error) {
          errorMessage = toErrorMessage(error)
          void pauseListening()
        },
      })
      microphoneStatus = 'listening'
    } catch (error) {
      errorMessage = toErrorMessage(error)
      microphoneStatus = sessionActive ? 'paused' : 'idle'
    }
  }

  async function pauseListening(): Promise<void> {
    if (!sessionActive || microphoneStatus !== 'listening') {
      return
    }

    pendingAudioChunks = []
    silenceStartedAt = undefined
    await microphoneRecorder.cancel()
    microphoneStatus = 'paused'
  }

  function toggleAutoPauseAfterResponse(): void {
    autoPauseAfterResponse = !autoPauseAfterResponse
  }

  function handleMicrophoneAudio(audio: KoreanTutorTurnVoiceSessionAudioInput & { speechDetected: boolean; voiceDetected: boolean }): void {
    if (turnProcessing) {
      return
    }

    if (sessionStatus !== 'active' || microphoneStatus !== 'listening') {
      return
    }

    errorMessage = undefined

    if (!currentInputId) {
      pendingAudioChunks = [...pendingAudioChunks, audio].slice(-prespeechChunkLimit)

      if (!audio.voiceDetected) {
        return
      }

      currentInputId = `input_${crypto.randomUUID().replaceAll('-', '')}`
      activeAudioChunks = pendingAudioChunks
      addTranscriptMessage('learner', 'Listening...', currentInputId)

      pendingAudioChunks = []
      silenceStartedAt = undefined
      return
    }

    activeAudioChunks = [...activeAudioChunks, audio]

    if (audio.voiceDetected || audio.speechDetected) {
      silenceStartedAt = undefined
      return
    }

    silenceStartedAt ??= performance.now()

    if (performance.now() - silenceStartedAt >= autoTurnSilenceMs) {
      void endCurrentAudioTurn()
    }
  }

  async function endCurrentAudioTurn(): Promise<void> {
    if (!currentInputId || turnProcessing) {
      return
    }

    const inputId = currentInputId
    const audioChunks = activeAudioChunks

    activeAudioChunks = []
    currentInputId = undefined
    pendingAudioChunks = []
    silenceStartedAt = undefined
    turnProcessing = true
    updateInputTranscription({
      inputId,
      text: 'Transcribing...',
    })

    try {
      await turnVoiceSessionClient.sendAudioTurn(mergeAudioChunks(audioChunks), inputId)
    } catch (error) {
      transcriptMessages = transcriptMessages.filter(message => message.id !== inputId)
      errorMessage = toErrorMessage(error)
      await pauseListening()
    } finally {
      turnProcessing = false
    }
  }

  function mergeAudioChunks(audioChunks: KoreanTutorTurnVoiceSessionAudioInput[]): KoreanTutorTurnVoiceSessionAudioInput {
    const firstChunk = audioChunks[0]

    if (!firstChunk) {
      throw new Error('No microphone audio was captured.')
    }

    if (audioChunks.some(audioChunk => audioChunk.mimeType !== firstChunk.mimeType)) {
      throw new Error('Microphone audio chunks used mixed formats.')
    }

    const data = new Uint8Array(audioChunks.reduce((totalLength, audioChunk) => totalLength + audioChunk.data.length, 0))
    let offset = 0

    for (const audioChunk of audioChunks) {
      data.set(audioChunk.data, offset)
      offset += audioChunk.data.length
    }

    return {
      data,
      mimeType: firstChunk.mimeType,
    }
  }

  function appendAgentText(text: string): void {
    if (!activeAgentMessageId) {
      activeAgentMessageId = addTranscriptMessage('agent', text)
      return
    }

    transcriptMessages = transcriptMessages.map(message =>
      message.id === activeAgentMessageId
        ? {
            ...message,
            text: `${message.text}${text}`,
          }
        : message,
    )
  }

  function updateInputTranscription(transcription: InputTranscription): void {
    if (!transcription.inputId) {
      addTranscriptMessage('learner', transcription.text)
      return
    }

    transcriptMessages = transcriptMessages.some(message => message.id === transcription.inputId)
      ? transcriptMessages.map(message =>
          message.id === transcription.inputId
            ? {
                ...message,
                text: transcription.text,
              }
            : message,
        )
      : [
          ...transcriptMessages,
          {
            id: transcription.inputId,
            speaker: 'learner',
            text: transcription.text,
            time: formatMessageTime(),
          },
        ]
  }

  function updateTurnMistakes(turnMistakes: TurnMistakes): void {
    if (!turnMistakes.inputId) {
      return
    }

    transcriptMessages = transcriptMessages.map(message =>
      message.id === turnMistakes.inputId
        ? {
            ...message,
            mistakes: turnMistakes.mistakes,
          }
        : message,
    )
  }

  function addTranscriptMessage(speaker: TranscriptMessage['speaker'], text: string, id: string = crypto.randomUUID()): string {
    transcriptMessages = [
      ...transcriptMessages,
      {
        id,
        speaker,
        text,
        time: formatMessageTime(),
      },
    ]

    return id
  }

  type TurnVoiceSessionEventHandlerMap = {
    [TEvent in KoreanTutorTurnVoiceSessionClientEvent as TEvent['type']]: (event: TEvent) => void
  }
</script>

<main class="shell">
  <SessionHeader
    {sessionActive}
    {sessionButtonLabel}
    {sessionControlDisabled}
    {sessionStatusLabel}
    {toggleSession}
  />

  <TutorSettings
    bind:autoTurnSilenceMs
    bind:modelConfiguration
    bind:playbackSpeed
    bind:tutorLevel
    {sessionBusy}
    {sessionSettingDisabled}
  />

  {#if errorMessage}
    <ErrorAlert message={errorMessage} />
  {/if}

  <TranscriptPanel messages={transcriptMessages} />

  <MicrophoneControl
    {autoPauseAfterResponse}
    {canControlMicrophone}
    {microphoneActive}
    {microphoneButtonLabel}
    {microphoneStatusLabel}
    {toggleAutoPauseAfterResponse}
    {toggleMicrophone}
  />
</main>

<style>
  :global(:root) {
    color-scheme: dark;
    --color-page: #08090d;
    --color-page-raised: #101217;
    --color-surface: #14171d;
    --color-surface-elevated: #191d25;
    --color-surface-muted: #0f1218;
    --color-control: #1d232d;
    --color-control-hover: #262f3c;
    --color-text: #f4f7fb;
    --color-text-muted: #a6b0c1;
    --color-text-subtle: #737f91;
    --color-border: rgba(166, 176, 193, 0.18);
    --color-border-strong: rgba(198, 209, 224, 0.32);
    --color-primary: #39d0a0;
    --color-primary-strong: #63e6be;
    --color-primary-soft: rgba(57, 208, 160, 0.14);
    --color-primary-border: rgba(57, 208, 160, 0.22);
    --color-accent: #7aa2ff;
    --color-accent-soft: rgba(122, 162, 255, 0.16);
    --color-accent-border: rgba(122, 162, 255, 0.24);
    --color-danger: #ff6b5f;
    --color-danger-strong: #ff8b82;
    --color-danger-soft: rgba(255, 107, 95, 0.15);
    --color-success: #39d0a0;
    --color-error-surface: rgba(255, 107, 95, 0.1);
    --radius-control: 6px;
    --radius-panel: 8px;
    --shadow-panel: 0 18px 48px rgba(0, 0, 0, 0.36);
    --shadow-control: 0 10px 24px rgba(0, 0, 0, 0.24);
    --focus-ring: 0 0 0 3px rgba(57, 208, 160, 0.24);
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
    background:
      linear-gradient(180deg, var(--color-page-raised) 0%, var(--color-page) 42%, var(--color-page) 100%),
      var(--color-page);
    color: var(--color-text);
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }

  :global(button) {
    min-height: 44px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: var(--color-primary);
    color: var(--color-page);
    cursor: pointer;
    font: inherit;
    font-weight: 800;
    padding: 0 18px;
    transition:
      background 140ms ease,
      border-color 140ms ease,
      color 140ms ease,
      opacity 140ms ease,
      transform 140ms ease;
  }

  :global(button:not(:disabled):hover) {
    background: var(--color-primary-strong);
    transform: translateY(-1px);
  }

  :global(button:focus-visible) {
    outline: 0;
    box-shadow: var(--focus-ring);
  }

  :global(button:disabled) {
    cursor: not-allowed;
    opacity: 0.62;
  }

  :global(button.active),
  :global(button.recording) {
    background: var(--color-danger);
    color: var(--color-page);
  }

  :global(button.active:not(:disabled):hover),
  :global(button.recording:not(:disabled):hover) {
    background: var(--color-danger-strong);
  }

  .shell {
    width: min(920px, calc(100vw - 32px));
    margin: 0 auto;
    padding: 40px 0 28px;
  }

  @media (max-width: 560px) {
    .shell {
      width: min(100vw - 24px, 920px);
      padding: 24px 0 18px;
    }
  }
</style>
