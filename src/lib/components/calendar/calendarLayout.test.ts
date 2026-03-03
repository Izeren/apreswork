// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import { pastOverlayHeightPx, HOUR_HEIGHT_PX } from './calendarLayout';

// All dates are constructed as new Date(y, m, d, h, min) — local time — so
// results are TZ-independent regardless of the runner's offset.

describe('pastOverlayHeightPx', () => {
  it.each([
    {
      label: 'yesterday → full 24-hour height',
      day: new Date(2026, 2, 24),
      now: new Date(2026, 2, 25, 10, 30),
      expected: 24 * HOUR_HEIGHT_PX,
    },
    {
      label: 'two days ago → full 24-hour height',
      day: new Date(2026, 2, 23),
      now: new Date(2026, 2, 25, 10, 30),
      expected: 24 * HOUR_HEIGHT_PX,
    },
    {
      label: 'tomorrow → 0',
      day: new Date(2026, 2, 26),
      now: new Date(2026, 2, 25, 10, 30),
      expected: 0,
    },
    {
      label: 'next week → 0',
      day: new Date(2026, 3, 1),
      now: new Date(2026, 2, 25, 10, 30),
      expected: 0,
    },
    {
      label: 'today at exact midnight (00:00) → 0',
      day: new Date(2026, 2, 25),
      now: new Date(2026, 2, 25, 0, 0),
      expected: 0,
    },
    {
      label: 'today at 10:30 → 630',
      day: new Date(2026, 2, 25),
      now: new Date(2026, 2, 25, 10, 30),
      expected: 630,
    },
    {
      label: 'today at 23:59 → ~1439',
      day: new Date(2026, 2, 25),
      now: new Date(2026, 2, 25, 23, 59),
      expected: (23 + 59 / 60) * HOUR_HEIGHT_PX,
    },
    {
      label: 'today at 1:00 → 60',
      day: new Date(2026, 2, 25),
      now: new Date(2026, 2, 25, 1, 0),
      expected: 60,
    },
  ])('$label', ({ day, now, expected }) => {
    expect(pastOverlayHeightPx(day, now)).toBeCloseTo(expected, 5);
  });
});
