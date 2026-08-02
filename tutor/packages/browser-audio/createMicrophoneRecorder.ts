export {
  createMicrophoneRecorder,
  type MicrophoneAudioChunk,
  type MicrophoneRecording,
  type MicrophoneRecorder,
  type MicrophoneVoiceDetectionOptions,
}

import { createSpeechActivityDetector } from '#browser-audio/createSpeechActivityDetector.ts'
import type { SpeechActivityDetector, SpeechActivityDetectorOptions } from '#browser-audio/createSpeechActivityDetector.ts'

const audioBufferSize = 4096
const microphoneSampleRateHertz = 24000
const microphoneMimeType = `audio/pcm;rate=${microphoneSampleRateHertz}`
const microphoneMediaConstraints = {
  audio: {
    autoGainControl: { ideal: false },
    channelCount: { ideal: 1 },
    echoCancellation: { ideal: true },
    noiseSuppression: { ideal: true },
  },
  video: false,
} satisfies MediaStreamConstraints

function createMicrophoneRecorder(): MicrophoneRecorder {
  let activeRecording: ActiveMicrophoneRecording | undefined

  return {
    async start() {
      if (activeRecording) {
        return
      }

      if (!navigator.mediaDevices?.getUserMedia) {
        throw new Error('This browser does not support microphone recording.')
      }

      const stream = await navigator.mediaDevices.getUserMedia(microphoneMediaConstraints)

      try {
        const samples: Float32Array[] = []

        activeRecording = await startAudioProcessing({
          samples,
          stream,
          onSamples(args) {
            samples.push(args.samples)
          },
        })
      } catch (error) {
        stopStream(stream)
        throw error
      }
    },
    async stop() {
      if (!activeRecording) {
        throw new Error('Start recording before sending microphone audio.')
      }

      const recording = activeRecording
      const sampleRateHertz = recording.audioContext.sampleRate

      activeRecording = undefined
      await releaseRecording(recording)

      if (recording.samples.length === 0) {
        throw new Error('No microphone audio was captured.')
      }

      return {
        data: toPcm16Audio(mergeFloat32Arrays(recording.samples), sampleRateHertz, microphoneSampleRateHertz),
        mimeType: microphoneMimeType,
      }
    },
    async startStreaming(args) {
      if (activeRecording) {
        return
      }

      if (!navigator.mediaDevices?.getUserMedia) {
        throw new Error('This browser does not support microphone recording.')
      }

      const stream = await navigator.mediaDevices.getUserMedia(microphoneMediaConstraints)
      let speechActivityDetector: SpeechActivityDetector | undefined

      try {
        const loadedSpeechActivityDetector = await createSpeechActivityDetector(args.voiceDetection)
        let recording: ActiveMicrophoneRecording | undefined

        speechActivityDetector = loadedSpeechActivityDetector
        recording = await startAudioProcessing({
          release: loadedSpeechActivityDetector.close,
          samples: [],
          stream,
          onSamples(audio) {
            void loadedSpeechActivityDetector
              .analyze({
                sampleRateHertz: audio.sampleRateHertz,
                samples: audio.samples,
              })
              .then(activity => {
                if (!recording || activeRecording !== recording) {
                  return
                }

                return args.onAudio({
                  data: toPcm16Audio(audio.samples, audio.sampleRateHertz, microphoneSampleRateHertz),
                  mimeType: microphoneMimeType,
                  speechDetected: activity.speechDetected,
                  voiceDetected: activity.voiceDetected,
                })
              })
              .catch(error => args.onError?.(error))
          },
        })
        activeRecording = recording
      } catch (error) {
        await speechActivityDetector?.close().catch(() => {})
        stopStream(stream)
        throw error
      }
    },
    async cancel() {
      if (!activeRecording) {
        return
      }

      const recording = activeRecording

      activeRecording = undefined
      await releaseRecording(recording)
    },
  }
}

async function startAudioProcessing(args: StartAudioProcessingArgs): Promise<ActiveMicrophoneRecording> {
  const audioContext = new AudioContext()
  const source = audioContext.createMediaStreamSource(args.stream)
  const highPassFilter = audioContext.createBiquadFilter()
  const lowPassFilter = audioContext.createBiquadFilter()
  const processor = audioContext.createScriptProcessor(audioBufferSize, 1, 1)
  const mutedOutput = audioContext.createGain()

  highPassFilter.frequency.value = 80
  highPassFilter.type = 'highpass'
  lowPassFilter.frequency.value = 8000
  lowPassFilter.type = 'lowpass'
  mutedOutput.gain.value = 0
  processor.onaudioprocess = event => {
    args.onSamples({
      sampleRateHertz: audioContext.sampleRate,
      samples: new Float32Array(event.inputBuffer.getChannelData(0)),
    })
  }
  source.connect(highPassFilter)
  highPassFilter.connect(lowPassFilter)
  lowPassFilter.connect(processor)
  processor.connect(mutedOutput)
  mutedOutput.connect(audioContext.destination)
  await audioContext.resume()

  return {
    audioContext,
    highPassFilter,
    lowPassFilter,
    mutedOutput,
    processor,
    ...(args.release ? { release: args.release } : {}),
    samples: args.samples,
    source,
    stream: args.stream,
  }
}

async function releaseRecording(recording: ActiveMicrophoneRecording): Promise<void> {
  recording.processor.onaudioprocess = null
  recording.source.disconnect()
  recording.highPassFilter.disconnect()
  recording.lowPassFilter.disconnect()
  recording.processor.disconnect()
  recording.mutedOutput.disconnect()
  stopStream(recording.stream)
  await recording.audioContext.close()
  await recording.release?.()
}

function stopStream(stream: MediaStream): void {
  for (const track of stream.getTracks()) {
    track.stop()
  }
}

function mergeFloat32Arrays(chunks: Float32Array[]): Float32Array {
  const samples = new Float32Array(chunks.reduce((totalLength, chunk) => totalLength + chunk.length, 0))
  let offset = 0

  for (const chunk of chunks) {
    samples.set(chunk, offset)
    offset += chunk.length
  }

  return samples
}

function toPcm16Audio(samples: Float32Array, sourceSampleRateHertz: number, targetSampleRateHertz: number): Uint8Array {
  const output = new Uint8Array(Math.floor((samples.length * targetSampleRateHertz) / sourceSampleRateHertz) * 2)
  const view = new DataView(output.buffer)
  const sampleRateRatio = sourceSampleRateHertz / targetSampleRateHertz

  for (let index = 0; index < output.length / 2; index += 1) {
    view.setInt16(index * 2, toPcm16Sample(samples[Math.min(samples.length - 1, Math.floor(index * sampleRateRatio))] ?? 0), true)
  }

  return output
}

function toPcm16Sample(sample: number): number {
  return Math.max(-1, Math.min(1, sample)) < 0 ? Math.max(-1, Math.min(1, sample)) * 0x8000 : Math.max(-1, Math.min(1, sample)) * 0x7fff
}

type MicrophoneRecorder = {
  start(): Promise<void>
  startStreaming(args: MicrophoneStreamingArgs): Promise<void>
  stop(): Promise<MicrophoneRecording>
  cancel(): Promise<void>
}

type MicrophoneRecording = {
  data: Uint8Array
  mimeType: string
}

type MicrophoneAudioChunk = MicrophoneRecording & {
  speechDetected: boolean
  voiceDetected: boolean
}

type MicrophoneStreamingArgs = {
  onAudio(args: MicrophoneAudioChunk): void | Promise<void>
  onError?(error: unknown): void
  voiceDetection: MicrophoneVoiceDetectionOptions
}

type MicrophoneVoiceDetectionOptions = SpeechActivityDetectorOptions

type ActiveMicrophoneRecording = {
  audioContext: AudioContext
  highPassFilter: BiquadFilterNode
  lowPassFilter: BiquadFilterNode
  mutedOutput: GainNode
  processor: ScriptProcessorNode
  release?: () => Promise<void>
  samples: Float32Array[]
  source: MediaStreamAudioSourceNode
  stream: MediaStream
}

type StartAudioProcessingArgs = {
  samples: Float32Array[]
  stream: MediaStream
  onSamples(args: MicrophoneSamples): void
  release?: () => Promise<void>
}

type MicrophoneSamples = {
  sampleRateHertz: number
  samples: Float32Array
}
