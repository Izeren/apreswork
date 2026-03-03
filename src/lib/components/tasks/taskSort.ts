// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { Task, Priority, TaskStatus } from '../../types';
import { PRIORITIES, TASK_STATUSES } from '../../types';

export type SortField = 'status' | 'priority' | 'deadline' | 'title' | 'logged';
export type SortDirection = 'asc' | 'desc';

export const SORT_FIELDS: readonly SortField[] = [
  'status',
  'priority',
  'deadline',
  'title',
  'logged',
];
export const SORT_DIRECTIONS: readonly SortDirection[] = ['asc', 'desc'];

/** One entry of the composable sort stack; index 0 is the primary key. */
export interface SortKey {
  readonly field: SortField;
  readonly direction: SortDirection;
}

/**
 * Baseline stack: Critical on top, earlier deadline breaks ties (null
 * deadlines last). The ONE definition of the default task-list ordering.
 */
export const DEFAULT_SORT_STACK: readonly SortKey[] = [
  { field: 'priority', direction: 'desc' },
  { field: 'deadline', direction: 'asc' },
];

function defaultDirection(field: SortField): SortDirection {
  return field === 'priority' ? 'desc' : 'asc';
}

const PRIORITY_ORDER: Record<Priority, number> = Object.fromEntries(
  PRIORITIES.map((p, i) => [p, PRIORITIES.length - i]),
) as Record<Priority, number>;

const STATUS_ORDER: Record<TaskStatus, number> = Object.fromEntries(
  TASK_STATUSES.map((s, i) => [s, i + 1]),
) as Record<TaskStatus, number>;

export function compareByPriority(a: Task, b: Task, direction: SortDirection = 'desc'): number {
  const diff = PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority];
  // Use || 0 to normalise -0 to 0 (avoids Object.is(-0, 0) === false in tests)
  return (direction === 'desc' ? -diff : diff) || 0;
}

export function compareByDeadline(a: Task, b: Task, direction: SortDirection = 'asc'): number {
  if (a.deadline === null && b.deadline === null) return 0;
  if (a.deadline === null) return 1;
  if (b.deadline === null) return -1;
  const diff = new Date(a.deadline).getTime() - new Date(b.deadline).getTime();
  return direction === 'asc' ? diff : -diff;
}

export function compareByTitle(a: Task, b: Task, direction: SortDirection = 'asc'): number {
  const diff = a.title.localeCompare(b.title);
  return direction === 'asc' ? diff : -diff;
}

export function compareByStatus(a: Task, b: Task, direction: SortDirection = 'asc'): number {
  const diff = STATUS_ORDER[a.status] - STATUS_ORDER[b.status];
  // Use || 0 to normalise -0 to 0 (avoids Object.is(-0, 0) === false in tests)
  return (direction === 'asc' ? diff : -diff) || 0;
}

export function compareByLogged(a: Task, b: Task, direction: SortDirection = 'asc'): number {
  const diff = a.time_logged_minutes - b.time_logged_minutes;
  // Use || 0 to normalise -0 to 0 (avoids Object.is(-0, 0) === false in tests)
  return (direction === 'asc' ? diff : -diff) || 0;
}

const COMPARATORS: Record<SortField, (a: Task, b: Task, direction: SortDirection) => number> = {
  status: compareByStatus,
  priority: compareByPriority,
  deadline: compareByDeadline,
  title: compareByTitle,
  logged: compareByLogged,
};

/**
 * Sort a tasks array by a key stack: the first key whose comparison is
 * non-zero decides. Full ties keep the input (backend) order — Array#sort is
 * stable. Returns a new array, does not mutate the input.
 */
export function sortTasks(tasks: Task[], keys: readonly SortKey[]): Task[] {
  const copy = [...tasks];
  copy.sort((a, b) => {
    for (const { field, direction } of keys) {
      const diff = COMPARATORS[field](a, b, direction);
      if (diff !== 0) return diff;
    }
    return 0;
  });
  return copy;
}

/**
 * Apply a sort-bar click to the stack (returns a new stack):
 * - clicking the primary field toggles its direction;
 * - clicking any other field promotes it to primary with its default
 *   direction, removing its previous stack entry (no duplicate fields).
 */
export function clickSortField(stack: readonly SortKey[], field: SortField): SortKey[] {
  const [primary, ...rest] = stack;
  if (primary !== undefined && primary.field === field) {
    return [{ field, direction: primary.direction === 'asc' ? 'desc' : 'asc' }, ...rest];
  }
  return [
    { field, direction: defaultDirection(field) },
    ...stack.filter((key) => key.field !== field),
  ];
}
