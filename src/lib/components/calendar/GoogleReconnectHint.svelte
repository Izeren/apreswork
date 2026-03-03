<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  interface Props {
    visible: boolean;
    onreconnect: () => void;
  }

  const { visible, onreconnect }: Props = $props();
</script>

{#if visible}
  <div class="reconnect-hint" role="status" aria-live="polite">
    <span class="reconnect-hint__text">
      Google Calendar is disconnected. Events shown below may be stale.
    </span>
    <button class="reconnect-hint__btn" onclick={onreconnect}>Open Settings</button>
  </div>
{/if}

<style>
  .reconnect-hint {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    padding: var(--spacing-2) var(--spacing-4);
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 50%, transparent);
    border-radius: var(--radius-sm);
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
  }

  .reconnect-hint__text {
    flex: 1;
  }

  .reconnect-hint__btn {
    flex-shrink: 0;
    padding: var(--spacing-1) var(--spacing-2);
    border: 1px solid currentColor;
    border-radius: var(--radius-sm);
    background: transparent;
    cursor: pointer;
    font-size: inherit;
    color: inherit;
  }

  .reconnect-hint__btn:hover {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
</style>
