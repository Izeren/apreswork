// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared harness for the TaskForm-family test files (TaskForm.test,
// TaskForm.comments.test, TaskForm.dirty.test, TaskForm.chunks.test,
// SharedFormFields.test). Not collected by vitest (no .test suffix).

import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, vi } from 'vitest';
import type { Mocked } from 'vitest';
import { toastState } from '../../stores/toast.svelte';
import type { ComponentProps } from 'svelte';
import type { Schedule, Task } from '../../types';
import type { TaskFormApi } from './taskFormShared';
import { baseChunk, baseComment } from './testFixtures';
import { default as TaskForm } from './TaskForm.svelte';

export type TaskFormProps = ComponentProps<typeof TaskForm>;

export type { TaskFormApi };

export const ISO_START = '2026-05-01T09:00:00.000Z';
export const ISO_DEADLINE = '2026-06-15T12:00:00.000Z';

/** Two schedules the form's schedule dropdown renders from `scheduleState`. */
export const demoSchedules: Schedule[] = [
  {
    id: 'sched-1',
    name: 'Work Week',
    is_default: true,
    windows: [],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'sched-2',
    name: 'Weekend',
    is_default: false,
    windows: [],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
];

/** A fully-populated task used to exercise TaskForm edit-mode pre-fill. */
export function formTask(overrides: Partial<Task> = {}): Task {
  return {
    id: 'task-1',
    title: 'My Task',
    description: 'A description',
    duration_minutes: 90,
    time_logged_minutes: 0,
    priority: 'High',
    status: 'pending',
    start_date: ISO_START,
    deadline: ISO_DEADLINE,
    schedule_id: 'sched-1',
    min_chunk_minutes: 30,
    no_split: true,
    recurring_template_id: null,
    labels: ['backend', 'urgent'],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

export async function setInputValue(el: HTMLInputElement, value: string) {
  el.value = value;
  await fireEvent.input(el);
}

export async function setSelectValue(el: HTMLSelectElement, value: string) {
  el.value = value;
  await fireEvent.change(el);
}

/** Find a DateTimePicker's trigger button by its visible field label (e.g. "Start date"). */
export function getDateTimeTrigger(container: HTMLElement, label: string): HTMLButtonElement {
  const picker = Array.from(container.querySelectorAll('.datetime-picker')).find((p) =>
    p.querySelector('.picker-label')?.textContent?.trim().startsWith(label),
  );
  if (!picker) throw new Error(`Missing date time picker with label "${label}"`);
  return picker.querySelector('button[aria-haspopup="dialog"]') as HTMLButtonElement;
}

export function taskFormFakeApi(): Mocked<TaskFormApi> {
  return {
    listComments: vi.fn<TaskFormApi['listComments']>().mockResolvedValue([]),
    createComment: vi.fn<TaskFormApi['createComment']>().mockResolvedValue(baseComment()),
    updateComment: vi.fn<TaskFormApi['updateComment']>().mockResolvedValue(baseComment()),
    deleteComment: vi.fn<TaskFormApi['deleteComment']>().mockResolvedValue(undefined),
    listChunksForTask: vi.fn<TaskFormApi['listChunksForTask']>().mockResolvedValue([]),
    unlockChunk: vi.fn<TaskFormApi['unlockChunk']>().mockResolvedValue(baseChunk()),
    deleteFixedChunk: vi.fn<TaskFormApi['deleteFixedChunk']>().mockResolvedValue(baseChunk()),
  };
}

/**
 * Render `TaskForm` open, with default `onsubmit`/`onclose` spies, then flush one
 * tick. Pass a fresh `taskFormFakeApi()` as `fake`; pass `props` to override
 * defaults or add `task` / `initialStartDate` / `onmakerecurring`.
 */
export async function renderTaskForm(
  fake: Mocked<TaskFormApi>,
  props: Partial<TaskFormProps> = {},
) {
  const result = render(TaskForm, {
    open: true,
    onsubmit: vi.fn(),
    onclose: vi.fn(),
    apiClient: fake,
    ...props,
  });
  await tick();
  return result;
}

export function installTaskFormHooks() {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    toastState.items = [];
  });
}
