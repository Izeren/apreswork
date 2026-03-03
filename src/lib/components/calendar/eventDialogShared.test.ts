// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import { isoToLocalDate } from '../shared/dateTimePickerShared';
import { localMidnightIso, buildAllDayRange, allDayEndToInclusiveDate } from './eventDialogShared';

describe('eventDialogShared — localMidnightIso', () => {
  it('round-trips back to the same local date', () => {
    expect(isoToLocalDate(localMidnightIso('2026-07-15'))).toBe('2026-07-15');
  });

  it('lands on local midnight (00:00)', () => {
    const d = new Date(localMidnightIso('2026-07-15'));
    expect(d.getHours()).toBe(0);
    expect(d.getMinutes()).toBe(0);
    expect(d.getSeconds()).toBe(0);
  });
});

describe('eventDialogShared — buildAllDayRange', () => {
  it.each([
    {
      label: 'single day',
      startDate: '2026-07-15',
      endDate: '2026-07-15',
      expectedStart: '2026-07-15',
      expectedEnd: '2026-07-16',
    },
    {
      label: 'multi-day',
      startDate: '2026-07-15',
      endDate: '2026-07-17',
      expectedStart: '2026-07-15',
      expectedEnd: '2026-07-18',
    },
    {
      label: 'month boundary',
      startDate: '2026-07-31',
      endDate: '2026-07-31',
      expectedStart: '2026-07-31',
      expectedEnd: '2026-08-01',
    },
    {
      label: 'year boundary',
      startDate: '2026-12-31',
      endDate: '2026-12-31',
      expectedStart: '2026-12-31',
      expectedEnd: '2027-01-01',
    },
  ])(
    '$label: exclusive end is one day after',
    ({ startDate, endDate, expectedStart, expectedEnd }) => {
      const { start, end } = buildAllDayRange(startDate, endDate);
      expect(isoToLocalDate(start)).toBe(expectedStart);
      expect(isoToLocalDate(end)).toBe(expectedEnd);
    },
  );
});

describe('eventDialogShared — allDayEndToInclusiveDate', () => {
  it('converts an exclusive end instant back to the inclusive last date', () => {
    expect(allDayEndToInclusiveDate(localMidnightIso('2026-07-16'))).toBe('2026-07-15');
  });

  it('inverts buildAllDayRange for a single-day range', () => {
    const { end } = buildAllDayRange('2026-07-15', '2026-07-15');
    expect(allDayEndToInclusiveDate(end)).toBe('2026-07-15');
  });

  it('inverts buildAllDayRange for a multi-day range', () => {
    const { end } = buildAllDayRange('2026-07-15', '2026-07-17');
    expect(allDayEndToInclusiveDate(end)).toBe('2026-07-17');
  });

  it.each([
    { label: 'month boundary', input: '2026-08-01', expected: '2026-07-31' },
    { label: 'year boundary', input: '2027-01-01', expected: '2026-12-31' },
  ])('rolls back across a $label', ({ input, expected }) => {
    expect(allDayEndToInclusiveDate(localMidnightIso(input))).toBe(expected);
  });
});
