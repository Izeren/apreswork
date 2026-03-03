// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import type { Priority, TaskStatus } from '../../types';
import type { SortKey } from './taskSort';
import {
  PRIORITY_FILTER_STORAGE_KEY,
  STATUS_FILTER_STORAGE_KEY,
  SORT_STORAGE_KEY,
  loadPriorityFilter,
  loadStatusFilter,
  savePriorityFilter,
  saveStatusFilter,
  loadSortStack,
  saveSortStack,
} from './taskListPrefs';
import { memoryStorage, throwingReadStorage, throwingWriteStorage } from '../../storageDoubles';

type PrefStorage = Pick<Storage, 'getItem' | 'setItem'>;

type FilterSuite = {
  label: string;
  load: (storage: PrefStorage) => string[];
  save: (values: string[], storage: PrefStorage) => void;
  storageKey: string;
  defaultValue: string[];
  roundTripCases: Array<{ name: string; values: string[] }>;
  invalidCases: Array<{ name: string; raw: string }>;
  dedupRaw: string;
  dedupExpected: string[];
  writeThrowSample: string[];
};

const filterSuites: FilterSuite[] = [
  {
    label: 'status filter persistence',
    load: (s) => loadStatusFilter(s),
    save: (v, s) => saveStatusFilter(v as TaskStatus[], s),
    storageKey: STATUS_FILTER_STORAGE_KEY,
    defaultValue: ['scheduled'],
    roundTripCases: [
      { name: 'multi-status selection', values: ['backlog', 'pending'] },
      { name: 'single status', values: ['completed'] },
      { name: 'empty selection (All)', values: [] },
      {
        name: 'all five statuses',
        values: ['backlog', 'pending', 'scheduled', 'completed', 'cancelled'],
      },
    ],
    invalidCases: [
      { name: 'non-JSON garbage', raw: 'not json' },
      { name: 'non-array JSON', raw: '{"statuses":["scheduled"]}' },
      { name: 'string literal', raw: '"scheduled"' },
      { name: 'null literal', raw: 'null' },
      { name: 'unknown status', raw: '["scheduled","evil"]' },
      { name: 'wrong case', raw: '["Scheduled"]' },
      { name: 'non-string element', raw: '[42]' },
    ],
    dedupRaw: '["pending","scheduled","pending"]',
    dedupExpected: ['pending', 'scheduled'],
    writeThrowSample: ['scheduled'],
  },
  {
    label: 'priority filter persistence',
    load: (s) => loadPriorityFilter(s),
    save: (v, s) => savePriorityFilter(v as Priority[], s),
    storageKey: PRIORITY_FILTER_STORAGE_KEY,
    defaultValue: [],
    roundTripCases: [
      { name: 'multi-priority selection', values: ['High', 'Critical'] },
      { name: 'single priority', values: ['Low'] },
      { name: 'empty selection (All)', values: [] },
      { name: 'all four priorities', values: ['Critical', 'High', 'Medium', 'Low'] },
    ],
    invalidCases: [
      { name: 'non-JSON garbage', raw: 'not json' },
      { name: 'non-array JSON', raw: '{"priorities":["High"]}' },
      { name: 'unknown priority', raw: '["High","Urgent"]' },
      { name: 'wrong case', raw: '["high"]' },
      { name: 'non-string element', raw: '[3]' },
    ],
    dedupRaw: '["High","Critical","High"]',
    dedupExpected: ['High', 'Critical'],
    writeThrowSample: ['High'],
  },
];

describe.each(filterSuites)(
  '$label',
  ({
    load,
    save,
    storageKey,
    defaultValue,
    roundTripCases,
    invalidCases,
    dedupRaw,
    dedupExpected,
    writeThrowSample,
  }) => {
    it.each(roundTripCases)('round-trips a saved $name', ({ values }) => {
      const storage = memoryStorage();
      save(values, storage);
      expect(load(storage)).toEqual(values);
    });

    it('defaults to the expected value when nothing is stored', () => {
      expect(load(memoryStorage())).toEqual(defaultValue);
    });

    // localStorage is user-editable — treat it as untrusted input.
    it.each(invalidCases)(
      'falls back to the default on invalid stored value ($name)',
      ({ raw }) => {
        const storage = memoryStorage();
        storage.setItem(storageKey, raw);
        expect(load(storage)).toEqual(defaultValue);
      },
    );

    it('deduplicates repeated values, preserving first-seen order', () => {
      const storage = memoryStorage();
      storage.setItem(storageKey, dedupRaw);
      expect(load(storage)).toEqual(dedupExpected);
    });

    it('falls back to the default when storage read throws', () => {
      expect(load(throwingReadStorage)).toEqual(defaultValue);
    });

    it('swallows storage write failures', () => {
      expect(() => save(writeThrowSample, throwingWriteStorage)).not.toThrow();
    });
  },
);

describe('sort stack persistence', () => {
  it.each([
    { name: 'single key', stack: [{ field: 'deadline', direction: 'desc' }] as SortKey[] },
    {
      name: 'two keys',
      stack: [
        { field: 'priority', direction: 'desc' },
        { field: 'deadline', direction: 'asc' },
      ] as SortKey[],
    },
    {
      name: 'all five fields',
      stack: [
        { field: 'logged', direction: 'desc' },
        { field: 'title', direction: 'asc' },
        { field: 'status', direction: 'asc' },
        { field: 'priority', direction: 'desc' },
        { field: 'deadline', direction: 'asc' },
      ] as SortKey[],
    },
  ])('round-trips a saved $name stack', ({ stack }) => {
    const storage = memoryStorage();
    saveSortStack(stack, storage);
    expect(loadSortStack(storage)).toEqual(stack);
  });

  it('returns null when nothing is stored', () => {
    expect(loadSortStack(memoryStorage())).toBeNull();
  });

  // localStorage is user-editable — treat it as untrusted input. The pre-stack
  // single-key {field, direction} shape is invalid on purpose: it fails once
  // and the caller resets to the default stack.
  it.each([
    { name: 'non-JSON garbage', raw: 'not json' },
    { name: 'pre-stack single-key object', raw: '{"field":"logged","direction":"desc"}' },
    { name: 'empty array', raw: '[]' },
    { name: 'unknown field', raw: '[{"field":"evil","direction":"asc"}]' },
    { name: 'unknown direction', raw: '[{"field":"priority","direction":"sideways"}]' },
    { name: 'non-object entry', raw: '["priority"]' },
    { name: 'missing direction', raw: '[{"field":"priority"}]' },
    {
      name: 'duplicate field',
      raw: '[{"field":"title","direction":"asc"},{"field":"title","direction":"desc"}]',
    },
    { name: 'null literal', raw: 'null' },
    { name: 'null entry', raw: '[null]' },
  ])('rejects invalid stored value ($name)', ({ raw }) => {
    const storage = memoryStorage();
    storage.setItem(SORT_STORAGE_KEY, raw);
    expect(loadSortStack(storage)).toBeNull();
  });

  it('returns null when storage read throws', () => {
    expect(loadSortStack(throwingReadStorage)).toBeNull();
  });

  it('swallows storage write failures', () => {
    expect(() =>
      saveSortStack([{ field: 'title', direction: 'asc' }], throwingWriteStorage),
    ).not.toThrow();
  });
});
