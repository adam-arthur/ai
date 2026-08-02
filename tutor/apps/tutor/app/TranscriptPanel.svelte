<script lang="ts">
  import type { TranscriptMessage } from '#tutor/app/types.ts'

  let { messages }: TranscriptPanelProps = $props()

  type TranscriptPanelProps = {
    messages: TranscriptMessage[]
  }
</script>

<section class="transcript" aria-label="Conversation transcript">
  {#if messages.length === 0}
    <p class="empty">No messages yet.</p>
  {:else}
    {#each messages as message (message.id)}
      <article class:agent={message.speaker === 'agent'} class="message">
        <span class="speaker">{message.speaker}</span>
        <p>{message.text}</p>
        {#if message.speaker === 'learner' && message.mistakes?.length}
          <div class="mistakes">
            <p class="mistakes-label">Corrections</p>
            <ul>
              {#each message.mistakes as mistake}
                <li>
                  <p class="mistake-correction">
                    <span>{mistake.original}</span>
                    <span class="mistake-arrow">-&gt;</span>
                    <span>{mistake.correction}</span>
                  </p>
                  <p class="mistake-explanation">{mistake.explanation}</p>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
        <time>{message.time}</time>
      </article>
    {/each}
  {/if}
</section>

<style>
  .transcript {
    display: grid;
    gap: 14px;
    min-height: 460px;
    margin-top: 24px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-surface);
    box-shadow: var(--shadow-panel);
    padding: 18px;
    align-content: start;
  }

  .empty {
    align-self: center;
    margin: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-surface-muted);
    color: var(--color-text-muted);
    padding: 16px 18px;
    text-align: center;
  }

  .message {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 8px 12px;
    width: min(86%, 560px);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-panel);
    background: var(--color-surface-elevated);
    padding: 14px;
  }

  .message.agent {
    justify-self: end;
    border-color: var(--color-accent-border);
    background: var(--color-accent-soft);
  }

  .message:not(.agent) {
    border-color: var(--color-primary-border);
    background: var(--color-primary-soft);
  }

  .speaker {
    color: var(--color-text-muted);
    font-size: 0.78rem;
    font-weight: 800;
    text-transform: capitalize;
  }

  .message p {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--color-text);
    line-height: 1.5;
  }

  .mistakes {
    grid-column: 1 / -1;
    display: grid;
    gap: 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface-muted);
    padding: 10px 12px;
  }

  .mistakes .mistakes-label {
    color: var(--color-primary);
    font-size: 0.72rem;
    font-weight: 850;
    line-height: 1.2;
    text-transform: uppercase;
  }

  .mistakes ul {
    display: grid;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .mistakes li {
    display: grid;
    gap: 4px;
  }

  .mistake-correction {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: baseline;
    font-weight: 750;
  }

  .mistake-arrow {
    color: var(--color-text-subtle);
    font-weight: 650;
  }

  .mistake-explanation {
    color: var(--color-text-muted);
    font-size: 0.9rem;
  }

  .message time {
    color: var(--color-text-subtle);
    font-size: 0.78rem;
  }

  @media (max-width: 560px) {
    .message {
      width: 100%;
    }
  }
</style>
