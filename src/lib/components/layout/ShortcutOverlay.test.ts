// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import ShortcutOverlay from './ShortcutOverlay.svelte';
import { registerShortcuts, resetShortcutsForTest } from '../../shortcuts.svelte';

beforeEach(() => {
  resetShortcutsForTest();
  registerShortcuts([
    { key: '1', description: 'Go to calendar', group: 'Global', handler: vi.fn() },
    { key: '?', description: 'Show this help', group: 'Global', handler: vi.fn() },
  ]);
  registerShortcuts([
    { key: 'ArrowLeft', description: 'Previous day/week', group: 'Calendar', handler: vi.fn() },
  ]);
});

afterEach(() => {
  cleanup();
  resetShortcutsForTest();
});

describe('ShortcutOverlay — open state', () => {
  it.each([
    { name: 'renders both group headings', expectedTexts: ['Global', 'Calendar'] },
    {
      name: 'renders all binding descriptions',
      expectedTexts: ['Go to calendar', 'Show this help', 'Previous day/week'],
    },
    { name: 'pretty-prints ArrowLeft as ← in the key chip', expectedTexts: ['←'] },
  ])('$name when open', async ({ expectedTexts }) => {
    const { getByText } = render(ShortcutOverlay, { open: true, onclose: vi.fn() });
    await tick();
    for (const text of expectedTexts) {
      expect(getByText(text)).toBeTruthy();
    }
  });
});

describe('ShortcutOverlay — closed state', () => {
  it('renders no dialog when open=false', async () => {
    const { queryByRole } = render(ShortcutOverlay, { open: false, onclose: vi.fn() });
    await tick();
    expect(queryByRole('dialog')).toBeNull();
  });
});

describe('ShortcutOverlay — registry changes between opens', () => {
  it('re-snapshots the registry on each open (no stale groups)', async () => {
    const unregisterCalendar = registerShortcuts([
      { key: 't', description: 'Jump to today', group: 'CalendarOnly', handler: vi.fn() },
    ]);
    const { getByText, queryByText, rerender } = render(ShortcutOverlay, {
      open: true,
      onclose: vi.fn(),
    });
    await tick();
    expect(getByText('CalendarOnly')).toBeTruthy();

    // Close, swap the registered view group (simulates navigating views),
    // reopen — the overlay must show the new group and drop the old one.
    await rerender({ open: false });
    unregisterCalendar();
    registerShortcuts([
      { key: 'n', description: 'New task', group: 'TasksOnly', handler: vi.fn() },
    ]);
    await rerender({ open: true });
    await tick();

    expect(getByText('TasksOnly')).toBeTruthy();
    expect(queryByText('CalendarOnly')).toBeNull();
  });
});

describe('ShortcutOverlay — close interaction', () => {
  it('calls onclose when Escape is pressed inside the dialog', async () => {
    const onclose = vi.fn();
    const { getByRole } = render(ShortcutOverlay, { open: true, onclose });
    await tick();
    const dialog = getByRole('dialog');
    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onclose).toHaveBeenCalled();
  });
});
