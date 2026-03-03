// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Persisted Mon/Sun week-start preference (per machine, localStorage-backed).
 *
 * Consumed by the DateTimePicker to anchor its calendar grid and quick-date
 * options. The main calendar week view is deliberately Monday-fixed for now
 * and does not read this preference — that is a separate decision.
 *
 * localStorage is user-editable, so the loaded value is validated strictly:
 * the stored token must be exactly 'mon' or 'sun'; anything else (missing key,
 * junk, JSON blobs, numbers) falls back to 'mon'. Callers pass their storage
 * (`window.localStorage` in components) — no ambient-global default, so
 * node-env tests stay dependency-free.
 */

import type { WeekStart } from './utils';

type PrefStorage = Pick<Storage, 'getItem' | 'setItem'>;

export const WEEK_START_STORAGE_KEY = 'apreswork.weekStart';

/**
 * Load the persisted week-start preference. The raw value must be exactly
 * `'mon'` or `'sun'` (stored as a plain token, not JSON); anything else falls
 * back to `'mon'`.
 */
export function loadWeekStart(storage: PrefStorage): WeekStart {
  try {
    const raw = storage.getItem(WEEK_START_STORAGE_KEY);
    if (raw === 'mon' || raw === 'sun') return raw;
    return 'mon';
  } catch {
    return 'mon';
  }
}

/**
 * Persist the week-start preference. Storage failures (quota, denial) are
 * swallowed — the preference still applies for the current session; only
 * persistence across sessions is lost.
 */
export function saveWeekStart(weekStart: WeekStart, storage: PrefStorage): void {
  try {
    storage.setItem(WEEK_START_STORAGE_KEY, weekStart);
  } catch {
    // quota/permission errors are expected; in-session preference is unaffected
  }
}
