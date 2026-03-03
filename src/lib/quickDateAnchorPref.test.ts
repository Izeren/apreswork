// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import {
  QUICK_DATE_ANCHOR_STORAGE_KEY,
  loadQuickDateAnchor,
  saveQuickDateAnchor,
} from './quickDateAnchorPref';
import { memoryStorage, throwingReadStorage, throwingWriteStorage } from './storageDoubles';

describe('loadQuickDateAnchor', () => {
  it.each([
    { label: 'key is missing', getStorage: () => memoryStorage() },
    { label: 'storage read throws', getStorage: () => throwingReadStorage },
  ])("defaults to 'auto' when $label", ({ getStorage }) => {
    expect(loadQuickDateAnchor(getStorage())).toBe('auto');
  });

  it.each(['auto', 'fri', 'sat', 'sun'] as const)('round-trips %s', (anchor) => {
    const s = memoryStorage();
    s.setItem(QUICK_DATE_ANCHOR_STORAGE_KEY, anchor);
    expect(loadQuickDateAnchor(s)).toBe(anchor);
  });

  it.each(['mon', 'FRI', 'friday', '', '3', '{"anchor":"fri"}', 'null'])(
    "falls back to 'auto' on junk value %j",
    (raw) => {
      const s = memoryStorage();
      s.setItem(QUICK_DATE_ANCHOR_STORAGE_KEY, raw);
      expect(loadQuickDateAnchor(s)).toBe('auto');
    },
  );
});

describe('saveQuickDateAnchor', () => {
  it('persists the anchor for a later load', () => {
    const s = memoryStorage();
    saveQuickDateAnchor('fri', s);
    expect(loadQuickDateAnchor(s)).toBe('fri');
  });

  it('does not throw when storage write fails', () => {
    expect(() => saveQuickDateAnchor('sun', throwingWriteStorage)).not.toThrow();
  });
});
