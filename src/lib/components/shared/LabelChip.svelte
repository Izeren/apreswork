<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  interface Props {
    label: string;
    onremove?: () => void;
  }

  const { label, onremove }: Props = $props();
</script>

<span class="label-chip">
  <span class="chip-text">{label}</span>
  {#if onremove}
    <button class="chip-remove" aria-label="Remove {label}" onclick={onremove}>✕</button>
  {/if}
</span>

<style>
  .label-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--spacing-1);
    padding: 2px var(--spacing-2);
    border-radius: var(--radius-sm);
    background: var(--color-bg-tertiary);
    color: var(--color-text);
    font-size: var(--font-size-xs);
    line-height: 1.4;
    white-space: nowrap;
  }

  .chip-text {
    display: inline-block;
  }

  .chip-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: var(--color-text-secondary);
    font-size: 10px;
    cursor: pointer;
    line-height: 1;
    transition: background var(--transition-fast);
  }

  .chip-remove:hover {
    background: var(--color-surface-active);
    color: var(--color-text);
  }

  .chip-remove:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
