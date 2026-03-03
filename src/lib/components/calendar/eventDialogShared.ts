// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Pure date helpers for EventDialog's all-day handling.
 *
 * No component state, no Svelte imports — plain TypeScript so the exclusive↔
 * inclusive end-date arithmetic can be unit-tested in isolation. Extracted from
 * EventDialog.svelte, mirroring the dateTimePickerShared split.
 *
 * All-day convention (matches the Rust write-back in services/sync.rs): the
 * mirror's `start_time` is Local midnight of the first day and `end_time` is
 * Local midnight of the day AFTER the last day (exclusive). The dialog shows the
 * user an INCLUSIVE last day, so these helpers translate between the two.
 */

import {
  isoToLocalDate,
  toLocalDateString,
  fromLocalDateString,
  addDays,
} from '../shared/dateTimePickerShared';

function addDaysToLocalDate(localDate: string, days: number): string {
  return toLocalDateString(addDays(fromLocalDateString(localDate), days));
}

/** Local midnight of a `yyyy-mm-dd` date, as a UTC ISO instant. */
export function localMidnightIso(localDate: string): string {
  // `T00:00:00` (no zone) parses as local time; toISOString normalizes to UTC.
  return new Date(`${localDate}T00:00:00`).toISOString();
}

/**
 * Build the `{ start, end }` ISO instants for an all-day range from the two
 * INCLUSIVE local dates the user picked. `end` is EXCLUSIVE (Local midnight of
 * the day after `endInclusive`), matching the Google all-day `date` convention.
 */
export function buildAllDayRange(
  startDate: string,
  endInclusive: string,
): { start: string; end: string } {
  const exclusiveEnd = addDaysToLocalDate(endInclusive, 1);
  return { start: localMidnightIso(startDate), end: localMidnightIso(exclusiveEnd) };
}

/**
 * Convert a mirrored all-day event's EXCLUSIVE `end_time` instant to the
 * INCLUSIVE last local date (`yyyy-mm-dd`) for display in the dialog.
 */
export function allDayEndToInclusiveDate(endIso: string): string {
  // fromLocalDateString anchors at local noon, so ±1 day is DST-safe.
  return addDaysToLocalDate(isoToLocalDate(endIso), -1);
}
