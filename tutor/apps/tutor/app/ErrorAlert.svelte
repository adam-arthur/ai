<script lang="ts">
  let { message }: ErrorAlertProps = $props()

  type ErrorAlertProps = {
    message: string
  }
</script>

<p class="error" role="alert">{message}</p>

<style>
  .error {
    margin: 16px 0 0;
    border: 1px solid var(--color-danger);
    border-radius: var(--radius-panel);
    background: var(--color-error-surface);
    color: var(--color-danger-strong);
    padding: 12px 14px;
  }
</style>
