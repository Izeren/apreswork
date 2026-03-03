// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared fixtures for the ChunkBlock*.test.ts files. Not collected by vitest
// (no .test suffix). Share the standard cleanup lifecycle hook below.

import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/svelte';
import type { AgendaItem, Chunk } from '../../types';

/**
 * Build an ISO string at the given local hour:minute for 2026-03-28.
 * Uses local time so position calculations in the component match.
 */
export function localISO(hour: number, minute: number = 0): string {
  return new Date(2026, 2, 28, hour, minute, 0).toISOString();
}

/**
 * Build an AgendaItem. `chunk` overrides merge onto the default chunk, so
 * callers pass only the fields they care about — no `...baseItem().chunk` spread.
 */
export function baseItem(
  overrides: Partial<Omit<AgendaItem, 'chunk'>> & { chunk?: Partial<Chunk> } = {},
): AgendaItem {
  const { chunk, ...rest } = overrides;
  return {
    chunk: {
      id: 'chunk-1',
      task_id: 'task-1',
      start_time: localISO(9),
      end_time: localISO(10),
      status: 'scheduled',
      is_fixed: false,
      logged_minutes: null,
      completed_at: null,
      google_event_id: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      ...chunk,
    },
    task_title: 'Test Task',
    task_priority: 'Medium',
    task_labels: [],
    task_recurring_template_id: null,
    task_deadline: null,
    ...rest,
  };
}

export async function importChunkBlock() {
  const mod = await import('./ChunkBlock.svelte');
  return mod.default;
}

export function installChunkBlockCleanup(): void {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });
}
