// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared entity fixtures for the task-component test files. Not collected by
// vitest (no .test suffix).

import type { Chunk, Comment, Task, RecurringTemplate, Schedule, Priority } from '../../types';

export { TEST_NOW } from '../../testFixtures';

export const priorityCases: Array<{ priority: Priority }> = [
  { priority: 'Low' },
  { priority: 'Medium' },
  { priority: 'High' },
  { priority: 'Critical' },
];

export const baseComment = (overrides: Partial<Comment> = {}): Comment => ({
  id: 'comment-1',
  task_id: 'task-1',
  author: 'User',
  content: 'First thoughts',
  created_at: '2026-01-01T10:00:00Z',
  updated_at: '2026-01-01T10:00:00Z',
  ...overrides,
});

export const baseTask = (overrides: Partial<Task> = {}): Task => ({
  id: 'task-1',
  title: 'Alpha task',
  description: null,
  duration_minutes: 60,
  time_logged_minutes: 0,
  priority: 'Medium',
  status: 'pending',
  start_date: null,
  deadline: '2026-06-01T00:00:00Z',
  schedule_id: 'sched-1',
  min_chunk_minutes: 15,
  no_split: false,
  recurring_template_id: null,
  labels: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...overrides,
});

export const baseChunk = (overrides: Partial<Chunk> = {}): Chunk => ({
  id: 'chunk-1',
  task_id: 'task-1',
  start_time: '2026-05-10T09:00:00.000Z',
  end_time: '2026-05-10T10:00:00.000Z',
  status: 'scheduled',
  is_fixed: false,
  logged_minutes: null,
  completed_at: null,
  google_event_id: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...overrides,
});

export const baseTemplate = (overrides: Partial<RecurringTemplate> = {}): RecurringTemplate => ({
  id: 'template-1',
  title: 'Weekly review',
  description: 'Keep the loop closed',
  duration_minutes: 45,
  priority: 'High',
  schedule_id: 'sched-1',
  cadence: {
    period: 'Weekly',
    interval: 1,
    windows: [
      { start: 0, end: 0 },
      { start: 3, end: 3 },
    ],
  },
  labels: ['ops'],
  is_active: true,
  start_date: '2026-01-01T00:00:00Z',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-02T00:00:00Z',
  ...overrides,
});

export const baseSchedule = (overrides: Partial<Schedule> = {}): Schedule => ({
  id: 'sched-1',
  name: 'Work Hours',
  is_default: true,
  windows: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...overrides,
});
