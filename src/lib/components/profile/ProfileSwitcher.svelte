<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { router } from '../../router.svelte';
  import { profileState, ProfileState } from '../../stores/profile.svelte';
  import type { ContextMenuItem } from '../../actions/taskActions';
  import ContextMenu from '../shared/ContextMenu.svelte';

  interface Props {
    store?: ProfileState;
  }

  const { store = profileState }: Props = $props();

  let open: boolean = $state(false);
  let menuX: number = $state(0);
  let menuY: number = $state(0);
  let buttonEl: HTMLButtonElement | null = $state(null);

  /** Set when the menu closes. The menu dismisses itself on any outside
   *  pointerdown — including the press half of a click on this toggle — so
   *  without a grace window that click would immediately reopen it.
   *  Deliberately a plain let, not $state: read only inside event handlers,
   *  never rendered. */
  let suppressReopenUntil = 0;

  const items: ContextMenuItem[] = $derived.by(() => [
    ...store.others.map((p) => ({
      // Direct switch — deliberately no confirmation dialog.
      label: `Switch to ${p.name}`,
      action: () => void store.switchTo(p.id),
    })),
    { label: 'Manage profiles…', action: () => router.navigate('profiles') },
  ]);

  function handleClose(): void {
    open = false;
    suppressReopenUntil = Date.now() + 200;
  }

  function handleToggle(): void {
    if (open) {
      open = false;
      return;
    }
    if (Date.now() < suppressReopenUntil) return;
    if (buttonEl !== null) {
      const rect = buttonEl.getBoundingClientRect();
      menuX = rect.left;
      menuY = rect.top;
    }
    open = true;
  }
</script>

<div class="switcher">
  <button
    bind:this={buttonEl}
    class="switcher-button"
    class:active={router.current === 'profiles'}
    aria-haspopup="menu"
    aria-expanded={open}
    disabled={store.switching}
    onclick={handleToggle}
  >
    <span class="switcher-label truncate">
      {store.switching ? 'Switching…' : (store.active?.name ?? 'Profile')}
    </span>
    <span class="switcher-caret" aria-hidden="true">▾</span>
  </button>
</div>

<ContextMenu {open} x={menuX} y={menuY} {items} onclose={handleClose} />

<style>
  .switcher {
    padding: var(--spacing-2);
    border-top: 1px solid var(--color-border-light);
  }

  .switcher-button {
    font-weight: var(--font-weight-medium);
  }

  .switcher-button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .switcher-button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .switcher-button.active {
    background: var(--color-primary-light);
    color: var(--color-primary);
    font-weight: var(--font-weight-semibold);
  }

  .switcher-label {
    min-width: 0;
  }

  .switcher-caret {
    flex-shrink: 0;
    font-size: var(--font-size-xs);
  }
</style>
