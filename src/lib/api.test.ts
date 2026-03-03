// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import { apiErrorMessage, syncErrorMessage, backupErrorMessage } from './api';

type ErrorHelper = (e: unknown, fallback: string) => string;

/** Shared "falls back for unusable input" cases for all three error-message helpers. */
function unusableErrorCases(errorKind: string): Array<[string, unknown]> {
  return [
    ['undefined', undefined],
    ['null', null],
    ['a string', 'boom'],
    ['an Error instance', new Error('boom')],
    ['missing message', { error: errorKind }],
    ['missing error', { message: 'hi' }],
    ['non-string message', { error: errorKind, message: 42 }],
    ['empty message', { error: errorKind, message: '' }],
  ];
}

const errorHelpers: Array<{
  name: string;
  fn: ErrorHelper;
  unusableKind: string;
  surfacedCases: Array<{ kind: string; message: string }>;
}> = [
  {
    name: 'apiErrorMessage',
    fn: apiErrorMessage,
    unusableKind: 'validation',
    surfacedCases: [{ kind: 'validation', message: 'deadline cannot be in the past' }],
  },
  {
    name: 'syncErrorMessage',
    fn: syncErrorMessage,
    unusableKind: 'calendar_sync',
    surfacedCases: [
      { kind: 'validation', message: 'deadline cannot be in the past' },
      { kind: 'calendar_sync', message: 'Calendar sync error: network error' },
    ],
  },
  {
    name: 'backupErrorMessage',
    fn: backupErrorMessage,
    unusableKind: 'backup',
    surfacedCases: [
      { kind: 'validation', message: 'deadline cannot be in the past' },
      { kind: 'backup', message: 'backup was written by a newer app' },
    ],
  },
];

describe.each(errorHelpers)('$name', ({ fn, unusableKind, surfacedCases }) => {
  const fallback = 'Something failed';

  it.each(surfacedCases)('surfaces the backend message for $kind errors', ({ kind, message }) => {
    expect(fn({ error: kind, message }, fallback)).toBe(message);
  });

  it('falls back for non-surfaced error kinds', () => {
    expect(fn({ error: 'internal', message: 'db is on fire' }, fallback)).toBe(fallback);
  });

  it.each(unusableErrorCases(unusableKind))('falls back when the error is %s', (_label, e) => {
    expect(fn(e, fallback)).toBe(fallback);
  });
});

describe('error-kind cross-leak prevention', () => {
  const fallback = 'fallback';

  it.each([
    {
      fnName: 'apiErrorMessage',
      fn: apiErrorMessage as ErrorHelper,
      error: 'calendar_sync',
      message: 'network error',
    },
    {
      fnName: 'apiErrorMessage',
      fn: apiErrorMessage as ErrorHelper,
      error: 'backup',
      message: 'archive corrupted',
    },
    {
      fnName: 'syncErrorMessage',
      fn: syncErrorMessage as ErrorHelper,
      error: 'backup',
      message: 'archive corrupted',
    },
    {
      fnName: 'backupErrorMessage',
      fn: backupErrorMessage as ErrorHelper,
      error: 'calendar_sync',
      message: 'network error',
    },
  ])('$fnName does not surface "$error"', ({ fn, error, message }) => {
    expect(fn({ error, message }, fallback)).toBe(fallback);
  });
});
