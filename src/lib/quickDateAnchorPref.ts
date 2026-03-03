// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Persisted quick-date week-end anchor preference (per machine,
 * localStorage-backed).
 *
 * Consumed by the DateTimePicker's quick-date options: `'auto'` keeps the
 * week-start-derived end of week; an explicit day ('fri'/'sat'/'sun') pins
 * "this week" to the next occurrence of that weekday. Surfaced in the
 * Settings page.
 *
 * localStorage is user-editable, so the loaded value is validated strictly:
 * the stored token must be exactly one of the known anchors; anything else
 * (missing key, junk, JSON blobs, numbers) falls back to 'auto'. Callers pass
 * their storage (`window.localStorage` in components) — no ambient-global
 * default, so node-env tests stay dependency-free.
 */

import type { QuickDateAnchor } from './utils';

type PrefStorage = Pick<Storage, 'getItem' | 'setItem'>;

export const QUICK_DATE_ANCHOR_STORAGE_KEY = 'apreswork.quickDateAnchor';

const VALID_ANCHORS: readonly QuickDateAnchor[] = ['auto', 'fri', 'sat', 'sun'];

/**
 * Load the persisted quick-date anchor preference. The raw value must be
 * exactly one of `'auto' | 'fri' | 'sat' | 'sun'` (stored as a plain token,
 * not JSON); anything else falls back to `'auto'`.
 */
export function loadQuickDateAnchor(storage: PrefStorage): QuickDateAnchor {
  try {
    const raw = storage.getItem(QUICK_DATE_ANCHOR_STORAGE_KEY);
    if ((VALID_ANCHORS as readonly string[]).includes(raw ?? '')) {
      return raw as QuickDateAnchor;
    }
    return 'auto';
  } catch {
    return 'auto';
  }
}

/**
 * Persist the quick-date anchor preference. Storage failures (quota, denial)
 * are swallowed — the preference still applies for the current session; only
 * persistence across sessions is lost.
 */
export function saveQuickDateAnchor(anchor: QuickDateAnchor, storage: PrefStorage): void {
  try {
    storage.setItem(QUICK_DATE_ANCHOR_STORAGE_KEY, anchor);
  } catch {
    // Preference still applies for the session; only persistence is lost.
  }
}
