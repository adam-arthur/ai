export type {
  CreateTurnVoiceSessionClientOptions,
  CreateTurnVoiceSessionRequest,
  CreateTurnVoiceSessionRequestArgs,
  CreateVoiceSessionClientOptions,
  TurnVoiceSessionAudioInput,
  TurnVoiceSessionAudioOutput,
  TurnVoiceSessionClient,
  TurnVoiceSessionClientEvent,
  TurnVoiceSessionClientEventHandler,
  TurnVoiceSessionCreateResponsePromptArgs,
  TurnVoiceSessionCustomEvent,
  TurnVoiceSessionInputTranscription,
  TurnVoiceSessionMiddleware,
  TurnVoiceSessionMiddlewareNext,
  TurnVoiceSessionMiddlewareOptions,
  TurnVoiceSessionPrepareTurnArgs,
  TurnVoiceSessionRequest,
  TurnVoiceSessionStartArgs,
  TurnVoiceSessionStartOptions,
  TurnVoiceSessionStartResult,
  TurnVoiceSessionTurnPreparation,
  TurnVoiceSessionMessage,
  CreateVoiceSessionRequest,
  CreateVoiceSessionRequestArgs,
  VoiceSessionAudioInput,
  VoiceSessionAudioOutput,
  VoiceSessionClient,
  VoiceSessionClientEvent,
  VoiceSessionClientEventHandler,
  VoiceSessionCustomEvent,
  VoiceSessionInputTranscription,
  VoiceSessionMiddleware,
  VoiceSessionMiddlewareNext,
  VoiceSessionMiddlewareOptions,
  VoiceSessionStartArgs,
  VoiceSessionStartOptions,
  VoiceSessionStartResult,
  VoiceSessionTurnEndArgs,
}

import type { IncomingMessage, ServerResponse } from 'node:http'

import type { PromptRequest, SpeechRequest, SpeechSynthesisRequest, VoiceSessionRequest } from '@ai/llm'

type CreateVoiceSessionClientOptions = {
  path?: string
}

type CreateTurnVoiceSessionClientOptions = {
  path?: string
}

type VoiceSessionClient<
  TStartOptions extends VoiceSessionStartOptions = VoiceSessionStartOptions,
  TCustomEvent extends VoiceSessionCustomEvent = never,
> = {
  start(args: VoiceSessionStartArgs<TStartOptions, TCustomEvent>): Promise<void>
  sendAudio(audio: VoiceSessionAudioInput, inputId: string): Promise<void>
  sendAudioChunk(audio: VoiceSessionAudioInput, inputId: string): Promise<void>
  endAudioTurn(inputId: string): Promise<void>
  stop(): Promise<void>
}

type VoiceSessionStartArgs<TStartOptions extends VoiceSessionStartOptions, TCustomEvent extends VoiceSessionCustomEvent> = TStartOptions & {
  onEvent: VoiceSessionClientEventHandler<TCustomEvent>
}

type VoiceSessionStartOptions = object

type VoiceSessionClientEventHandler<TCustomEvent extends VoiceSessionCustomEvent = never> = (
  event: VoiceSessionClientEvent<TCustomEvent>,
) => void

type VoiceSessionClientEvent<TCustomEvent extends VoiceSessionCustomEvent = never> =
  | {
      type: 'audio'
      audio: VoiceSessionAudioOutput
    }
  | {
      type: 'text'
      text: string
    }
  | {
      type: 'input-transcription'
      transcription: VoiceSessionInputTranscription
    }
  | {
      type: 'turn-complete'
    }
  | {
      type: 'error'
      message: string
    }
  | TCustomEvent

type VoiceSessionCustomEvent = {
  type: string
}

type VoiceSessionAudioInput = {
  data: Uint8Array
  mimeType: string
}

type VoiceSessionAudioOutput = {
  data: string
  mimeType: string
}

type VoiceSessionInputTranscription = {
  inputId?: string
  text: string
}

type VoiceSessionMiddleware = (request: IncomingMessage, response: ServerResponse, next: VoiceSessionMiddlewareNext) => Promise<void> | void

type VoiceSessionMiddlewareNext = (error?: Error) => void

type VoiceSessionMiddlewareOptions<
  TStartOptions extends VoiceSessionStartOptions = VoiceSessionStartOptions,
  TCustomEvent extends VoiceSessionCustomEvent = never,
> = {
  createSessionRequest(
    args: CreateVoiceSessionRequestArgs,
  ): CreateVoiceSessionRequest<TStartOptions> | Promise<CreateVoiceSessionRequest<TStartOptions> | undefined> | undefined
  inputTranscriptionWaitMs?: number
  invalidStartRequestMessage?: string
  onAudioTurnEnd?(
    args: VoiceSessionTurnEndArgs<TStartOptions>,
  ):
    | Promise<VoiceSessionClientEvent<TCustomEvent> | VoiceSessionClientEvent<TCustomEvent>[] | undefined>
    | VoiceSessionClientEvent<TCustomEvent>
    | VoiceSessionClientEvent<TCustomEvent>[]
    | undefined
  path?: string
}

type CreateVoiceSessionRequestArgs = {
  body: unknown
}

type CreateVoiceSessionRequest<TStartOptions extends VoiceSessionStartOptions = VoiceSessionStartOptions> = {
  options: TStartOptions
  request: VoiceSessionStartResult
}

type VoiceSessionStartResult = Omit<VoiceSessionRequest, 'onEvent'>

type VoiceSessionTurnEndArgs<TStartOptions extends VoiceSessionStartOptions = VoiceSessionStartOptions> = {
  inputId?: string
  inputTranscription?: string
  options: TStartOptions
  previousModelText?: string
}

type TurnVoiceSessionClient<
  TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent = never,
> = {
  start(args: TurnVoiceSessionStartArgs<TStartOptions, TCustomEvent>): Promise<void>
  sendAudioTurn(audio: TurnVoiceSessionAudioInput, inputId: string): Promise<void>
  stop(): Promise<void>
}

type TurnVoiceSessionStartArgs<
  TStartOptions extends TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent,
> = TStartOptions & {
  onEvent: TurnVoiceSessionClientEventHandler<TCustomEvent>
}

type TurnVoiceSessionStartOptions = object

type TurnVoiceSessionClientEventHandler<TCustomEvent extends TurnVoiceSessionCustomEvent = never> = (
  event: TurnVoiceSessionClientEvent<TCustomEvent>,
) => void

type TurnVoiceSessionClientEvent<TCustomEvent extends TurnVoiceSessionCustomEvent = never> = VoiceSessionClientEvent<TCustomEvent>

type TurnVoiceSessionCustomEvent = VoiceSessionCustomEvent

type TurnVoiceSessionAudioInput = VoiceSessionAudioInput

type TurnVoiceSessionAudioOutput = VoiceSessionAudioOutput

type TurnVoiceSessionInputTranscription = VoiceSessionInputTranscription

type TurnVoiceSessionMiddleware = VoiceSessionMiddleware

type TurnVoiceSessionMiddlewareNext = VoiceSessionMiddlewareNext

type TurnVoiceSessionMiddlewareOptions<
  TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions,
  TCustomEvent extends TurnVoiceSessionCustomEvent = never,
> = {
  createResponsePrompt(args: TurnVoiceSessionCreateResponsePromptArgs<TStartOptions>): Promise<string> | string
  createSessionRequest(
    args: CreateTurnVoiceSessionRequestArgs,
  ): CreateTurnVoiceSessionRequest<TStartOptions> | Promise<CreateTurnVoiceSessionRequest<TStartOptions> | undefined> | undefined
  invalidStartRequestMessage?: string
  path?: string
  prepareTurn?(
    args: TurnVoiceSessionPrepareTurnArgs<TStartOptions>,
  ): Promise<TurnVoiceSessionTurnPreparation<TCustomEvent> | undefined> | TurnVoiceSessionTurnPreparation<TCustomEvent> | undefined
}

type CreateTurnVoiceSessionRequestArgs = {
  body: unknown
}

type CreateTurnVoiceSessionRequest<TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions> = {
  options: TStartOptions
  request: TurnVoiceSessionStartResult
}

type TurnVoiceSessionStartResult = TurnVoiceSessionRequest

type TurnVoiceSessionRequest = {
  response: Omit<PromptRequest, 'prompt'>
  synthesis: Omit<SpeechSynthesisRequest, 'text'>
  transcription: Omit<SpeechRequest, 'audio'>
}

type TurnVoiceSessionPrepareTurnArgs<TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions> = {
  conversation: readonly TurnVoiceSessionMessage[]
  inputId?: string
  options: TStartOptions
  previousModelText?: string
  transcription: string
}

type TurnVoiceSessionCreateResponsePromptArgs<TStartOptions extends TurnVoiceSessionStartOptions = TurnVoiceSessionStartOptions> = {
  conversation: readonly TurnVoiceSessionMessage[]
  inputId?: string
  options: TStartOptions
  previousModelText?: string
  responseInstructions?: string
  transcription: string
}

type TurnVoiceSessionTurnPreparation<TCustomEvent extends TurnVoiceSessionCustomEvent = never> = {
  events?: TurnVoiceSessionClientEvent<TCustomEvent> | TurnVoiceSessionClientEvent<TCustomEvent>[]
  responseInstructions?: string
}

type TurnVoiceSessionMessage =
  | {
      inputId?: string
      role: 'learner'
      text: string
    }
  | {
      role: 'model'
      text: string
    }
