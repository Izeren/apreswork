// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Date and duration formatting utilities.
 *
 * All datetime values are ISO 8601 UTC strings from the backend.
 * Formatting converts to the user's local timezone for display.
 */

/**
 * Parse a natural-language duration string into total minutes.
 *
 * Accepted formats (case-insensitive, whitespace-tolerant):
 *   Xh Ym  /  XhYm  /  Xh  /  Ym  /  X  (plain number → minutes)
 *
 * Returns null when the input cannot be parsed.
 */
export function parseDuration(input: string): number | null {
  const trimmed = input.trim().toLowerCase();
  if (trimmed === '') return null;

  // Plain integer: treat as minutes
  const plainMatch = /^\d+$/.exec(trimmed);
  if (plainMatch) {
    return parseInt(trimmed, 10);
  }

  // Xh Ym  or  XhYm  (both parts present)
  const bothMatch = /^(\d+)\s*h\s*(\d+)\s*m$/.exec(trimmed);
  if (bothMatch) {
    return parseInt(bothMatch[1], 10) * 60 + parseInt(bothMatch[2], 10);
  }

  const hoursMatch = /^(\d+)\s*h$/.exec(trimmed);
  if (hoursMatch) {
    return parseInt(hoursMatch[1], 10) * 60;
  }

  const minutesMatch = /^(\d+)\s*m$/.exec(trimmed);
  if (minutesMatch) {
    return parseInt(minutesMatch[1], 10);
  }

  return null;
}

export function formatDuration(minutes: number): string {
  if (minutes <= 0) return '0m';
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return `${h}h ${m}m`;
}

function parseIso(iso: string | null): Date | null {
  if (!iso) return null;
  const d = new Date(iso);
  return isNaN(d.getTime()) ? null : d;
}

export function formatShortDate(iso: string | null): string {
  const d = parseIso(iso);
  return d ? d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '—';
}

export function formatTime(iso: string | null): string {
  const d = parseIso(iso);
  return d
    ? d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12: false })
    : '—';
}

export function formatDateTime(iso: string | null): string {
  const d = parseIso(iso);
  if (!d) return '—';
  const date = d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  const time = d.toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
  return `${date}, ${time}`;
}

/** Toast text for a completed manual sync (shared by Settings and Calendar). */
export function syncSuccessText(placedCount: number, pushedCount: number): string {
  const chunks = `${placedCount} ${placedCount === 1 ? 'chunk' : 'chunks'} scheduled`;
  const events = `${pushedCount} Google ${pushedCount === 1 ? 'event' : 'events'} updated`;
  return `Synced — ${chunks}, ${events}.`;
}

export function isSameLocalDate(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export const DAYS_PER_WEEK = 7;

/** Which day of the week a calendar grid or quick-date anchor starts on. */
export type WeekStart = 'mon' | 'sun';

/**
 * Which weekday the "this week" quick-date resolves to: `'auto'` derives the
 * last day of the week from [`WeekStart`]; an explicit day means the next
 * occurrence of that weekday (today counts).
 */
export type QuickDateAnchor = 'auto' | 'fri' | 'sat' | 'sun';

/** Return midnight-local timestamp for today. Used to initialise the calendar. */
export function startOfTodayMs(now: Date): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/**
 * Compute the timestamp N days from the given timestamp.
 * Pass a negative `n` to go backwards.
 */
export function addDaysMs(timestamp: number, n: number): number {
  const d = new Date(timestamp);
  d.setDate(d.getDate() + n);
  return d.getTime();
}

/**
 * Return the start of the week containing `date`, anchored on Monday (default)
 * or Sunday per the `weekStart` argument. The returned Date is at midnight in
 * local time.
 *
 * Note: with `weekStart === 'mon'` this is the ISO week start; `'sun'` returns
 * the US/calendar-style Sunday anchor instead. The main calendar week view is
 * deliberately Monday-fixed and ignores `weekStart`; this parameter drives the
 * DateTimePicker quick-date anchor only.
 */
export function getWeekStart(date: Date, weekStart: WeekStart = 'mon'): Date {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  const startDow = weekStart === 'sun' ? 0 : 1;
  // JS: 0=Sun, 1=Mon … 6=Sat. Shift so the anchor day becomes 0.
  const dow = (d.getDay() + DAYS_PER_WEEK - startDow) % DAYS_PER_WEEK;
  d.setDate(d.getDate() - dow);
  return d;
}

/**
 * Return the end of the week containing `date`, anchored on Monday (default)
 * or Sunday per the `weekStart` argument. The returned Date is at 23:59:59.999
 * local time.
 */
export function getWeekEnd(date: Date, weekStart: WeekStart = 'mon'): Date {
  const d = getWeekStart(date, weekStart);
  d.setDate(d.getDate() + (DAYS_PER_WEEK - 1));
  d.setHours(23, 59, 59, 999);
  return d;
}

/**
 * Return an ISO 8601 UTC string for the start of `date` (00:00:00 local →
 * converted to UTC). Used when building API range queries.
 */
export function toStartOfDayISO(date: Date): string {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  return d.toISOString();
}

/**
 * Return an ISO 8601 UTC string for the end of `date` (23:59:59.999 local →
 * converted to UTC).
 */
export function toEndOfDayISO(date: Date): string {
  const d = new Date(date);
  d.setHours(23, 59, 59, 999);
  return d.toISOString();
}

/** Format a Date as "Monday, March 28, 2026" for the day-mode header. */
export function formatDayHeader(date: Date): string {
  return date.toLocaleDateString(undefined, {
    weekday: 'long',
    month: 'long',
    day: 'numeric',
    year: 'numeric',
  });
}

/**
 * Format a week range header like "Mar 23 – 29, 2026".
 * If the Monday and Sunday span two different years, both years are shown:
 * "Dec 29, 2025 – Jan 4, 2026".
 */
export function formatWeekHeader(date: Date): string {
  const start = getWeekStart(date);
  const end = getWeekEnd(date);

  const sameYear = start.getFullYear() === end.getFullYear();
  const sameMonth = start.getMonth() === end.getMonth();

  if (sameYear && sameMonth) {
    // "Mar 23 – 29, 2026"
    const month = start.toLocaleDateString(undefined, { month: 'short' });
    const startDay = start.getDate();
    const endDay = end.getDate();
    const year = start.getFullYear();
    return `${month} ${startDay} \u2013 ${endDay}, ${year}`;
  }

  if (sameYear) {
    // "Mar 30 – Apr 5, 2026"
    const startPart = start.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
    const endPart = end.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
    const year = start.getFullYear();
    return `${startPart} \u2013 ${endPart}, ${year}`;
  }

  // "Dec 29, 2025 – Jan 4, 2026"
  const startFull = start.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
  const endFull = end.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  });
  return `${startFull} \u2013 ${endFull}`;
}

export function getWeekdayName(date: Date): 'Mon' | 'Tue' | 'Wed' | 'Thu' | 'Fri' | 'Sat' | 'Sun' {
  const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const;
  return DAYS[date.getDay()];
}

export function parseTimeToHours(hhmmss: string): number {
  const [hh, mm] = hhmmss.split(':').map(Number);
  return hh + (mm ?? 0) / 60;
}

/** Relative time from now: "2h ago", "in 3d", "just now". */
export function formatRelativeTime(iso: string | null, now: Date): string {
  if (!iso) return '—';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '—';
  const diffMs = d.getTime() - now.getTime();
  const absDiffMin = Math.abs(Math.round(diffMs / 60_000));

  if (absDiffMin < 1) return 'just now';

  const future = diffMs > 0;
  let label: string;

  if (absDiffMin < 60) {
    label = `${absDiffMin}m`;
  } else if (absDiffMin < 1440) {
    label = `${Math.floor(absDiffMin / 60)}h`;
  } else {
    label = `${Math.floor(absDiffMin / 1440)}d`;
  }

  return future ? `in ${label}` : `${label} ago`;
}
