// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import {
  formatDuration,
  formatShortDate,
  formatTime,
  formatDateTime,
  formatRelativeTime,
  startOfTodayMs,
  addDaysMs,
  getWeekStart,
  getWeekEnd,
  toStartOfDayISO,
  toEndOfDayISO,
  formatDayHeader,
  formatWeekHeader,
  getWeekdayName,
  parseTimeToHours,
  isSameLocalDate,
  syncSuccessText,
} from './utils';

describe('formatDuration', () => {
  it.each([
    [0, '0m'],
    [-5, '0m'],
    [5, '5m'],
    [30, '30m'],
    [59, '59m'],
    [60, '1h'],
    [90, '1h 30m'],
    [120, '2h'],
    [1500, '25h'],
    [1441, '24h 1m'],
  ])('formats %i minutes as "%s"', (minutes, expected) => {
    expect(formatDuration(minutes)).toBe(expected);
  });
});

describe('syncSuccessText', () => {
  it.each([
    [0, 0, 'Synced — 0 chunks scheduled, 0 Google events updated.'],
    [1, 1, 'Synced — 1 chunk scheduled, 1 Google event updated.'],
    [2, 3, 'Synced — 2 chunks scheduled, 3 Google events updated.'],
  ])('%i placed chunks, %i pushed → "%s"', (placed, pushed, expected) => {
    expect(syncSuccessText(placed, pushed)).toBe(expected);
  });
});

describe('formatShortDate', () => {
  it.each([
    { label: 'null', input: null },
    { label: 'invalid date', input: 'not-a-date' },
  ])('returns dash for $label', ({ input }) => {
    expect(formatShortDate(input)).toBe('—');
  });

  it('formats a valid ISO date', () => {
    const result = formatShortDate('2026-03-15T18:00:00Z');
    expect(result).toMatch(/Mar/);
    expect(result).toMatch(/15/);
  });
});

describe('formatTime', () => {
  it.each([
    { label: 'null', input: null },
    { label: 'invalid date', input: 'bad' },
  ])('returns dash for $label', ({ input }) => {
    expect(formatTime(input)).toBe('—');
  });

  it('formats a valid ISO time', () => {
    const result = formatTime('2026-03-15T18:30:00Z');
    // Result depends on locale/timezone, just check it's not dash
    expect(result).not.toBe('—');
    expect(result).toMatch(/\d{2}:\d{2}/);
  });
});

describe('formatDateTime', () => {
  it.each([
    { label: 'null', input: null },
    { label: 'invalid date', input: 'xyz' },
  ])('returns dash for $label', ({ input }) => {
    expect(formatDateTime(input)).toBe('—');
  });

  it('formats a valid ISO datetime', () => {
    const result = formatDateTime('2026-03-15T18:30:00Z');
    expect(result).toMatch(/Mar/);
    expect(result).toMatch(/\d{2}:\d{2}/);
  });
});

describe('formatRelativeTime', () => {
  const now = new Date('2026-03-15T12:00:00Z');

  it.each([
    { label: 'null', input: null },
    { label: 'invalid date', input: 'bad' },
  ])('returns dash for $label', ({ input }) => {
    expect(formatRelativeTime(input, now)).toBe('—');
  });

  it.each([
    { label: 'just now — future 20s', input: '2026-03-15T12:00:20Z', expected: 'just now' },
    { label: 'just now — past 20s', input: '2026-03-15T11:59:40Z', expected: 'just now' },
    { label: '15m ago', input: '2026-03-15T11:45:00Z', expected: '15m ago' },
    { label: 'in 30m', input: '2026-03-15T12:30:00Z', expected: 'in 30m' },
    { label: '3h ago', input: '2026-03-15T09:00:00Z', expected: '3h ago' },
    { label: 'in 2h', input: '2026-03-15T14:00:00Z', expected: 'in 2h' },
    { label: '2d ago', input: '2026-03-13T12:00:00Z', expected: '2d ago' },
    { label: 'in 3d', input: '2026-03-18T12:00:00Z', expected: 'in 3d' },
  ])('$label', ({ input, expected }) => {
    expect(formatRelativeTime(input, now)).toBe(expected);
  });
});

describe('startOfTodayMs', () => {
  const fixed = new Date(2026, 2, 28, 15, 30, 0); // March 28, 2026 at 15:30 local

  it('returns a timestamp at local midnight (hours = 0)', () => {
    const ts = startOfTodayMs(fixed);
    const d = new Date(ts);
    expect(d.getHours()).toBe(0);
    expect(d.getMinutes()).toBe(0);
    expect(d.getSeconds()).toBe(0);
    expect(d.getMilliseconds()).toBe(0);
  });

  it("returns the injected date's calendar date", () => {
    const ts = startOfTodayMs(fixed);
    const result = new Date(ts);
    expect(result.getFullYear()).toBe(2026);
    expect(result.getMonth()).toBe(2);
    expect(result.getDate()).toBe(28);
  });
});

describe('addDaysMs', () => {
  // Base timestamp: 2026-03-28T12:00:00 local (noon to avoid DST edge cases)
  const base = new Date(2026, 2, 28, 12, 0, 0).getTime();

  it.each([
    { n: 0, expectedDay: 28, label: 'zero days — same day' },
    { n: 1, expectedDay: 29, label: 'positive 1 day — next day' },
    { n: 7, expectedDay: 4, label: 'positive 7 days — wraps to April 4' },
    { n: -1, expectedDay: 27, label: 'negative 1 day — previous day' },
    { n: -7, expectedDay: 21, label: 'negative 7 days — previous week' },
  ])('$label', ({ n, expectedDay }) => {
    const result = new Date(addDaysMs(base, n));
    expect(result.getDate()).toBe(expectedDay);
  });

  it('does not mutate the input value (it is a number)', () => {
    const original = base;
    addDaysMs(base, 5);
    expect(base).toBe(original);
  });

  it('preserves hour component (DST-safe at noon)', () => {
    const result = new Date(addDaysMs(base, 1));
    // At noon, DST transitions should not alter the hour
    expect(result.getHours()).toBe(12);
  });
});

describe('getWeekStart', () => {
  // All inputs use a fixed "local" date constructed via new Date(y, m, d) so
  // results are not affected by the runner's timezone offset.
  it.each([
    // Wednesday → preceding Monday
    { input: new Date(2026, 2, 25), expectedDay: 23, label: 'Wed Mar 25 → Mon Mar 23' },
    // Monday itself → same Monday
    { input: new Date(2026, 2, 23), expectedDay: 23, label: 'Mon Mar 23 → Mon Mar 23' },
    // Sunday → preceding Monday
    { input: new Date(2026, 2, 29), expectedDay: 23, label: 'Sun Mar 29 → Mon Mar 23' },
    // Saturday → preceding Monday
    { input: new Date(2026, 2, 28), expectedDay: 23, label: 'Sat Mar 28 → Mon Mar 23' },
  ])('$label', ({ input, expectedDay }) => {
    const start = getWeekStart(input);
    expect(start.getDate()).toBe(expectedDay);
    expect(start.getHours()).toBe(0);
    expect(start.getMinutes()).toBe(0);
  });

  it('does not mutate the input date', () => {
    const d = new Date(2026, 2, 25);
    const original = d.getTime();
    getWeekStart(d);
    expect(d.getTime()).toBe(original);
  });
});

function expectEndOfDayDate(end: Date, expectedDay: number) {
  expect(end.getDate()).toBe(expectedDay);
  expect(end.getHours()).toBe(23);
  expect(end.getMinutes()).toBe(59);
  expect(end.getSeconds()).toBe(59);
}

describe('getWeekEnd', () => {
  it.each([
    // Wednesday → Sunday of same week
    { input: new Date(2026, 2, 25), expectedDay: 29, label: 'Wed Mar 25 → Sun Mar 29' },
    // Monday itself → Sunday of same week
    { input: new Date(2026, 2, 23), expectedDay: 29, label: 'Mon Mar 23 → Sun Mar 29' },
    // Sunday → same Sunday
    { input: new Date(2026, 2, 29), expectedDay: 29, label: 'Sun Mar 29 → Sun Mar 29' },
  ])('$label', ({ input, expectedDay }) => {
    expectEndOfDayDate(getWeekEnd(input), expectedDay);
  });

  it('week end is within the same 7-day window as week start', () => {
    const d = new Date(2026, 2, 25);
    const start = getWeekStart(d);
    const end = getWeekEnd(d);
    const diffDays = (end.getTime() - start.getTime()) / (1000 * 60 * 60 * 24);
    // Start=Mon 00:00:00, end=Sun 23:59:59.999 → diff is ~6.999 days
    expect(diffDays).toBeGreaterThanOrEqual(6);
    expect(diffDays).toBeLessThan(7);
  });
});

describe('getWeekStart with weekStart=sun', () => {
  it.each([
    // Wednesday → previous Sunday
    { input: new Date(2026, 6, 8), expectedDay: 5, label: 'Wed Jul 8 → Sun Jul 5' },
    // Sunday → same day
    { input: new Date(2026, 6, 5), expectedDay: 5, label: 'Sun Jul 5 → Sun Jul 5' },
    // Saturday → previous Sunday
    { input: new Date(2026, 6, 11), expectedDay: 5, label: 'Sat Jul 11 → Sun Jul 5' },
  ])('$label', ({ input, expectedDay }) => {
    const start = getWeekStart(input, 'sun');
    expect(start.getDate()).toBe(expectedDay);
    expect(start.getHours()).toBe(0);
    expect(start.getMinutes()).toBe(0);
  });

  it('default arg (no weekStart param) still gives Monday', () => {
    // 2026-07-08 is Wednesday; Monday start → Jul 6
    const d = new Date(2026, 6, 8);
    expect(getWeekStart(d).getDate()).toBe(6);
  });
});

describe('getWeekEnd with weekStart=sun', () => {
  it.each([
    // Wednesday → next Saturday (end of Sun–Sat week)
    { input: new Date(2026, 6, 8), expectedDay: 11, label: 'Wed Jul 8 → Sat Jul 11' },
    // Sunday → Saturday of same week
    { input: new Date(2026, 6, 5), expectedDay: 11, label: 'Sun Jul 5 → Sat Jul 11' },
    // Saturday → same Saturday
    { input: new Date(2026, 6, 11), expectedDay: 11, label: 'Sat Jul 11 → Sat Jul 11' },
  ])('$label — end at 23:59:59.999', ({ input, expectedDay }) => {
    const end = getWeekEnd(input, 'sun');
    expectEndOfDayDate(end, expectedDay);
    expect(end.getMilliseconds()).toBe(999);
  });

  it('default arg (no weekStart param) still gives Sunday', () => {
    // Mon-anchored week for Jul 8 (Wed) ends on Jul 12 (Sun)
    const d = new Date(2026, 6, 8);
    expect(getWeekEnd(d).getDate()).toBe(12);
  });
});

describe('toStartOfDayISO', () => {
  it('returns a valid ISO string', () => {
    const result = toStartOfDayISO(new Date(2026, 2, 28));
    expect(() => new Date(result)).not.toThrow();
    expect(isNaN(new Date(result).getTime())).toBe(false);
  });

  it('local midnight → UTC string differs by timezone offset but is still valid', () => {
    const d = new Date(2026, 2, 28);
    d.setHours(0, 0, 0, 0);
    const result = toStartOfDayISO(d);
    const parsed = new Date(result);
    // The time component in UTC might be anything depending on tz, but the
    // date returned is always midnight local time.
    expect(parsed.toLocaleDateString()).toBe(d.toLocaleDateString());
  });

  it('does not mutate the input date', () => {
    const d = new Date(2026, 2, 28, 14, 30);
    const original = d.getTime();
    toStartOfDayISO(d);
    expect(d.getTime()).toBe(original);
  });
});

describe('toEndOfDayISO', () => {
  it('returns a valid ISO string', () => {
    const result = toEndOfDayISO(new Date(2026, 2, 28));
    expect(isNaN(new Date(result).getTime())).toBe(false);
  });

  it('end of day ISO is later than start of day ISO', () => {
    const d = new Date(2026, 2, 28);
    const start = new Date(toStartOfDayISO(d)).getTime();
    const end = new Date(toEndOfDayISO(d)).getTime();
    expect(end).toBeGreaterThan(start);
  });
});

describe('formatDayHeader', () => {
  it('includes the weekday, month, day, and year', () => {
    // Use a fixed local date to avoid timezone edge cases
    const d = new Date(2026, 2, 28);
    const result = formatDayHeader(d);
    expect(result).toMatch(/2026/);
    expect(result).toMatch(/28/);
    expect(result.toLowerCase()).toMatch(/mar/);
  });

  it('returns a non-empty string for any date', () => {
    const result = formatDayHeader(new Date(2026, 0, 1));
    expect(result.length).toBeGreaterThan(0);
  });
});

describe('formatWeekHeader', () => {
  it('includes the year for a week within a single month', () => {
    // Week of Mar 23–29, 2026
    const d = new Date(2026, 2, 25);
    const result = formatWeekHeader(d);
    expect(result).toMatch(/2026/);
    expect(result).toMatch(/23/);
    expect(result).toMatch(/29/);
  });

  it('contains an en-dash separator', () => {
    const d = new Date(2026, 2, 25);
    const result = formatWeekHeader(d);
    expect(result).toContain('\u2013');
  });

  it('handles week spanning two months in same year', () => {
    // Week of Mar 30 – Apr 5, 2026
    const d = new Date(2026, 2, 30); // Monday
    const result = formatWeekHeader(d);
    expect(result).toMatch(/2026/);
    expect(result.toLowerCase()).toMatch(/mar/);
    expect(result.toLowerCase()).toMatch(/apr/);
  });

  it('handles week spanning two years', () => {
    // Dec 29, 2025 – Jan 4, 2026
    const d = new Date(2025, 11, 29); // Mon Dec 29 2025
    const result = formatWeekHeader(d);
    expect(result).toMatch(/2025/);
    expect(result).toMatch(/2026/);
  });
});

describe('getWeekdayName', () => {
  // 2026-03-23 is Monday
  const weekCases = [
    { date: new Date(2026, 2, 23), expected: 'Mon', label: 'Monday' },
    { date: new Date(2026, 2, 24), expected: 'Tue', label: 'Tuesday' },
    { date: new Date(2026, 2, 25), expected: 'Wed', label: 'Wednesday' },
    { date: new Date(2026, 2, 26), expected: 'Thu', label: 'Thursday' },
    { date: new Date(2026, 2, 27), expected: 'Fri', label: 'Friday' },
    { date: new Date(2026, 2, 28), expected: 'Sat', label: 'Saturday' },
    { date: new Date(2026, 2, 29), expected: 'Sun', label: 'Sunday' },
    { date: new Date(0), expected: 'Thu', label: 'epoch 1970-01-01' },
  ] as const;

  it.each(weekCases)('$label returns "$expected"', ({ date, expected }) => {
    expect(getWeekdayName(date)).toBe(expected);
  });
});

describe('isSameLocalDate', () => {
  it.each([
    {
      a: new Date(2026, 2, 28, 9, 0),
      b: new Date(2026, 2, 28, 18, 30),
      expected: true,
      label: 'same day different time',
    },
    {
      a: new Date(2026, 2, 28, 0, 0),
      b: new Date(2026, 2, 28, 23, 59),
      expected: true,
      label: 'same day midnight to 23:59',
    },
    {
      a: new Date(2026, 2, 28),
      b: new Date(2026, 2, 27),
      expected: false,
      label: 'consecutive days differ',
    },
    {
      a: new Date(2026, 2, 28),
      b: new Date(2026, 3, 28),
      expected: false,
      label: 'same day-of-month different month',
    },
    {
      a: new Date(2025, 2, 28),
      b: new Date(2026, 2, 28),
      expected: false,
      label: 'same day-of-month different year',
    },
    {
      a: new Date(2026, 2, 31, 23, 59, 59, 999),
      b: new Date(2026, 3, 1, 0, 0, 0, 0),
      expected: false,
      label: 'midnight boundary — last moment of Mar 31 vs first moment of Apr 1',
    },
  ])('$label → $expected', ({ a, b, expected }) => {
    expect(isSameLocalDate(a, b)).toBe(expected);
  });

  it('is symmetric: isSameLocalDate(a, b) === isSameLocalDate(b, a)', () => {
    const a = new Date(2026, 2, 28, 9, 0);
    const b = new Date(2026, 2, 29, 9, 0);
    expect(isSameLocalDate(a, b)).toBe(isSameLocalDate(b, a));
  });
});

describe('parseTimeToHours', () => {
  const cases = [
    { input: '00:00:00', expected: 0, label: 'midnight' },
    { input: '09:00:00', expected: 9, label: '9 AM whole hour' },
    { input: '18:30:00', expected: 18.5, label: '18:30 → 18.5h' },
    { input: '07:30:00', expected: 7.5, label: '07:30 → 7.5h' },
    { input: '23:59:00', expected: 23 + 59 / 60, label: '23:59' },
    { input: '24:00:00', expected: 24, label: '24:00 end of day' },
    { input: '10:00:30', expected: 10, label: 'ignores seconds (whole hour)' },
    { input: '10:30:59', expected: 10.5, label: 'ignores seconds (half hour)' },
  ];

  it.each(cases)('$label → $expected', ({ input, expected }) => {
    expect(parseTimeToHours(input)).toBeCloseTo(expected, 5);
  });
});
