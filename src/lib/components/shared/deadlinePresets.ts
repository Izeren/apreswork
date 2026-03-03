// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { addDays, toLocalDateString } from './dateTimePickerShared';

export function todayDeadline(now: Date): string {
  return customDeadlineIso(toLocalDateString(now));
}

export function tomorrowDeadline(now: Date): string {
  return customDeadlineIso(toLocalDateString(addDays(now, 1)));
}

/** Deadline preset values are relative to the current time (plan decision). */
export function nextWeekDeadline(now: Date): string {
  const d = new Date(now);
  d.setDate(d.getDate() + 7);
  return d.toISOString();
}

export function nextMonthDeadline(now: Date): string {
  const d = new Date(now);
  // Clamp to the last day of the next month (Jan 31 → Feb 28, not Mar 3).
  const day = d.getDate();
  d.setDate(1);
  d.setMonth(d.getMonth() + 1);
  d.setDate(Math.min(day, new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate()));
  return d.toISOString();
}

/**
 * ISO deadline for a calendar-picked local date: always end of day
 * (23:59), regardless of any existing deadline's time of day (owner
 * decision — a calendar pick resets the time, it never carries one over).
 */
export function customDeadlineIso(localDate: string): string {
  return new Date(`${localDate}T23:59:00`).toISOString();
}
