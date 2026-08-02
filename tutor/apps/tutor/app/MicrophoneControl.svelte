<script lang="ts">
  let {
    autoPauseAfterResponse,
    canControlMicrophone,
    microphoneActive,
    microphoneButtonLabel,
    microphoneStatusLabel,
    toggleAutoPauseAfterResponse,
    toggleMicrophone,
  }: MicrophoneControlProps = $props()

  type MicrophoneControlProps = {
    autoPauseAfterResponse: boolean
    canControlMicrophone: boolean
    microphoneActive: boolean
    microphoneButtonLabel: string
    microphoneStatusLabel: string
    toggleAutoPauseAfterResponse(): void
    toggleMicrophone(): Promise<void>
  }
</script>

<section class="microphone" aria-live="polite">
  <div class="microphone-primary">
    <button class:recording={microphoneActive} disabled={!canControlMicrophone} type="button" onclick={toggleMicrophone}>
      {microphoneButtonLabel}
    </button>
    <p>{microphoneStatusLabel}</p>
  </div>

  <label class="microphone-toggle">
    <input checked={autoPauseAfterResponse} type="checkbox" onchange={toggleAutoPauseAfterResponse} />
    <span>Auto pause after reply</span>
  </label>
</section>

<style>
  .microphone {
    position: sticky;
    z-index: 2;
    bottom: 18px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-top: 18px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-panel);
    background: color-mix(in srgb, var(--color-surface-elevated) 92%, transparent);
    box-shadow: var(--shadow-panel);
    padding: 16px;
    backdrop-filter: blur(18px);
  }

  .microphone-primary {
    display: flex;
    align-items: center;
    gap: 16px;
    min-width: 0;
  }

  .microphone p {
    margin: 0;
    color: var(--color-text-muted);
    font-weight: 650;
  }

  .microphone button {
    min-width: 160px;
  }

  .microphone-toggle {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--color-text-muted);
    cursor: pointer;
    font-weight: 700;
    white-space: nowrap;
  }

  .microphone-toggle input {
    width: 18px;
    height: 18px;
    accent-color: var(--color-primary);
  }

  @media (max-width: 560px) {
    .microphone {
      align-items: stretch;
      flex-direction: column;
      bottom: 12px;
    }

    .microphone-primary {
      align-items: stretch;
      flex-direction: column;
    }

    .microphone button {
      width: 100%;
    }

    .microphone-toggle {
      white-space: normal;
    }
  }
</style>
