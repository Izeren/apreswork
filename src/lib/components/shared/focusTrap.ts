// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/** CSS selector for all potentially focusable elements. */
const FOCUSABLE_SELECTORS = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
  'details > summary',
].join(', ');

/**
 * Returns all focusable elements within a container, in DOM order.
 * The selector excludes disabled and negative-tabindex elements; the tabIndex
 * guard catches programmatic changes not yet reflected in DOM attributes.
 * Visibility filtering is the caller's responsibility.
 */
export function getFocusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTORS)).filter(
    (el) => el.tabIndex !== -1,
  );
}

/**
 * Handles a Tab or Shift+Tab keydown event to keep focus cycling within
 * the provided container. Returns true if the event was handled (and
 * preventDefault was called), false otherwise.
 */
export function handleTabTrap(event: KeyboardEvent, container: HTMLElement): boolean {
  if (event.key !== 'Tab') return false;

  const focusable = getFocusableElements(container);
  if (focusable.length === 0) return false;

  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement as HTMLElement | null;

  const boundary = event.shiftKey ? first : last;
  const target = event.shiftKey ? last : first;
  if (active === boundary) {
    event.preventDefault();
    target.focus();
    return true;
  }

  return false;
}

/**
 * Focuses the first focusable element inside the container.
 * If none exist, focuses the container itself if it can receive focus.
 */
export function focusFirst(container: HTMLElement): void {
  const focusable = getFocusableElements(container);
  if (focusable.length > 0) {
    focusable[0].focus();
  } else if (container.tabIndex >= 0) {
    container.focus();
  }
}
