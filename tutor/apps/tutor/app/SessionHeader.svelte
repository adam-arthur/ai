<script lang="ts">
  let {
    sessionActive,
    sessionButtonLabel,
    sessionControlDisabled,
    sessionStatusLabel,
    toggleSession,
  }: SessionHeaderProps = $props()

  type SessionHeaderProps = {
    sessionActive: boolean
    sessionButtonLabel: string
    sessionControlDisabled: boolean
    sessionStatusLabel: string
    toggleSession(): Promise<void>
  }
</script>

<section class="session">
  <div>
    <p class="eyebrow">Voice tutor</p>
    <h1>Korean tutor</h1>
    <div class="status-row" aria-live="polite">
      <span class:live={sessionActive}></span>
      <p>{sessionStatusLabel}</p>
    </div>
  </div>

  <button class:active={sessionActive} disabled={sessionControlDisabled} type="button" onclick={toggleSession}>
    {sessionButtonLabel}
  </button>
</section>

<style>
  .session {
    display: flex;
    align-items: end;
    justify-content: space-between;
    gap: 20px;
    padding: 4px 0 22px;
    border-bottom: 1px solid var(--color-border);
  }

  .eyebrow {
    margin: 0 0 6px;
    color: var(--color-primary);
    font-size: 0.78rem;
    font-weight: 800;
    letter-spacing: 0;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    color: var(--color-text);
    font-size: clamp(2.25rem, 7vw, 3.75rem);
    line-height: 1;
  }

  .status-row {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-height: 34px;
    margin-top: 16px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface-muted);
    color: var(--color-text-muted);
    padding: 0 12px;
  }

  .status-row span {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--color-text-subtle);
  }

  .status-row span.live {
    background: var(--color-success);
    box-shadow: 0 0 18px var(--color-success);
  }

  .status-row p {
    margin: 0;
    font-weight: 650;
  }

  @media (max-width: 560px) {
    .session {
      align-items: stretch;
      flex-direction: column;
    }

    .status-row {
      width: 100%;
    }
  }
</style>
