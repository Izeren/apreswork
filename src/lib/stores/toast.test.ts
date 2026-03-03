// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

import { ToastState, type ToastLevel } from './toast.svelte';

describe('ToastState', () => {
  let toast: ToastState;

  beforeEach(() => {
    vi.useFakeTimers();
    toast = new ToastState();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('push adds item with correct level and text', () => {
    toast.push('info', 'hello');
    expect(toast.items).toHaveLength(1);
    expect(toast.items[0].level).toBe('info');
    expect(toast.items[0].text).toBe('hello');
    expect(toast.items[0].id).toBeDefined();
  });

  describe.each([
    ['success', 'success' as ToastLevel],
    ['error', 'error' as ToastLevel],
    ['info', 'info' as ToastLevel],
    ['warn', 'warning' as ToastLevel],
  ])('%s() convenience method', (method, expectedLevel) => {
    it(`sets level to "${expectedLevel}"`, () => {
      (toast[method as keyof ToastState] as (text: string) => void)('msg');
      expect(toast.items).toHaveLength(1);
      expect(toast.items[0].level).toBe(expectedLevel);
      expect(toast.items[0].text).toBe('msg');
    });
  });

  it.each([
    {
      label: 'removes correct toast by id',
      setup: (t: ToastState) => {
        t.push('info', 'first');
        t.push('info', 'second');
      },
      dismiss: (t: ToastState) => t.dismiss(t.items[0].id),
      expectedLength: 1,
      expectedFirstText: 'second',
    },
    {
      label: 'non-existent id is a no-op',
      setup: (t: ToastState) => {
        t.push('info', 'existing');
      },
      dismiss: (t: ToastState) => t.dismiss('non-existent-id'),
      expectedLength: 1,
      expectedFirstText: 'existing',
    },
  ])('dismiss: $label', ({ setup, dismiss, expectedLength, expectedFirstText }) => {
    setup(toast);
    dismiss(toast);
    expect(toast.items).toHaveLength(expectedLength);
    expect(toast.items[0].text).toBe(expectedFirstText);
  });

  it.each(['success', 'error', 'info', 'warning'] as const)(
    'auto-dismisses %s toast after the 3s default',
    (level) => {
      toast.push(level, 'msg');
      expect(toast.items).toHaveLength(1);
      vi.advanceTimersByTime(2_999);
      expect(toast.items).toHaveLength(1);
      vi.advanceTimersByTime(1);
      expect(toast.items).toHaveLength(0);
    },
  );

  it.each([
    { label: 'custom 1000ms', autoMs: 1_000, beforeMs: 999, afterMs: 1, expectedFinalLength: 0 },
    {
      label: 'autoMs=0 never dismisses',
      autoMs: 0,
      beforeMs: 60_000,
      afterMs: 0,
      expectedFinalLength: 1,
    },
  ])('$label timeout behavior', ({ autoMs, beforeMs, afterMs, expectedFinalLength }) => {
    toast.push('info', 'msg', autoMs);
    expect(toast.items).toHaveLength(1);
    vi.advanceTimersByTime(beforeMs);
    expect(toast.items).toHaveLength(1);
    vi.advanceTimersByTime(afterMs);
    expect(toast.items).toHaveLength(expectedFinalLength);
  });

  it('multiple toasts stack correctly (order preserved)', () => {
    toast.push('info', 'first');
    toast.push('warning', 'second');
    toast.push('error', 'third');
    expect(toast.items).toHaveLength(3);
    expect(toast.items[0].text).toBe('first');
    expect(toast.items[1].text).toBe('second');
    expect(toast.items[2].text).toBe('third');
  });

  it('each toast gets a unique id', () => {
    toast.push('info', 'a');
    toast.push('info', 'b');
    expect(toast.items[0].id).not.toBe(toast.items[1].id);
  });
});
