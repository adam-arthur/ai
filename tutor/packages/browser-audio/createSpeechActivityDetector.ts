export { createSpeechActivityDetector, type SpeechActivityAnalysis, type SpeechActivityDetector, type SpeechActivityDetectorOptions }

import { FrameProcessor } from '@ricky0123/vad-web/dist/frame-processor.js'
import type { FrameProcessorEvent } from '@ricky0123/vad-web/dist/frame-processor.js'
import { Message } from '@ricky0123/vad-web/dist/messages.js'
import { SileroV5 } from '@ricky0123/vad-web/dist/models/index.js'
import * as ort from 'onnxruntime-web/wasm'

const defaultMinimumSpeechMs = 160
const defaultNegativeSpeechThreshold = 0.15
const defaultPositiveSpeechThreshold = 0.25
const defaultPreSpeechPadMs = 320
const defaultRedemptionMs = 500
const vadFrameSamples = 512
const vadSampleRateHertz = 16000

async function createSpeechActivityDetector(args: SpeechActivityDetectorOptions): Promise<SpeechActivityDetector> {
  const config = toSpeechActivityDetectorConfig(args)

  ort.env.logLevel = 'error'
  ort.env.wasm.numThreads = 1
  ort.env.wasm.wasmPaths = config.onnxWasmPaths

  const model = await SileroV5.new(ort, async () => {
    const response = await fetch(config.modelUrl)

    if (!response.ok) {
      throw new Error('Unable to load the voice activity detection model.')
    }

    return response.arrayBuffer()
  })
  const frameProcessor = new FrameProcessor(
    model.process,
    model.reset_state,
    {
      minSpeechMs: config.minSpeechMs,
      negativeSpeechThreshold: config.negativeSpeechThreshold,
      positiveSpeechThreshold: config.positiveSpeechThreshold,
      preSpeechPadMs: config.preSpeechPadMs,
      redemptionMs: config.redemptionMs,
      submitUserSpeechOnPause: false,
    },
    vadFrameSamples / (vadSampleRateHertz / 1000),
  )
  let analysisQueue = Promise.resolve()
  let resampler = createVadResampler()
  let validSpeechActive = false

  frameProcessor.resume()

  return {
    analyze(args) {
      const analysis = analysisQueue.then(async () => {
        const rms = calculateRms(args.samples)
        const state = {
          maxSpeechProbability: 0,
          speechDetected: false,
          voiceDetected: false,
        }

        for (const frame of resampler.process({
          sampleRateHertz: args.sampleRateHertz,
          samples: args.samples,
        })) {
          await frameProcessor.process(frame, event => {
            updateSpeechActivityState({
              config,
              event,
              state,
            })
          })
        }

        return {
          noiseRms: 0,
          rms,
          speechDetected: state.speechDetected,
          speechProbability: state.maxSpeechProbability,
          voiceDetected: state.voiceDetected || (validSpeechActive && state.speechDetected),
          zeroCrossingRate: calculateZeroCrossingRate(args.samples),
        }
      })

      analysisQueue = analysis.then(
        () => undefined,
        () => undefined,
      )

      return analysis
    },
    async close() {
      await analysisQueue
      await model.release()
    },
  }

  function updateSpeechActivityState(args: UpdateSpeechActivityStateArgs): void {
    switch (args.event.msg) {
      case Message.FrameProcessed:
        args.state.maxSpeechProbability = Math.max(args.state.maxSpeechProbability, args.event.probs.isSpeech)
        args.state.speechDetected = args.state.speechDetected || args.event.probs.isSpeech >= args.config.positiveSpeechThreshold
        break
      case Message.SpeechRealStart:
        validSpeechActive = true
        args.state.voiceDetected = true
        break
      case Message.SpeechEnd:
      case Message.VADMisfire:
        validSpeechActive = false
        break
    }
  }
}

function createVadResampler(): VadResampler {
  let inputSamples: number[] = []

  return {
    process(args) {
      const frames: Float32Array[] = []

      inputSamples = [...inputSamples, ...args.samples]

      while ((inputSamples.length * vadSampleRateHertz) / args.sampleRateHertz >= vadFrameSamples) {
        const frame = new Float32Array(vadFrameSamples)
        let inputIndex = 0

        for (let outputIndex = 0; outputIndex < frame.length; outputIndex += 1) {
          let sampleCount = 0
          let sampleTotal = 0

          while (inputIndex < Math.min(inputSamples.length, ((outputIndex + 1) * args.sampleRateHertz) / vadSampleRateHertz)) {
            sampleTotal += inputSamples[inputIndex] ?? 0
            sampleCount += 1
            inputIndex += 1
          }

          frame[outputIndex] = sampleCount ? sampleTotal / sampleCount : 0
        }

        inputSamples = inputSamples.slice(inputIndex)
        frames.push(frame)
      }

      return frames
    },
  }
}

function calculateRms(samples: Float32Array): number {
  return Math.sqrt(samples.reduce((total, sample) => total + sample * sample, 0) / samples.length)
}

function calculateZeroCrossingRate(samples: Float32Array): number {
  let zeroCrossings = 0

  for (let index = 1; index < samples.length; index += 1) {
    if ((samples[index - 1] ?? 0) < 0 !== (samples[index] ?? 0) < 0) {
      zeroCrossings += 1
    }
  }

  return zeroCrossings / samples.length
}

function toSpeechActivityDetectorConfig(args: SpeechActivityDetectorOptions): SpeechActivityDetectorConfig {
  const config = {
    minSpeechMs: toPositiveNumber({
      fallback: defaultMinimumSpeechMs,
      name: 'minSpeechMs',
      value: args.minSpeechMs,
    }),
    modelUrl: toRequiredString({
      name: 'modelUrl',
      value: args.modelUrl,
    }),
    negativeSpeechThreshold: toRatio({
      fallback: defaultNegativeSpeechThreshold,
      name: 'negativeSpeechThreshold',
      value: args.negativeSpeechThreshold,
    }),
    onnxWasmPaths: toRequiredWasmPaths(args.onnxWasmPaths),
    positiveSpeechThreshold: toRatio({
      fallback: defaultPositiveSpeechThreshold,
      name: 'positiveSpeechThreshold',
      value: args.positiveSpeechThreshold,
    }),
    preSpeechPadMs: toPositiveNumber({
      fallback: defaultPreSpeechPadMs,
      name: 'preSpeechPadMs',
      value: args.preSpeechPadMs,
    }),
    redemptionMs: toPositiveNumber({
      fallback: defaultRedemptionMs,
      name: 'redemptionMs',
      value: args.redemptionMs,
    }),
  }

  if (config.negativeSpeechThreshold >= config.positiveSpeechThreshold) {
    throw new Error('negativeSpeechThreshold must be lower than positiveSpeechThreshold.')
  }

  return config
}

function toPositiveNumber(args: SpeechActivityNumericOption): number {
  if (args.value === undefined) {
    return args.fallback
  }

  if (!Number.isFinite(args.value) || args.value <= 0) {
    throw new Error(`${args.name} must be a positive number.`)
  }

  return args.value
}

function toRatio(args: SpeechActivityNumericOption): number {
  if (args.value === undefined) {
    return args.fallback
  }

  if (!Number.isFinite(args.value) || args.value <= 0 || args.value >= 1) {
    throw new Error(`${args.name} must be greater than 0 and less than 1.`)
  }

  return args.value
}

function toRequiredString(args: SpeechActivityStringOption): string {
  if (args.value) {
    return args.value
  }

  throw new Error(`${args.name} is required.`)
}

function toRequiredWasmPaths(paths: SpeechActivityDetectorWasmPaths): SpeechActivityDetectorWasmPaths {
  if (paths?.mjs && paths.wasm) {
    return paths
  }

  throw new Error('onnxWasmPaths must include mjs and wasm asset URLs.')
}

type SpeechActivityDetector = {
  analyze(args: SpeechActivityDetectorAnalyzeArgs): Promise<SpeechActivityAnalysis>
  close(): Promise<void>
}

type SpeechActivityDetectorOptions = {
  minSpeechMs?: number
  modelUrl: string
  negativeSpeechThreshold?: number
  onnxWasmPaths: SpeechActivityDetectorWasmPaths
  positiveSpeechThreshold?: number
  preSpeechPadMs?: number
  redemptionMs?: number
}

type SpeechActivityDetectorWasmPaths = {
  mjs: string
  wasm: string
}

type SpeechActivityDetectorAnalyzeArgs = {
  sampleRateHertz: number
  samples: Float32Array
}

type SpeechActivityAnalysis = {
  noiseRms: number
  rms: number
  speechDetected: boolean
  speechProbability: number
  voiceDetected: boolean
  zeroCrossingRate: number
}

type SpeechActivityDetectorConfig = {
  minSpeechMs: number
  modelUrl: string
  negativeSpeechThreshold: number
  onnxWasmPaths: SpeechActivityDetectorWasmPaths
  positiveSpeechThreshold: number
  preSpeechPadMs: number
  redemptionMs: number
}

type SpeechActivityNumericOption = {
  fallback: number
  name: string
  value: number | undefined
}

type SpeechActivityStringOption = {
  name: string
  value: string
}

type VadResampler = {
  process(args: VadResamplerProcessArgs): Float32Array[]
}

type VadResamplerProcessArgs = {
  sampleRateHertz: number
  samples: Float32Array
}

type SpeechActivityState = {
  maxSpeechProbability: number
  speechDetected: boolean
  voiceDetected: boolean
}

type UpdateSpeechActivityStateArgs = {
  config: SpeechActivityDetectorConfig
  event: FrameProcessorEvent
  state: SpeechActivityState
}
