export { createPcmAudioPlayer, type PcmAudio, type PcmAudioPlayer }

function createPcmAudioPlayer(): PcmAudioPlayer {
  let audioContext: AudioContext | undefined
  let nextStartTime = 0
  let playbackRate = 1
  let sources: AudioBufferSourceNode[] = []

  return {
    async prepare() {
      if (getAudioContext().state === 'suspended') {
        await getAudioContext().resume()
      }
    },
    play(audio) {
      const context = getAudioContext()
      const source = context.createBufferSource()

      source.buffer = toAudioBuffer(context, decodeBase64(audio.data), getSampleRateHertz(audio.mimeType))
      source.playbackRate.value = playbackRate
      source.connect(context.destination)
      source.onended = () => {
        sources = sources.filter(existingSource => existingSource !== source)
      }
      source.start(Math.max(context.currentTime + 0.02, nextStartTime))
      nextStartTime = Math.max(context.currentTime + 0.02, nextStartTime) + source.buffer.duration / playbackRate
      sources = [...sources, source]
      void context.resume()
    },
    setPlaybackRate(rate) {
      playbackRate = toPlaybackRate(rate)

      for (const source of sources) {
        source.playbackRate.value = playbackRate
      }
    },
    reset() {
      for (const source of sources) {
        stopSource(source)
      }

      sources = []
      nextStartTime = audioContext?.currentTime ?? 0
    },
    async close() {
      this.reset()

      if (audioContext) {
        await audioContext.close()
        audioContext = undefined
      }

      nextStartTime = 0
    },
  }

  function getAudioContext(): AudioContext {
    return (audioContext ??= new AudioContext())
  }
}

function toAudioBuffer(context: AudioContext, bytes: Uint8Array, sampleRateHertz: number): AudioBuffer {
  const buffer = context.createBuffer(1, Math.floor(bytes.length / 2), sampleRateHertz)
  const channelData = buffer.getChannelData(0)
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)

  for (let index = 0; index < channelData.length; index += 1) {
    channelData[index] = view.getInt16(index * 2, true) / 0x8000
  }

  return buffer
}

function toPlaybackRate(rate: number): number {
  if (!Number.isFinite(rate) || rate < 0.25 || rate > 1.5) {
    throw new Error('Playback speed must be between 0.25 and 1.5.')
  }

  return rate
}

function getSampleRateHertz(mimeType: string): number {
  return Number(mimeType.match(/(?:^|;)rate=(\d+)(?:;|$)/)?.[1] ?? 24000)
}

function decodeBase64(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), character => character.charCodeAt(0))
}

function stopSource(source: AudioBufferSourceNode): void {
  try {
    source.stop()
  } catch {}
}

type PcmAudioPlayer = {
  prepare(): Promise<void>
  play(audio: PcmAudio): void
  setPlaybackRate(rate: number): void
  reset(): void
  close(): Promise<void>
}

type PcmAudio = {
  data: string
  mimeType: string
}
