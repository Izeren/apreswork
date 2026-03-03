// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Persisted task-list view preferences (per machine, localStorage-backed).
 *
 * localStorage is user-editable, so every loaded value is validated
 * field-by-field; anything unexpected falls back to the default. Callers pass
 * their storage (`window.localStorage` in components) — no ambient-global
 * default, so node-env tests stay dependency-free.
 */

import type { Priority, TaskStatus } from '../../types';
import { PRIORITIES, TASK_STATUSES } from '../../types';
import { SORT_FIELDS, SORT_DIRECTIONS } from './taskSort';
import type { SortDirection, SortField, SortKey } from './taskSort';

type PrefStorage = Pick<Storage, 'getItem' | 'setItem'>;

/**
 * Load a persisted selection list. An empty array is a valid stored value
 * (the explicit "All" selection); only a missing or invalid value falls back
 * to the given default. Duplicates are dropped, keeping first-seen order.
 */
function loadListFilter<T extends string>(
  storage: PrefStorage,
  key: string,
  allowed: readonly T[],
  fallback: readonly T[],
): T[] {
  try {
    const raw = storage.getItem(key);
    if (raw === null) return [...fallback];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [...fallback];
    const values: T[] = [];
    for (const entry of parsed) {
      if (typeof entry !== 'string' || !(allowed as readonly string[]).includes(entry)) {
        return [...fallback];
      }
      const value = entry as T;
      if (!values.includes(value)) values.push(value);
    }
    return values;
  } catch {
    return [...fallback];
  }
}

/** Persist a selection list; storage failures (quota, denial) are ignored. */
function saveListFilter(storage: PrefStorage, key: string, values: readonly string[]): void {
  try {
    storage.setItem(key, JSON.stringify(values));
  } catch {
    // Filtering still works for the session; only persistence is lost.
  }
}

export const STATUS_FILTER_STORAGE_KEY = 'apreswork.taskList.statuses';

/** Baseline selection: a freshly opened task list shows scheduled tasks only. */
export const DEFAULT_STATUS_FILTER: readonly TaskStatus[] = ['scheduled'];

export function loadStatusFilter(storage: PrefStorage): TaskStatus[] {
  return loadListFilter(storage, STATUS_FILTER_STORAGE_KEY, TASK_STATUSES, DEFAULT_STATUS_FILTER);
}

export function saveStatusFilter(statuses: TaskStatus[], storage: PrefStorage): void {
  saveListFilter(storage, STATUS_FILTER_STORAGE_KEY, statuses);
}

export const PRIORITY_FILTER_STORAGE_KEY = 'apreswork.taskList.priorities';

/** Baseline selection: no priority constraint ("All"). */
export const DEFAULT_PRIORITY_FILTER: readonly Priority[] = [];

export function loadPriorityFilter(storage: PrefStorage): Priority[] {
  return loadListFilter(storage, PRIORITY_FILTER_STORAGE_KEY, PRIORITIES, DEFAULT_PRIORITY_FILTER);
}

export function savePriorityFilter(priorities: Priority[], storage: PrefStorage): void {
  saveListFilter(storage, PRIORITY_FILTER_STORAGE_KEY, priorities);
}

export const SORT_STORAGE_KEY = 'apreswork.taskList.sort';

/**
 * Load the persisted sort-key stack; anything unexpected yields null and the
 * caller falls back to the default stack. Rejecting duplicate fields bounds
 * the stack to one entry per field (≤5). The pre-stack single-key
 * {field, direction} value fails validation once and thereby resets.
 */
export function loadSortStack(storage: PrefStorage): SortKey[] | null {
  try {
    const raw = storage.getItem(SORT_STORAGE_KEY);
    if (raw === null) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed) || parsed.length === 0) return null;
    const keys: SortKey[] = [];
    for (const entry of parsed) {
      if (typeof entry !== 'object' || entry === null) return null;
      const { field, direction } = entry as Record<string, unknown>;
      if (typeof field !== 'string' || !SORT_FIELDS.includes(field as SortField)) return null;
      if (typeof direction !== 'string' || !SORT_DIRECTIONS.includes(direction as SortDirection))
        return null;
      if (keys.some((key) => key.field === field)) return null;
      keys.push({ field: field as SortField, direction: direction as SortDirection });
    }
    return keys;
  } catch {
    return null;
  }
}

/** Persist the sort-key stack; storage failures (quota, denial) are ignored. */
export function saveSortStack(stack: readonly SortKey[], storage: PrefStorage): void {
  try {
    storage.setItem(SORT_STORAGE_KEY, JSON.stringify(stack));
  } catch {
    // Sorting still works for the session; only persistence is lost.
  }
}
