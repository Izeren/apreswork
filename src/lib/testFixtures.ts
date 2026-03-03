// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Cross-layer test fixtures shared by unit tests in different feature folders.
// Feature-local fixtures live next to their tests (calendar/testFixtures.ts,
// tasks/testFixtures.ts); this root module holds primitives reused across layers
// so the same literal isn't re-declared per test file. calendar/testFixtures.ts
// imports chunkFixture from here rather than redefining it.

import { vi } from 'vitest';
import { tick } from 'svelte';
import { fireEvent, waitFor } from '@testing-library/svelte';
import type { AppConfig, Chunk, ScheduleWarning, SyncOutcome, TaskStatus } from './types';
import type { ToastMessage } from './stores/toast.svelte';

export const TEST_NOW = new Date('2026-01-01T12:00:00Z');

/** {status,label} pairs for every TaskStatus, in lifecycle order — for it.each over status badges. */
export const statusCases: Array<{ status: TaskStatus; label: string }> = [
  { status: 'backlog', label: 'Backlog' },
  { status: 'pending', label: 'Pending' },
  { status: 'scheduled', label: 'Scheduled' },
  { status: 'completed', label: 'Completed' },
  { status: 'cancelled', label: 'Cancelled' },
];

export function chunkFixture(overrides: Partial<Chunk> = {}): Chunk {
  return {
    id: 'chunk-1',
    task_id: 'task-1',
    start_time: '2026-03-28T12:00:00.000Z',
    end_time: '2026-03-28T13:00:00.000Z',
    status: 'scheduled',
    is_fixed: false,
    logged_minutes: null,
    completed_at: null,
    google_event_id: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

/** Canonical getConfig() resolved value; override any field. */
export function configFixture(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    planning_horizon_days: 30,
    timezone: 'UTC',
    max_continuous_minutes: 120,
    min_break_minutes: 5,
    last_reschedule: null,
    last_mutation: null,
    last_sync: null,
    last_busy_sync: null,
    ...overrides,
  };
}

/**
 * Flush a promise chain. Defaults to 3 links (api call → .then/.catch → .finally);
 * pass `count` when the chain under test is longer or shorter.
 * Shared by action-trigger tests (rescheduleTrigger, syncTrigger, chunkListLoad).
 */
export async function flushPromises(count = 3): Promise<void> {
  for (let i = 0; i < count; i++) {
    await Promise.resolve();
  }
}

export async function flushReactivity(count = 1): Promise<void> {
  for (let i = 0; i < count; i++) {
    await Promise.resolve();
    await tick();
  }
}

/** Canonical ScheduleWarning fixture; override any field. */
export function makeWarning(overrides: Partial<ScheduleWarning> = {}): ScheduleWarning {
  return {
    task_id: 't1',
    task_title: 'Task One',
    kind: { Unschedulable: { reason: 'no windows' } },
    ...overrides,
  };
}

function makeSyncResult(placedChunks: Chunk[]): SyncOutcome {
  return {
    schedule: { placed_chunks: placedChunks, warnings: [] },
    pushed: { created: placedChunks.length, updated: 0, deleted: 0 },
  };
}

/** SyncOutcome with one placed chunk and one created push event. */
export function syncSuccessResult(): SyncOutcome {
  return makeSyncResult([chunkFixture()]);
}

export function syncSuccessResultEmpty(): SyncOutcome {
  return makeSyncResult([]);
}

/** Collapses the two-assertion pattern (level + text) into one expect() call. */
export function lastToastError(items: ToastMessage[]): string | null {
  return items.find((t) => t.level === 'error')?.text ?? null;
}

/** Extract the boolean arg sequence from a busy-state spy (e.g. `[true, false]` after one call pair). */
export function busyCallSequence(busyMock: ReturnType<typeof vi.fn>): boolean[] {
  return busyMock.mock.calls.map((c) => c[0] as boolean);
}

/**
 * Shared DOMRect mock factory for time-menu scroll-center tests.
 * Returns a getBoundingClientRect implementation that reports realistic positions
 * for `.time-menu-list` and `.option-btn--active` based on the given measurements.
 */
export function makeTimeMenuGetBCR(opts: {
  listHeight: number;
  listBottom: number;
  activeTop: number;
  activeBottom: number;
  activeY: number;
}): (this: HTMLElement) => DOMRect {
  return function (this: HTMLElement): DOMRect {
    const base = { left: 0, right: 220, width: 220, x: 0 };
    let rect = { top: 0, ...base, bottom: 0, height: 0, y: 0 };
    if (this.classList.contains('time-menu-list')) {
      rect = { top: 50, ...base, bottom: opts.listBottom, height: opts.listHeight, y: 50 };
    } else if (this.classList.contains('option-btn--active')) {
      rect = {
        top: opts.activeTop,
        ...base,
        bottom: opts.activeBottom,
        height: 32,
        y: opts.activeY,
      };
    }
    return { ...rect, toJSON: () => rect } as DOMRect;
  };
}

export async function clickConfirmAndExpect(
  confirmBtn: Element | null,
  assertion: () => void,
): Promise<void> {
  await fireEvent.click(confirmBtn!);
  await waitFor(assertion);
}
