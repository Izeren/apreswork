// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import {
  FALLBACK_TIME,
  buildCalendarDays,
  buildQuickDateOptions,
  buildTimeOptions,
  endOfMonth,
  formatDisplayDate,
  fromLocalDateString,
  getInitialTime,
  getRelativeDateLabel,
  getTimezoneHint,
  isoToLocalDate,
  isoToLocalTime,
  shiftMonth,
  toLocalDateString,
  weekdayLabels,
  formatShortcutDate,
} from './dateTimePickerShared';

function optDate(opts: { id: string; date: string }[], id: string): string | undefined {
  return opts.find((o) => o.id === id)?.date;
}

describe('weekdayLabels', () => {
  it.each([
    { anchor: 'mon' as const, expected: ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'] },
    { anchor: 'sun' as const, expected: ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'] },
  ])('$anchor anchor labels and length', ({ anchor, expected }) => {
    const labels = weekdayLabels(anchor);
    expect(labels).toEqual(expected);
    expect(labels).toHaveLength(7);
  });
});

describe('buildCalendarDays — July 2026 (1st = Wednesday)', () => {
  const viewMonth = new Date(2026, 6, 1);

  it.each([
    { anchor: 'mon' as const, expectedStart: '2026-06-29', expectedLength: 42 },
    { anchor: 'sun' as const, expectedStart: '2026-06-28', expectedLength: 42 },
  ])(
    '$anchor grid starts $expectedStart with $expectedLength cells',
    ({ anchor, expectedStart, expectedLength }) => {
      const days = buildCalendarDays(viewMonth, anchor, new Date(2026, 0, 1));
      expect(days[0].date).toBe(expectedStart);
      expect(days).toHaveLength(expectedLength);
    },
  );

  it('isCurrentMonth flips at month boundaries', () => {
    const days = buildCalendarDays(viewMonth, 'mon', new Date(2026, 0, 1));
    expect(days[0].isCurrentMonth).toBe(false);
    const jul1 = days.find((d) => d.date === '2026-07-01');
    expect(jul1?.isCurrentMonth).toBe(true);
    expect(days[41].isCurrentMonth).toBe(false);
  });

  it('isToday marks the passed today arg', () => {
    const today = new Date(2026, 6, 15);
    const days = buildCalendarDays(viewMonth, 'mon', today);
    const todayCell = days.find((d) => d.date === '2026-07-15');
    expect(todayCell?.isToday).toBe(true);
    const notToday = days.find((d) => d.date === '2026-07-01');
    expect(notToday?.isToday).toBe(false);
  });
});

describe('buildCalendarDays — months starting on anchor day (offset 0)', () => {
  it.each([
    {
      label: "June 2026 (Mon) with 'mon'",
      date: new Date(2026, 5, 1),
      anchor: 'mon' as const,
      expectedFirst: '2026-06-01',
    },
    {
      label: "Feb 2026 (Sun) with 'sun'",
      date: new Date(2026, 1, 1),
      anchor: 'sun' as const,
      expectedFirst: '2026-02-01',
    },
  ])('$label → first cell is the 1st', ({ date, anchor, expectedFirst }) => {
    const days = buildCalendarDays(date, anchor, new Date(2026, 0, 1));
    expect(days[0].date).toBe(expectedFirst);
    expect(days[0].isCurrentMonth).toBe(true);
  });
});

describe('buildQuickDateOptions — anchor pinning', () => {
  const wed = new Date(2026, 6, 8);

  it.each([
    { weekStart: 'mon' as const, id: 'this-week', expected: '2026-07-12' },
    { weekStart: 'sun' as const, id: 'this-week', expected: '2026-07-11' },
    { weekStart: 'mon' as const, id: 'next-week', expected: '2026-07-19' },
    { weekStart: 'sun' as const, id: 'next-week', expected: '2026-07-18' },
    { weekStart: 'mon' as const, id: 'two-weeks', expected: '2026-07-26' },
    { weekStart: 'sun' as const, id: 'two-weeks', expected: '2026-07-25' },
  ])('$weekStart $id from Wednesday = $expected', ({ weekStart, id, expected }) => {
    const opts = buildQuickDateOptions(weekStart, wed);
    expect(optDate(opts, id)).toBe(expected);
  });

  const sun = new Date(2026, 6, 12);

  it("'mon' this-week on a Sunday = same day (old currentWeekSunday parity)", () => {
    expect(optDate(buildQuickDateOptions('mon', sun), 'this-week')).toBe('2026-07-12');
  });

  it("'sun' this-week on a Sunday = next Saturday (end of this Sun–Sat week)", () => {
    expect(optDate(buildQuickDateOptions('sun', sun), 'this-week')).toBe('2026-07-18');
  });

  it('today and tomorrow are unaffected by anchor', () => {
    const monOpts = buildQuickDateOptions('mon', wed);
    const sunOpts = buildQuickDateOptions('sun', wed);
    for (const id of ['today', 'tomorrow', 'seven-days', 'this-month', 'next-month']) {
      expect(optDate(monOpts, id)).toBe(optDate(sunOpts, id));
    }
  });

  it('always returns 8 options', () => {
    expect(buildQuickDateOptions('mon', wed)).toHaveLength(8);
    expect(buildQuickDateOptions('sun', wed)).toHaveLength(8);
  });
});

describe('buildQuickDateOptions — explicit week-end anchor', () => {
  const wed = new Date(2026, 6, 8);

  it.each([{ weekStart: 'mon' as const }, { weekStart: 'sun' as const }])(
    "$weekStart 'auto' is identical to omitting the anchor",
    ({ weekStart }) => {
      expect(buildQuickDateOptions(weekStart, wed, 'auto')).toEqual(
        buildQuickDateOptions(weekStart, wed),
      );
    },
  );

  it.each([
    { id: 'this-week', expected: '2026-07-10' },
    { id: 'next-week', expected: '2026-07-17' },
    { id: 'two-weeks', expected: '2026-07-24' },
  ])("'fri' $id from Wednesday = $expected", ({ id, expected }) => {
    const opts = buildQuickDateOptions('mon', wed, 'fri');
    expect(optDate(opts, id)).toBe(expected);
  });

  it("'sat' this-week from Wednesday = the coming Saturday", () => {
    const opts = buildQuickDateOptions('mon', wed, 'sat');
    expect(optDate(opts, 'this-week')).toBe('2026-07-11');
  });

  it("'sun' anchor matches 'auto' on a mon-start week", () => {
    expect(optDate(buildQuickDateOptions('mon', wed, 'sun'), 'this-week')).toBe(
      optDate(buildQuickDateOptions('mon', wed, 'auto'), 'this-week'),
    );
  });

  it("'fri' on a Friday = same day (today counts as the next occurrence)", () => {
    const fri = new Date(2026, 6, 10);
    expect(optDate(buildQuickDateOptions('mon', fri, 'fri'), 'this-week')).toBe('2026-07-10');
  });

  it("'fri' on a Saturday rolls to the next Friday (never a past date)", () => {
    const sat = new Date(2026, 6, 11);
    expect(optDate(buildQuickDateOptions('mon', sat, 'fri'), 'this-week')).toBe('2026-07-17');
  });

  it('anchor is independent of weekStart', () => {
    expect(buildQuickDateOptions('mon', wed, 'fri')).toEqual(
      buildQuickDateOptions('sun', wed, 'fri'),
    );
  });

  it('anchor does not affect non-week options', () => {
    const auto = buildQuickDateOptions('mon', wed, 'auto');
    const fri = buildQuickDateOptions('mon', wed, 'fri');
    for (const id of ['today', 'tomorrow', 'seven-days', 'this-month', 'next-month']) {
      expect(optDate(fri, id)).toBe(optDate(auto, id));
    }
  });
});

describe('buildTimeOptions', () => {
  const options = buildTimeOptions();

  it('returns 49 options with correct boundaries', () => {
    // 24h × 2 half-hours = 48, plus the extra 23:59 sentinel
    expect(options).toHaveLength(49);
    expect(options[0].value).toBe('00:00');
    expect(options.some((o) => o.value === '23:30')).toBe(true);
    expect(options[options.length - 1].value).toBe('23:59');
  });
});

describe('isoToLocalDate', () => {
  it('extracts local date in YYYY-MM-DD format', () => {
    // noon to avoid DST date-boundary shift
    const iso = new Date(2026, 2, 28, 12, 0, 0).toISOString();
    expect(isoToLocalDate(iso)).toBe('2026-03-28');
  });
});

describe('isoToLocalTime', () => {
  it.each([
    { desc: 'extracts local HH:MM', hour: 14, minute: 30, expected: '14:30' },
    { desc: 'zero-pads hours and minutes', hour: 9, minute: 5, expected: '09:05' },
  ])('$desc', ({ hour, minute, expected }) => {
    const iso = new Date(2026, 2, 28, hour, minute, 0).toISOString();
    expect(isoToLocalTime(iso)).toBe(expected);
  });
});

describe('toLocalDateString / fromLocalDateString round-trip', () => {
  it.each([
    '2026-01-01',
    '2026-07-08',
    '2026-12-31',
    '2024-02-29', // leap year
  ])('round-trips %s', (date) => {
    const d = fromLocalDateString(date);
    expect(toLocalDateString(d)).toBe(date);
  });
});

describe('formatDisplayDate', () => {
  it.each([
    { date: '2026-07-08', expected: '08/07/2026' },
    { date: '2026-01-01', expected: '01/01/2026' },
    { date: '2026-12-31', expected: '31/12/2026' },
  ])('formats $date as $expected', ({ date, expected }) => {
    expect(formatDisplayDate(date)).toBe(expected);
  });
});

describe('getRelativeDateLabel', () => {
  const today = new Date(2026, 6, 8); // Wed Jul 8

  it.each([
    { date: '2026-07-08', expected: 'Today' },
    { date: '2026-07-09', expected: 'Tomorrow' },
    { date: '2026-07-10', expected: null },
    { date: '2026-07-07', expected: null },
  ])('$date → $expected', ({ date, expected }) => {
    expect(getRelativeDateLabel(date, today)).toBe(expected);
  });
});

describe('getInitialTime', () => {
  const iso14h30 = new Date(2026, 2, 28, 14, 30, 0).toISOString();

  it.each([
    {
      label: 'ISO string extracts local time',
      value: iso14h30,
      defaultTime: null,
      expected: '14:30',
    },
    {
      label: 'null value falls back to defaultTime',
      value: null,
      defaultTime: '10:00',
      expected: '10:00',
    },
    {
      label: 'both null falls back to FALLBACK_TIME',
      value: null,
      defaultTime: null,
      expected: FALLBACK_TIME,
    },
  ])('$label', ({ value, defaultTime, expected }) => {
    expect(getInitialTime(value, defaultTime)).toBe(expected);
  });
});

describe('endOfMonth', () => {
  it.each([
    { label: 'January → 31', date: new Date(2026, 0, 15), month: 0, lastDay: 31 },
    { label: 'February 2026 (non-leap)', date: new Date(2026, 1, 10), month: 1, lastDay: 28 },
    { label: 'February 2024 (leap)', date: new Date(2024, 1, 10), month: 1, lastDay: 29 },
  ])('$label', ({ date, month, lastDay }) => {
    const end = endOfMonth(date);
    expect(end.getMonth()).toBe(month);
    expect(end.getDate()).toBe(lastDay);
    expect(end.getHours()).toBe(0);
  });
});

describe('shiftMonth', () => {
  it.each([
    {
      label: 'forward one month',
      input: new Date(2026, 0, 15),
      delta: 1,
      expectedYear: 2026,
      expectedMonth: 1,
      expectedDate: 1,
    },
    {
      label: 'backward one month',
      input: new Date(2026, 2, 15),
      delta: -1,
      expectedYear: 2026,
      expectedMonth: 1,
      expectedDate: 1,
    },
    {
      label: 'month-length overflow (Jan 31)',
      input: new Date(2026, 0, 31),
      delta: 1,
      expectedYear: 2026,
      expectedMonth: 1,
      expectedDate: 1,
    },
    {
      label: 'year wrap (Dec → Jan)',
      input: new Date(2026, 11, 1),
      delta: 1,
      expectedYear: 2027,
      expectedMonth: 0,
      expectedDate: 1,
    },
  ])('$label', ({ input, delta, expectedYear, expectedMonth, expectedDate }) => {
    const result = shiftMonth(input, delta);
    expect(result.getFullYear()).toBe(expectedYear);
    expect(result.getMonth()).toBe(expectedMonth);
    expect(result.getDate()).toBe(expectedDate);
  });
});

describe('formatShortcutDate', () => {
  it.each([
    { check: 'is non-empty', predicate: (s: string) => s.length > 0 },
    { check: 'contains the day number', predicate: (s: string) => s.includes('8') },
  ])('$check', ({ predicate }) => {
    expect(predicate(formatShortcutDate('2026-07-08'))).toBe(true);
  });
});

describe('getTimezoneHint', () => {
  it('is non-empty and contains UTC', () => {
    const hint = getTimezoneHint(new Date(2026, 6, 8, 12, 0, 0));
    expect(hint.length).toBeGreaterThan(0);
    expect(hint).toContain('UTC');
  });
});
