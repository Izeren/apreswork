// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import { WEEK_START_STORAGE_KEY, loadWeekStart, saveWeekStart } from './weekStartPref';
import { memoryStorage, throwingReadStorage, throwingWriteStorage } from './storageDoubles';

describe('loadWeekStart', () => {
  it.each([
    { label: 'nothing stored', stored: null as string | null, expected: 'mon' as const },
    { label: "'sun' stored", stored: 'sun', expected: 'sun' as const },
    { label: "'mon' stored", stored: 'mon', expected: 'mon' as const },
  ])('returns $expected when $label', ({ stored, expected }) => {
    const s = memoryStorage();
    if (stored !== null) s.setItem(WEEK_START_STORAGE_KEY, stored);
    expect(loadWeekStart(s)).toBe(expected);
  });

  // localStorage is user-editable — validate strictly (plain token, not JSON).
  it.each([
    { name: 'wrong token', raw: 'monday' },
    { name: 'JSON-encoded sun', raw: '"sun"' },
    { name: 'JSON-encoded mon', raw: '"mon"' },
    { name: 'numeric', raw: '0' },
    { name: 'empty string', raw: '' },
    { name: 'boolean', raw: 'true' },
    { name: 'object JSON', raw: '{"weekStart":"sun"}' },
  ])('falls back to mon for invalid stored value ($name)', ({ raw }) => {
    const s = memoryStorage();
    s.setItem(WEEK_START_STORAGE_KEY, raw);
    expect(loadWeekStart(s)).toBe('mon');
  });

  it('falls back to mon when storage read throws', () => {
    expect(loadWeekStart(throwingReadStorage)).toBe('mon');
  });
});

describe('saveWeekStart', () => {
  it.each([{ value: 'mon' as const }, { value: 'sun' as const }])(
    'round-trips $value',
    ({ value }) => {
      const s = memoryStorage();
      saveWeekStart(value, s);
      expect(loadWeekStart(s)).toBe(value);
    },
  );

  it('stores the raw token (not JSON)', () => {
    const s = memoryStorage();
    saveWeekStart('sun', s);
    expect(s.getItem(WEEK_START_STORAGE_KEY)).toBe('sun');
  });

  it('swallows storage write failures', () => {
    expect(() => saveWeekStart('sun', throwingWriteStorage)).not.toThrow();
  });
});
