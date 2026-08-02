<script lang="ts">
  import type {
    KoreanTutorLevel,
    ModelConfiguration,
    SpeechSynthesisModel,
    TextModel,
    TranscriptionModel,
  } from '#tutor/app/generated/api.ts'
  import { speechSynthesisModels, textModels, transcriptionModels } from '#tutor/app/models.ts'

  const tutorLevels: KoreanTutorLevel[] = ['A1', 'A2']
  const modelLabels = {
    'gemini-3.1-flash-lite': 'Gemini Flash-Lite',
    'gemini-3.5-flash': 'Gemini Flash',
    'gpt-4o-mini-transcribe': 'GPT-4o Mini',
    'gpt-4o-transcribe': 'GPT-4o',
    'gpt-5.5': 'GPT-5.5',
    'tts-1': 'OpenAI TTS',
  } satisfies Record<TextModel | TranscriptionModel | SpeechSynthesisModel, string>
  const modelProfiles = [
    {
      id: 'reliable',
      label: 'Reliable',
      modelConfiguration: {
        mistakeDetection: 'gemini-3.1-flash-lite',
        reply: 'gemini-3.1-flash-lite',
        speechSynthesis: 'tts-1',
        transcription: 'gpt-4o-mini-transcribe',
      },
    },
    {
      id: 'sharper',
      label: 'Sharper',
      modelConfiguration: {
        mistakeDetection: 'gemini-3.1-flash-lite',
        reply: 'gemini-3.5-flash',
        speechSynthesis: 'tts-1',
        transcription: 'gpt-4o-mini-transcribe',
      },
    },
    {
      id: 'gpt-reply',
      label: 'GPT reply',
      modelConfiguration: {
        mistakeDetection: 'gemini-3.1-flash-lite',
        reply: 'gpt-5.5',
        speechSynthesis: 'tts-1',
        transcription: 'gpt-4o-mini-transcribe',
      },
    },
  ] satisfies TutorModelProfile[]

  let {
    autoTurnSilenceMs = $bindable(),
    modelConfiguration = $bindable(),
    playbackSpeed = $bindable(),
    sessionBusy,
    sessionSettingDisabled,
    tutorLevel = $bindable(),
  }: TutorSettingsProps = $props()

  const activeModelProfileId = $derived(
    modelProfiles.find(profile =>
      hasSameModelConfiguration({
        left: modelConfiguration,
        right: profile.modelConfiguration,
      }),
    )?.id ?? 'custom',
  )
  const autoTurnSilenceLabel = $derived(`${(autoTurnSilenceMs / 1000).toFixed(1)}s`)
  const modelConfigurationSummary = $derived(
    `${activeModelProfileId === 'custom' ? 'Custom' : 'Preset'} · ${modelLabels[modelConfiguration.reply]} reply`,
  )
  const playbackSpeedLabel = $derived(`${playbackSpeed.toFixed(2)}x`)

  function applyModelProfile(args: { modelConfiguration: ModelConfiguration }): void {
    modelConfiguration = { ...args.modelConfiguration }
  }

  function updateModelConfiguration(args: Partial<ModelConfiguration>): void {
    modelConfiguration = {
      ...modelConfiguration,
      ...args,
    }
  }

  function hasSameModelConfiguration(args: {
    left: ModelConfiguration
    right: ModelConfiguration
  }): boolean {
    return (
      args.left.mistakeDetection === args.right.mistakeDetection &&
      args.left.reply === args.right.reply &&
      args.left.speechSynthesis === args.right.speechSynthesis &&
      args.left.transcription === args.right.transcription
    )
  }

  type TutorSettingsProps = {
    autoTurnSilenceMs: number
    modelConfiguration: ModelConfiguration
    playbackSpeed: number
    sessionBusy: boolean
    sessionSettingDisabled: boolean
    tutorLevel: KoreanTutorLevel
  }

  type TutorModelProfile = {
    id: string
    label: string
    modelConfiguration: ModelConfiguration
  }
</script>

<section class="settings" aria-label="Voice settings">
  <div class="setting-row">
    <p id="model-profile-label">Model set</p>
    <div class="choice-options profile-options" role="radiogroup" aria-labelledby="model-profile-label">
      {#each modelProfiles as profile}
        <label class:active={activeModelProfileId === profile.id}>
          <input
            checked={activeModelProfileId === profile.id}
            disabled={sessionSettingDisabled}
            name="model-profile"
            onchange={() => applyModelProfile({ modelConfiguration: profile.modelConfiguration })}
            type="radio"
          />
          <span>{profile.label}</span>
        </label>
      {/each}
    </div>
  </div>

  <div class="setting-row">
    <p id="tutor-level-label">Level</p>
    <div class="choice-options level-options" role="radiogroup" aria-labelledby="tutor-level-label">
      {#each tutorLevels as level}
        <label class:active={tutorLevel === level}>
          <input bind:group={tutorLevel} disabled={sessionSettingDisabled} name="tutor-level" type="radio" value={level} />
          <span>{level}</span>
        </label>
      {/each}
    </div>
  </div>

  <details class="model-routing">
    <summary>
      <span>Model routing</span>
      <span>{modelConfigurationSummary}</span>
    </summary>

    <div class="model-routing-grid">
      <label>
        <span>Mistakes</span>
        <select
          disabled={sessionSettingDisabled}
          onchange={event => updateModelConfiguration({ mistakeDetection: event.currentTarget.value as TextModel })}
          value={modelConfiguration.mistakeDetection}
        >
          {#each textModels as model}
            <option value={model}>{modelLabels[model]}</option>
          {/each}
        </select>
      </label>

      <label>
        <span>Reply</span>
        <select
          disabled={sessionSettingDisabled}
          onchange={event => updateModelConfiguration({ reply: event.currentTarget.value as TextModel })}
          value={modelConfiguration.reply}
        >
          {#each textModels as model}
            <option value={model}>{modelLabels[model]}</option>
          {/each}
        </select>
      </label>

      <label>
        <span>Speech to text</span>
        <select
          disabled={sessionSettingDisabled}
          onchange={event => updateModelConfiguration({ transcription: event.currentTarget.value as TranscriptionModel })}
          value={modelConfiguration.transcription}
        >
          {#each transcriptionModels as model}
            <option value={model}>{modelLabels[model]}</option>
          {/each}
        </select>
      </label>

      <label>
        <span>Text to speech</span>
        <select
          disabled={sessionSettingDisabled}
          onchange={event =>
            updateModelConfiguration({ speechSynthesis: event.currentTarget.value as SpeechSynthesisModel })}
          value={modelConfiguration.speechSynthesis}
        >
          {#each speechSynthesisModels as model}
            <option value={model}>{modelLabels[model]}</option>
          {/each}
        </select>
      </label>
    </div>
  </details>

  <div class="setting-row">
    <label for="playback-speed">Talking speed</label>
    <div class="slider-control">
      <input
        bind:value={playbackSpeed}
        disabled={sessionBusy}
        id="playback-speed"
        max="1.5"
        min="0.25"
        step="0.05"
        type="range"
      />
      <output for="playback-speed">{playbackSpeedLabel}</output>
    </div>
  </div>

  <div class="setting-row">
    <label for="auto-turn-silence">Turn silence</label>
    <div class="slider-control">
      <input
        bind:value={autoTurnSilenceMs}
        disabled={sessionBusy}
        id="auto-turn-silence"
        max="2500"
        min="500"
        step="100"
        type="range"
      />
      <output for="auto-turn-silence">{autoTurnSilenceLabel}</output>
    </div>
  </div>
</section>

<style>
  .settings {
    display: grid;
    gap: 14px;
    margin-top: 16px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-surface);
    box-shadow: var(--shadow-panel);
    padding: 16px;
  }

  .setting-row {
    display: grid;
    grid-template-columns: minmax(120px, auto) 1fr;
    align-items: center;
    gap: 14px 18px;
  }

  .setting-row > label,
  .setting-row > p {
    margin: 0;
    color: var(--color-text-muted);
    font-weight: 750;
  }

  .choice-options {
    display: grid;
    gap: 4px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-surface-muted);
    padding: 4px;
  }

  .profile-options {
    grid-template-columns: repeat(3, minmax(86px, 1fr));
  }

  .level-options {
    grid-template-columns: repeat(2, minmax(56px, 1fr));
  }

  .choice-options label {
    position: relative;
    display: grid;
    place-items: center;
    min-height: 38px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    color: var(--color-text-muted);
    cursor: pointer;
    font-weight: 800;
    transition:
      background 140ms ease,
      border-color 140ms ease,
      color 140ms ease;
  }

  .choice-options label.active {
    border-color: var(--color-border-strong);
    background: var(--color-control);
    color: var(--color-text);
    box-shadow: var(--shadow-control);
  }

  .choice-options label:has(input:not(:disabled)):hover {
    background: var(--color-control-hover);
    color: var(--color-text);
  }

  .choice-options label:has(input:focus-visible) {
    box-shadow: var(--focus-ring);
  }

  .choice-options input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .choice-options:has(input:disabled) label {
    cursor: not-allowed;
    opacity: 0.62;
  }

  .model-routing {
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-surface-muted);
  }

  .model-routing summary {
    display: grid;
    grid-template-columns: minmax(120px, auto) minmax(0, 1fr);
    align-items: center;
    gap: 14px 18px;
    min-height: 44px;
    cursor: pointer;
    list-style: none;
    padding: 0 14px;
  }

  .model-routing summary::-webkit-details-marker {
    display: none;
  }

  .model-routing summary span:first-child,
  .model-routing-grid label span {
    color: var(--color-text-muted);
    font-weight: 750;
  }

  .model-routing summary span:last-child {
    overflow: hidden;
    color: var(--color-text);
    font-weight: 800;
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .model-routing-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    border-top: 1px solid var(--color-border);
    padding: 14px;
  }

  .model-routing-grid label {
    display: grid;
    min-width: 0;
    gap: 6px;
  }

  .model-routing-grid select {
    width: 100%;
    min-height: 38px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-control);
    color: var(--color-text);
    font: inherit;
    font-weight: 750;
    padding: 0 10px;
  }

  .model-routing-grid select:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .model-routing-grid select:disabled {
    cursor: not-allowed;
    opacity: 0.62;
  }

  .slider-control {
    display: grid;
    grid-template-columns: 1fr 58px;
    align-items: center;
    gap: 12px;
  }

  .slider-control input {
    width: 100%;
    accent-color: var(--color-primary);
  }

  .slider-control output {
    color: var(--color-text);
    font-weight: 800;
    text-align: right;
  }

  @media (max-width: 560px) {
    .setting-row,
    .model-routing summary,
    .model-routing-grid {
      grid-template-columns: 1fr;
    }

    .model-routing summary {
      align-items: start;
      padding: 12px 14px;
    }

    .model-routing summary span:last-child {
      text-align: left;
    }
  }
</style>
