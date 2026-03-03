<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts" generics="T extends string">
  interface Props {
    /** Summary prefix and aria group label, e.g. "Status" → "Status: All". */
    label: string;
    /** Every selectable value, in display order. */
    options: readonly T[];
    /** Current selection; empty means "All" (no constraint). */
    selected: T[];
    onchange: (selected: T[]) => void;
  }

  const { label, options, selected, onchange }: Props = $props();

  let open = $state(false);
  let rootEl: HTMLElement | null = $state(null);
  let buttonEl: HTMLButtonElement | null = $state(null);

  const optionLabel = (value: T) => value.charAt(0).toUpperCase() + value.slice(1);

  const summary = $derived.by(() => {
    if (selected.length === 0) return `${label}: All`;
    if (selected.length === 1) return `${label}: ${optionLabel(selected[0])}`;
    return `${label}: ${selected.length}`;
  });

  function toggle(value: T) {
    onchange(selected.includes(value) ? selected.filter((v) => v !== value) : [...selected, value]);
  }

  // Same dismiss idiom as ContextMenu: capture-phase pointerdown outside
  // closes, so a press a child swallows (stopPropagation) can't leak one.
  $effect(() => {
    if (!open) return;
    const handleOutsidePointerDown = (event: PointerEvent) => {
      if (rootEl && !rootEl.contains(event.target as Node)) open = false;
    };
    document.addEventListener('pointerdown', handleOutsidePointerDown, true);
    return () => document.removeEventListener('pointerdown', handleOutsidePointerDown, true);
  });

  function handleWindowKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') {
      event.preventDefault();
      open = false;
      buttonEl?.focus();
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<div class="filter-dropdown" bind:this={rootEl}>
  <button
    bind:this={buttonEl}
    class="filter-dropdown-toggle"
    aria-expanded={open}
    aria-haspopup="true"
    onclick={() => (open = !open)}
  >
    {summary}
  </button>
  {#if open}
    <div class="filter-popover" role="group" aria-label="Filter by {label.toLowerCase()}">
      {#each options as option (option)}
        <label class="filter-option">
          <input
            type="checkbox"
            checked={selected.includes(option)}
            onchange={() => toggle(option)}
          />
          {optionLabel(option)}
        </label>
      {/each}
    </div>
  {/if}
</div>

<style>
  .filter-dropdown {
    position: relative;
  }

  /* Visually a sibling of the filter bar's other controls. */
  .filter-dropdown-toggle {
    background: var(--color-bg);
    color: var(--color-text);
    cursor: pointer;
    white-space: nowrap;
    transition: border-color var(--transition-fast);
  }

  .filter-dropdown-toggle:focus {
    outline: none;
    border-color: var(--color-primary);
  }

  .filter-dropdown-toggle:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .filter-dropdown-toggle[aria-expanded='true'] {
    border-color: var(--color-primary);
  }

  .filter-popover {
    position: absolute;
    top: calc(100% + var(--spacing-1));
    left: 0;
    z-index: 1100;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
    min-width: 160px;
    padding: var(--spacing-2);
  }

  .filter-option {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    padding: var(--spacing-1) var(--spacing-2);
    border-radius: var(--radius-sm);
    font-size: var(--font-size-sm);
    color: var(--color-text);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .filter-option:hover {
    background: var(--color-surface-hover);
  }

  .filter-option input {
    accent-color: var(--color-primary);
    cursor: pointer;
  }
</style>
