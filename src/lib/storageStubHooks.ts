// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared beforeEach/afterEach installer that wires a working in-memory
// `localStorage` onto the global for component tests. This project's jsdom
// build ships a broken localStorage (the `--localstorage-file` warning), so
// components that read/write preferences need a stub. Mirrors the
// `installCalendarHooks()` pattern in the calendar testFixtures. Kept separate
// from `storageDoubles.ts` (which is pure) so node-env unit tests never pull in
// `@testing-library/svelte`.

import { afterEach, beforeEach, vi } from 'vitest';
import { cleanup } from '@testing-library/svelte';

/**
 * Register hooks that stub `localStorage` with a fresh in-memory map per test
 * and tear the render + globals down afterwards. Pass `restoreMocks: true` to
 * use `vi.restoreAllMocks()` instead of `vi.clearAllMocks()` in teardown.
 * Tests seed/read stored values through the stubbed global `localStorage`.
 */
export function installLocalStorageStub({ restoreMocks = false }: { restoreMocks?: boolean } = {}) {
  const lsMap = new Map<string, string>();
  const stub = {
    getItem: (key: string): string | null => lsMap.get(key) ?? null,
    setItem: (key: string, value: string): void => {
      lsMap.set(key, value);
    },
    removeItem: (key: string): void => {
      lsMap.delete(key);
    },
    clear: (): void => {
      lsMap.clear();
    },
  };

  beforeEach(() => {
    lsMap.clear();
    vi.stubGlobal('localStorage', stub);
  });

  afterEach(() => {
    cleanup();
    if (restoreMocks) vi.restoreAllMocks();
    else vi.clearAllMocks();
    vi.unstubAllGlobals();
    lsMap.clear();
  });
}
