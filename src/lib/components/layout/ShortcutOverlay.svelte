<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import Modal from '../shared/Modal.svelte';
  import { activeShortcuts } from '../../shortcuts.svelte';

  interface Props {
    open: boolean;
    onclose: () => void;
  }

  const { open, onclose }: Props = $props();

  /** Pretty-print map for special keys shown as symbols in the key chip. */
  const KEY_DISPLAY: Record<string, string> = {
    ArrowLeft: '←',
    ArrowRight: '→',
    ArrowUp: '↑',
    ArrowDown: '↓',
  };

  function displayKey(key: string): string {
    return KEY_DISPLAY[key] ?? (key.length === 1 ? key.toUpperCase() : key);
  }

  /**
   * Build ordered groups from the current registry. Insertion order of groups
   * is preserved (first binding that mentions a group establishes its position).
   * Uses a plain object accumulator to avoid the SvelteMap requirement.
   *
   * The registry itself is non-reactive, so `open` is the derived's only
   * dependency: each open recomputes a fresh snapshot (view switches between
   * opens change the registered groups).
   */
  const groups = $derived.by(() => {
    if (!open) return [];
    const order: string[] = [];
    const index: Record<string, { key: string; description: string }[]> = {};
    for (const binding of activeShortcuts()) {
      if (!index[binding.group]) {
        order.push(binding.group);
        index[binding.group] = [];
      }
      index[binding.group].push({ key: binding.key, description: binding.description });
    }
    return order.map((group) => ({ group, items: index[group] }));
  });
</script>

<Modal {open} title="Keyboard shortcuts" {onclose}>
  {#if groups.length === 0}
    <p class="empty">No shortcuts registered.</p>
  {:else}
    <div class="shortcuts">
      {#each groups as { group, items } (group)}
        <section class="group">
          <h3 class="group-title">{group}</h3>
          <ul class="binding-list">
            {#each items as { key, description }, i (i)}
              <li class="binding-row">
                <kbd class="key-chip">{displayKey(key)}</kbd>
                <span class="binding-desc">{description}</span>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
</Modal>

<style>
  .shortcuts {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-5);
  }

  .group-title {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: var(--spacing-2);
  }

  .binding-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .binding-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-3);
  }

  .key-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 2rem;
    padding: 2px var(--spacing-2);
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-family: inherit;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    text-align: center;
  }

  .binding-desc {
    font-size: var(--font-size-sm);
    color: var(--color-text);
  }

  .empty {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }
</style>
