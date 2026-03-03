// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';

describe('CalendarFocusState', () => {
  let CalendarFocusState: typeof import('./calendarFocus.svelte').CalendarFocusState;
  let focus: InstanceType<typeof CalendarFocusState>;

  beforeEach(async () => {
    const mod = await import('./calendarFocus.svelte');
    CalendarFocusState = mod.CalendarFocusState;
    focus = new CalendarFocusState();
  });

  it('starts with no focused chunk', () => {
    expect(focus.chunkId).toBeNull();
    expect(focus.startTime).toBeNull();
    expect(focus.nonce).toBe(0);
  });

  it.each([
    {
      name: 'request sets the chunk id, start time, and bumps the nonce',
      shouldClear: false,
      expectedChunkId: 'chunk-1',
      expectedStartTime: '2026-07-10T09:00:00.000Z',
      expectedNonce: 1,
    },
    {
      name: 'clear resets the chunk id and start time but keeps the nonce',
      shouldClear: true,
      expectedChunkId: null,
      expectedStartTime: null,
      expectedNonce: 1,
    },
  ])('$name', ({ shouldClear, expectedChunkId, expectedStartTime, expectedNonce }) => {
    focus.request('chunk-1', '2026-07-10T09:00:00.000Z');
    if (shouldClear) focus.clear();

    expect(focus.chunkId).toBe(expectedChunkId);
    expect(focus.startTime).toBe(expectedStartTime);
    expect(focus.nonce).toBe(expectedNonce);
  });

  it('repeated requests for the same chunk still bump the nonce', () => {
    focus.request('chunk-1', '2026-07-10T09:00:00.000Z');
    focus.request('chunk-1', '2026-07-10T09:00:00.000Z');

    expect(focus.nonce).toBe(2);
  });
});
