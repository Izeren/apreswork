// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Pure date/format/build helpers and picker types/constants for DateTimePicker.
 *
 * No component state, no Svelte imports — plain TypeScript only. Extracted
 * from DateTimePicker.svelte to keep that file under the 1000-line limit and
 * to enable unit-testing the pure logic independently.
 */

import { getWeekEnd, DAYS_PER_WEEK } from '../../utils';
import type { QuickDateAnchor, WeekStart } from '../../utils';

export interface CalendarDay {
  date: string;
  label: string;
  isCurrentMonth: boolean;
  isToday: boolean;
}

export interface QuickDateOption {
  id: string;
  label: string;
  date: string;
}

export interface TimeOption {
  label: string;
  value: string;
}

export const FALLBACK_TIME = '09:00';

export const STICKY_TIME_OPTIONS: TimeOption[] = [
  { label: 'Morning', value: FALLBACK_TIME },
  { label: 'Lunch', value: '12:00' },
  { label: 'Evening', value: '17:00' },
  { label: 'End of day', value: '23:59' },
];

export const PICKER_MARGIN = 12;
export const PICKER_GAP = 8;

export function getTimezoneHint(now: Date): string {
  const tz = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const offsetMinutes = -now.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? '+' : '-';
  const absH = Math.floor(Math.abs(offsetMinutes) / 60);
  const absM = Math.abs(offsetMinutes) % 60;
  if (absM === 0) return `${tz} (UTC${sign}${absH})`;
  return `${tz} (UTC${sign}${absH}:${String(absM).padStart(2, '0')})`;
}

export function isoToLocalDate(iso: string): string {
  const date = new Date(iso);
  return toLocalDateString(date);
}

export function isoToLocalTime(iso: string): string {
  const date = new Date(iso);
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  return `${hours}:${minutes}`;
}

export function startOfDay(date: Date): Date {
  const next = new Date(date);
  next.setHours(0, 0, 0, 0);
  return next;
}

export function startOfMonth(date: Date): Date {
  const next = startOfDay(date);
  next.setDate(1);
  return next;
}

export function addDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

export function shiftMonth(date: Date, delta: number): Date {
  const next = startOfMonth(date);
  next.setMonth(next.getMonth() + delta);
  return next;
}

export function toLocalDateString(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export function fromLocalDateString(localDate: string): Date {
  return new Date(`${localDate}T12:00:00`);
}

export function formatDisplayDate(localDate: string): string {
  const [year, month, day] = localDate.split('-');
  return `${day}/${month}/${year}`;
}

export function getRelativeDateLabel(localDate: string, today: Date): string | null {
  const current = toLocalDateString(startOfDay(today));
  if (localDate === current) return 'Today';
  if (localDate === toLocalDateString(addDays(startOfDay(today), 1))) return 'Tomorrow';
  return null;
}

export function formatShortcutDate(localDate: string): string {
  return fromLocalDateString(localDate).toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
  });
}

export function endOfMonth(date: Date): Date {
  const next = startOfMonth(date);
  next.setMonth(next.getMonth() + 1, 0);
  return startOfDay(next);
}

export function buildTimeOptions(): TimeOption[] {
  const options: TimeOption[] = [];
  for (let hour = 0; hour < 24; hour += 1) {
    for (const minute of [0, 30]) {
      const value = `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`;
      options.push({ label: value, value });
    }
  }
  options.push({ label: '23:59', value: '23:59' });
  return options;
}

export function weekdayLabels(weekStart: WeekStart): string[] {
  if (weekStart === 'sun') {
    return ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];
  }
  return ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'];
}

/** JS `getDay()` index for each explicit quick-date anchor day. */
const QUICK_DATE_ANCHOR_DOW: Record<Exclude<QuickDateAnchor, 'auto'>, number> = {
  sun: 0,
  fri: 5,
  sat: 6,
};

/**
 * Build the 8 quick-date options for the picker sidebar.
 *
 * With `anchor === 'auto'`, the "this-week" / "next-week" / "in-2-weeks"
 * anchors use the last day of the current week according to `weekStart`:
 *   - `'mon'`: Sunday (end of Mon–Sun week)
 *   - `'sun'`: Saturday (end of Sun–Sat week)
 *
 * An explicit `anchor` overrides that with the next occurrence of the chosen
 * weekday (today counts), independent of `weekStart` — so the anchor never
 * yields a past date; "next-week" / "in-2-weeks" stay +7 / +14 from it.
 *
 * For `'mon'`/`'auto'` this is provably identical to the old
 * `currentWeekSunday` helper on every weekday; see
 * `dateTimePickerShared.test.ts` anchor-pinning tests.
 */
export function buildQuickDateOptions(
  weekStart: WeekStart,
  today: Date,
  anchor: QuickDateAnchor = 'auto',
): QuickDateOption[] {
  const start = startOfDay(today);
  // Last day of the week containing today (or the next occurrence of the
  // explicit anchor day), stripped to midnight.
  const weekEndDay =
    anchor === 'auto'
      ? startOfDay(getWeekEnd(start, weekStart))
      : addDays(
          start,
          (QUICK_DATE_ANCHOR_DOW[anchor] - start.getDay() + DAYS_PER_WEEK) % DAYS_PER_WEEK,
        );
  const nextMonthDate = shiftMonth(start, 1);
  return [
    { id: 'today', label: 'Today', date: toLocalDateString(start) },
    { id: 'tomorrow', label: 'Tomorrow', date: toLocalDateString(addDays(start, 1)) },
    { id: 'this-week', label: 'This week', date: toLocalDateString(weekEndDay) },
    { id: 'seven-days', label: '7 days from now', date: toLocalDateString(addDays(start, 7)) },
    { id: 'next-week', label: 'Next week', date: toLocalDateString(addDays(weekEndDay, 7)) },
    { id: 'two-weeks', label: 'In 2 weeks', date: toLocalDateString(addDays(weekEndDay, 14)) },
    { id: 'this-month', label: 'This month', date: toLocalDateString(endOfMonth(start)) },
    { id: 'next-month', label: 'Next month', date: toLocalDateString(endOfMonth(nextMonthDate)) },
  ];
}

/**
 * Build the 42-cell calendar grid for `viewMonth`, anchored on Monday or
 * Sunday per `weekStart`. Pass an explicit `today` to fix the "today" marker
 * in tests.
 */
export function buildCalendarDays(
  viewMonth: Date,
  weekStart: WeekStart,
  today: Date,
): CalendarDay[] {
  const firstOfMonth = startOfMonth(viewMonth);
  const startDow = weekStart === 'sun' ? 0 : 1;
  const offset = (firstOfMonth.getDay() + DAYS_PER_WEEK - startDow) % DAYS_PER_WEEK;
  const gridStart = addDays(firstOfMonth, -offset);
  const todayStr = toLocalDateString(startOfDay(today));
  const days: CalendarDay[] = [];

  for (let index = 0; index < 42; index += 1) {
    const current = addDays(gridStart, index);
    days.push({
      date: toLocalDateString(current),
      label: String(current.getDate()),
      isCurrentMonth: current.getMonth() === viewMonth.getMonth(),
      isToday: toLocalDateString(current) === todayStr,
    });
  }

  return days;
}

/**
 * Determine the initial draft time when syncing from a prop value.
 * - If `nextValue` is an ISO string, extract its local HH:MM.
 * - Otherwise fall back to `defaultTime`, then `FALLBACK_TIME`.
 */
export function getInitialTime(nextValue: string | null, defaultTime: string | null): string {
  if (nextValue) return isoToLocalTime(nextValue);
  return defaultTime ?? FALLBACK_TIME;
}
