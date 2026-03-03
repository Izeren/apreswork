// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { Task, CreateTaskInput, UpdateTaskInput, TaskFilter } from '../types';
import type { ToastMessage } from './toast.svelte';

function buildClient() {
  return {
    listTasks: vi.fn(),
    createTask: vi.fn(),
    updateTask: vi.fn(),
    deleteTask: vi.fn(),
  };
}

const mockTask = (overrides?: Partial<Task>): Task => ({
  id: 'task-1',
  title: 'Test Task',
  description: null,
  duration_minutes: 60,
  time_logged_minutes: 0,
  priority: 'Medium',
  status: 'pending',
  start_date: null,
  deadline: '2026-04-01T00:00:00Z',
  schedule_id: 'sched-1',
  min_chunk_minutes: 15,
  no_split: false,
  recurring_template_id: null,
  labels: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...overrides,
});

function expectToast(items: ToastMessage[], level: string, text: string) {
  expect(items).toHaveLength(1);
  expect(items[0].level).toBe(level);
  expect(items[0].text).toBe(text);
}

describe('TaskState', () => {
  let TaskState: typeof import('./tasks.svelte').TaskState;
  let toastState: import('./toast.svelte').ToastState;
  let tasks: InstanceType<typeof TaskState>;
  let client: ReturnType<typeof buildClient>;

  beforeEach(async () => {
    client = buildClient();
    const taskMod = await import('./tasks.svelte');
    const toastMod = await import('./toast.svelte');
    TaskState = taskMod.TaskState;
    toastState = toastMod.toastState;
    toastState.items = [];
    tasks = new TaskState(client);
  });

  describe('load', () => {
    it('populates items and manages loading states', async () => {
      const task1 = mockTask();
      const task2 = mockTask({ id: 'task-2', title: 'Task Two' });
      client.listTasks.mockResolvedValue([task1, task2]);

      expect(tasks.loading).toBe(false);
      const promise = tasks.load();
      expect(tasks.loading).toBe(true);
      await promise;
      expect(tasks.loading).toBe(false);
      expect(tasks.items).toEqual([task1, task2]);
    });

    it('shows toast and sets loading false on error, items unchanged', async () => {
      const existing = mockTask();
      tasks.items = [existing];
      client.listTasks.mockRejectedValue(new Error('network'));

      await tasks.load();

      expect(tasks.loading).toBe(false);
      expect(tasks.items).toEqual([existing]);
      expectToast(toastState.items, 'error', 'Failed to load tasks');
    });

    it('passes current filter to API', async () => {
      const filter: TaskFilter = { statuses: ['pending'], priorities: ['High'] };
      tasks.setFilter(filter);
      client.listTasks.mockResolvedValue([]);

      await tasks.load();

      expect(client.listTasks).toHaveBeenCalledWith(filter);
    });
  });

  describe('create', () => {
    it('adds returned task to items and shows success toast', async () => {
      const task = mockTask();
      client.createTask.mockResolvedValue(task);
      const input: CreateTaskInput = {
        title: 'Test Task',
        duration_minutes: 60,
        deadline: '2026-04-01T00:00:00Z',
      };

      const result = await tasks.create(input);

      expect(result).toEqual(task);
      expect(tasks.items).toEqual([task]);
      expectToast(toastState.items, 'success', 'Task created');
    });

    it('shows error toast and returns undefined on error, items unchanged', async () => {
      client.createTask.mockRejectedValue(new Error('fail'));
      const input: CreateTaskInput = {
        title: 'Bad Task',
        duration_minutes: 30,
        deadline: '2026-04-01T00:00:00Z',
      };

      const result = await tasks.create(input);

      expect(result).toBeUndefined();
      expect(tasks.items).toEqual([]);
      expectToast(toastState.items, 'error', 'Failed to create task');
    });
  });

  describe('update (optimistic)', () => {
    it('optimistically applies changes then replaces with server version', async () => {
      const task = mockTask();
      tasks.items = [task];

      const serverVersion = mockTask({ title: 'Server Title', updated_at: '2026-02-01T00:00:00Z' });
      client.updateTask.mockResolvedValue(serverVersion);

      const input: UpdateTaskInput = { title: 'Server Title' };
      const promise = tasks.update('task-1', input);

      expect(tasks.items[0].title).toBe('Server Title');

      await promise;

      expect(tasks.items[0]).toEqual(serverVersion);
      expectToast(toastState.items, 'success', 'Task updated');
    });

    it('rolls back to snapshot on error and shows error toast', async () => {
      const task = mockTask({ title: 'Original' });
      tasks.items = [task];

      client.updateTask.mockRejectedValue(new Error('fail'));

      const input: UpdateTaskInput = { title: 'New Title' };
      await tasks.update('task-1', input);

      expect(tasks.items[0].title).toBe('Original');
      expectToast(toastState.items, 'error', 'Failed to update task');
    });

    it('surfaces the backend message when the rejection is a validation error', async () => {
      tasks.items = [mockTask()];
      client.updateTask.mockRejectedValue({
        error: 'validation',
        message: 'deadline cannot be in the past',
      });

      await tasks.update('task-1', { title: 'New Title' });

      expectToast(toastState.items, 'error', 'deadline cannot be in the past');
    });
  });

  describe('remove (optimistic)', () => {
    it('optimistically removes item', async () => {
      const task1 = mockTask();
      const task2 = mockTask({ id: 'task-2' });
      tasks.items = [task1, task2];
      client.deleteTask.mockResolvedValue(undefined);

      await tasks.remove('task-1');

      expect(tasks.items).toEqual([task2]);
      expectToast(toastState.items, 'success', 'Task deleted');
    });

    it('rolls back on error and shows error toast', async () => {
      const task = mockTask();
      tasks.items = [task];
      client.deleteTask.mockRejectedValue(new Error('fail'));

      await tasks.remove('task-1');

      expect(tasks.items).toEqual([task]);
      expectToast(toastState.items, 'error', 'Failed to delete task');
    });

    it.each<{ label: string; selectedId: string; expectedSelectedId: string | null }>([
      {
        label: 'clears selectedId if deleted task was selected',
        selectedId: 'task-1',
        expectedSelectedId: null,
      },
      {
        label: 'does not affect selectedId when removing a non-selected task',
        selectedId: 'task-2',
        expectedSelectedId: 'task-2',
      },
    ])('$label', async ({ selectedId, expectedSelectedId }) => {
      const task1 = mockTask();
      const task2 = mockTask({ id: 'task-2' });
      tasks.items = [task1, task2];
      tasks.select(selectedId);
      client.deleteTask.mockResolvedValue(undefined);

      await tasks.remove('task-1');

      expect(tasks.selectedId).toBe(expectedSelectedId);
    });
  });

  describe('select', () => {
    it.each<{ id: string | null; expected: string | null }>([
      { id: 'task-1', expected: 'task-1' },
      { id: null, expected: null },
    ])('sets selectedId to $expected', ({ id, expected }) => {
      tasks.select(id);
      expect(tasks.selectedId).toBe(expected);
    });
  });

  describe('requestTemplateEdit', () => {
    it('stores the requested template id and increments the nonce', () => {
      expect(tasks.templateEditRequestNonce).toBe(0);

      tasks.requestTemplateEdit('tpl-1');

      expect(tasks.templateEditRequestId).toBe('tpl-1');
      expect(tasks.templateEditRequestNonce).toBe(1);
    });

    it('clears the request id but keeps the nonce', () => {
      tasks.requestTemplateEdit('tpl-1');

      tasks.clearTemplateEditRequest();

      expect(tasks.templateEditRequestId).toBeNull();
      expect(tasks.templateEditRequestNonce).toBe(1);
    });
  });

  describe('selected (derived)', () => {
    it.each<{ label: string; selectedId: string | null; expected: Task | undefined }>([
      { label: 'matching task', selectedId: 'task-1', expected: mockTask() },
      { label: 'undefined for null selectedId', selectedId: null, expected: undefined },
      {
        label: 'undefined for nonexistent selectedId',
        selectedId: 'nonexistent',
        expected: undefined,
      },
    ])('returns $label', ({ selectedId, expected }) => {
      tasks.items = [mockTask()];
      tasks.select(selectedId);
      expect(tasks.selected).toEqual(expected);
    });
  });

  describe('setFilter', () => {
    it('updates filter state', () => {
      const filter: TaskFilter = { statuses: ['pending', 'scheduled'], labels: ['work'] };
      tasks.setFilter(filter);
      expect(tasks.filter).toEqual(filter);
    });
  });

  describe('reset', () => {
    it('drops all profile-scoped state but keeps the nonces monotonic', () => {
      tasks.items = [mockTask()];
      tasks.loading = true;
      tasks.select('task-1');
      tasks.requestTemplateEdit('tpl-1');
      tasks.setFilter({ statuses: ['pending'] });
      const editNonce = tasks.templateEditRequestNonce;

      tasks.reset();

      expect(tasks.items).toEqual([]);
      expect(tasks.loading).toBe(false);
      expect(tasks.selectedId).toBeNull();
      expect(tasks.templateEditRequestId).toBeNull();
      expect(tasks.filter).toEqual({});
      expect(tasks.templateEditRequestNonce).toBe(editNonce);
    });
  });
});
