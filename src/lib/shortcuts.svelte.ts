// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

export interface ShortcutBinding {
  /** KeyboardEvent.key matched exactly, e.g. 'n', 'ArrowLeft', '?'. */
  key: string;
  /** Human-readable description shown in the help overlay. */
  description: string;
  /** Overlay grouping: 'Global' | 'Calendar' | 'Tasks' */
  group: string;
  handler: () => void;
}

// Plain array — no $state. Registrations happen synchronously inside $effect
// during flush_sync, which conflicts with Svelte 5's update tracking.
// The overlay compensates by re-snapshotting the registry on every open
// (its `groups` derived depends on the `open` prop), so live reactivity
// while the overlay stays open is not required.
let bindings: ShortcutBinding[] = [];

/**
 * Register shortcuts. Returns an unregister function that removes exactly
 * these bindings. Duplicate keys: the LAST registration wins at dispatch
 * time. In practice, calendar/tasks views are never mounted together.
 */
export function registerShortcuts(next: ShortcutBinding[]): () => void {
  bindings = [...bindings, ...next];
  return () => {
    bindings = bindings.filter((b) => !next.includes(b));
  };
}

export function activeShortcuts(): readonly ShortcutBinding[] {
  return bindings;
}

const SUPPRESSED_ROLES = ['dialog', 'alertdialog', 'menu', 'listbox'];
const SUPPRESSED_SELECTOR = SUPPRESSED_ROLES.map((r) => `[role="${r}"]`).join(', ');

/**
 * Global key dispatcher. Attach to `<svelte:window onkeydown={...} />` in
 * Shell — that is the ONE and only window listener for these bindings.
 *
 * Suppression rules (evaluated in order):
 *   1. event.defaultPrevented
 *   2. ctrlKey | metaKey | altKey (shiftKey is NOT excluded — '?' is Shift+/)
 *   3. Target is an editable element (input / textarea / select / contenteditable)
 *   4. Any element with a role in SUPPRESSED_ROLES exists in the DOM
 */
export function handleShortcutKeydown(event: KeyboardEvent): void {
  if (event.defaultPrevented) return;
  if (event.ctrlKey || event.metaKey || event.altKey) return;

  const target = event.target;
  if (target instanceof HTMLElement) {
    const tag = target.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable) {
      return;
    }
  }

  if (document.querySelector(SUPPRESSED_SELECTOR)) {
    return;
  }

  for (let i = bindings.length - 1; i >= 0; i--) {
    if (bindings[i].key === event.key) {
      event.preventDefault();
      bindings[i].handler();
      return;
    }
  }
}

export function resetShortcutsForTest(): void {
  bindings = [];
}
