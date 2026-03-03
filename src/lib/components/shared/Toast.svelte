<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { toastState, type ToastLevel } from '../../stores/toast.svelte';

  const levelColor: Record<ToastLevel, string> = {
    success: 'var(--color-success)',
    error: 'var(--color-error)',
    warning: 'var(--color-warning)',
    info: 'var(--color-info)',
  };
</script>

<div class="toast-container" aria-live="polite" aria-label="Notifications">
  {#each toastState.items as toast (toast.id)}
    <div
      class="toast-item toast-{toast.level}"
      role="status"
      style="--toast-accent: {levelColor[toast.level]};"
    >
      <span class="toast-accent-bar"></span>
      <span class="toast-text">{toast.text}</span>
      <button
        class="toast-dismiss"
        aria-label="Dismiss notification"
        onclick={() => toastState.dismiss(toast.id)}
      >
        ✕
      </button>
    </div>
  {/each}
</div>

<style>
  .toast-container {
    position: fixed;
    top: var(--spacing-6);
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--spacing-2);
    z-index: 2000;
    pointer-events: none;
  }

  .toast-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    padding: var(--spacing-3) var(--spacing-4);
    min-width: 260px;
    max-width: 420px;
    pointer-events: auto;
  }

  .toast-accent-bar {
    flex-shrink: 0;
    width: 4px;
    height: 100%;
    min-height: 20px;
    border-radius: var(--radius-sm);
    background: var(--toast-accent);
    align-self: stretch;
  }

  .toast-text {
    flex: 1;
    font-size: var(--font-size-sm);
    color: var(--color-text);
    line-height: var(--line-height);
  }

  .toast-dismiss {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
  }

  .toast-dismiss:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
