// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi, type MockedFunction } from 'vitest';
import { chunkFixture as mockChunk, TEST_NOW } from '../testFixtures';
import type { AgendaItem, Chunk, Task } from '../types';
import {
  TaskActions,
  chunkContextMenuItems,
  deadlineExtendItems,
  taskContextMenuItems,
} from './taskActions';
import { todayDeadline } from '../components/shared/deadlinePresets';
import type { ConfirmSpec, TaskActionsApiSubset } from './taskActions';
import { toastState } from '../stores/toast.svelte';
import { apiErrorMessage } from '../api';
import { formatDateTime } from '../utils';

type FakeTaskActionsApi = {
  completeTask: MockedFunction<NonNullable<TaskActionsApiSubset['completeTask']>>;
  cancelTask: MockedFunction<NonNullable<TaskActionsApiSubset['cancelTask']>>;
  getTask: MockedFunction<NonNullable<TaskActionsApiSubset['getTask']>>;
  listChunksForTask: MockedFunction<NonNullable<TaskActionsApiSubset['listChunksForTask']>>;
  createFixedChunk: MockedFunction<NonNullable<TaskActionsApiSubset['createFixedChunk']>>;
  updateTask: MockedFunction<NonNullable<TaskActionsApiSubset['updateTask']>>;
  deleteTask: MockedFunction<NonNullable<TaskActionsApiSubset['deleteTask']>>;
  apiErrorMessage: NonNullable<TaskActionsApiSubset['apiErrorMessage']>;
  completeChunk: MockedFunction<NonNullable<TaskActionsApiSubset['completeChunk']>>;
  reopenChunk: MockedFunction<NonNullable<TaskActionsApiSubset['reopenChunk']>>;
  lockChunk: MockedFunction<NonNullable<TaskActionsApiSubset['lockChunk']>>;
  unlockChunk: MockedFunction<NonNullable<TaskActionsApiSubset['unlockChunk']>>;
  deleteFixedChunk: MockedFunction<NonNullable<TaskActionsApiSubset['deleteFixedChunk']>>;
};

type TaskActionsT = InstanceType<typeof TaskActions>;

const mockItem = (
  chunkOverrides?: Partial<Chunk>,
  templateId: string | null = null,
): AgendaItem => ({
  chunk: mockChunk(chunkOverrides),
  task_title: 'My task',
  task_priority: 'Medium',
  task_labels: [],
  task_recurring_template_id: templateId,
  task_deadline: null,
});

const mockTask = (overrides?: Partial<Task>): Task => ({
  id: 'task-1',
  title: 'My task',
  description: null,
  duration_minutes: 60,
  time_logged_minutes: 0,
  priority: 'Medium',
  status: 'scheduled',
  start_date: null,
  deadline: '2026-08-01T00:00:00Z',
  schedule_id: 'sched-1',
  min_chunk_minutes: 15,
  no_split: false,
  recurring_template_id: null,
  labels: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...overrides,
});

function okOrFail(
  m: MockedFunction<(...args: never[]) => Promise<unknown>>,
  ok: boolean,
  resolved: unknown,
): void {
  if (ok) m.mockResolvedValue(resolved);
  else m.mockRejectedValue(new Error('network'));
}

describe('TaskActions', () => {
  let fakeApi: FakeTaskActionsApi;
  let host: {
    refresh: MockedFunction<() => void>;
    confirm: MockedFunction<(spec: ConfirmSpec) => Promise<boolean>>;
    openTaskEditor: MockedFunction<(taskId: string) => void>;
    openTemplateEditor: MockedFunction<(templateId: string) => void>;
  };
  let actions: TaskActionsT;

  const simpleVerbs = () => [
    {
      name: 'completeChunk',
      setup: (ok: boolean) => okOrFail(fakeApi.completeChunk, ok, [mockChunk(), mockTask()]),
      invoke: (a: TaskActionsT) => a.completeChunk('chunk-1'),
      assertCall: () => expect(fakeApi.completeChunk).toHaveBeenCalledWith('chunk-1'),
      success: 'Chunk completed',
      failure: 'Failed to complete chunk',
    },
    {
      name: 'reopenChunk',
      setup: (ok: boolean) => okOrFail(fakeApi.reopenChunk, ok, [mockChunk(), mockTask()]),
      invoke: (a: TaskActionsT) => a.reopenChunk('chunk-1'),
      assertCall: () => expect(fakeApi.reopenChunk).toHaveBeenCalledWith('chunk-1'),
      success: 'Chunk reopened',
      failure: 'Failed to reopen chunk',
    },
    {
      name: 'lockChunk',
      setup: (ok: boolean) => okOrFail(fakeApi.lockChunk, ok, mockChunk({ is_fixed: true })),
      invoke: (a: TaskActionsT) => a.lockChunk('chunk-1'),
      assertCall: () => expect(fakeApi.lockChunk).toHaveBeenCalledWith('chunk-1'),
      success: 'Chunk locked',
      failure: 'Failed to lock chunk',
    },
    {
      name: 'unlockChunk',
      setup: (ok: boolean) => okOrFail(fakeApi.unlockChunk, ok, mockChunk()),
      invoke: (a: TaskActionsT) => a.unlockChunk('chunk-1'),
      assertCall: () => expect(fakeApi.unlockChunk).toHaveBeenCalledWith('chunk-1'),
      success: 'Chunk unlocked',
      failure: 'Failed to unlock chunk',
    },
    {
      name: 'deleteFixedChunk',
      setup: (ok: boolean) => okOrFail(fakeApi.deleteFixedChunk, ok, mockChunk({ is_fixed: true })),
      invoke: (a: TaskActionsT) => a.deleteFixedChunk('chunk-1'),
      assertCall: () => expect(fakeApi.deleteFixedChunk).toHaveBeenCalledWith('chunk-1'),
      success: 'Fixed chunk deleted',
      failure: 'Failed to delete chunk',
    },
    {
      name: 'extendDeadline',
      setup: (ok: boolean) => okOrFail(fakeApi.updateTask, ok, mockTask()),
      invoke: (a: TaskActionsT) => a.extendDeadline('task-1', '2026-09-01T00:00:00Z'),
      assertCall: () =>
        expect(fakeApi.updateTask).toHaveBeenCalledWith('task-1', {
          deadline: '2026-09-01T00:00:00Z',
        }),
      success: 'Deadline updated',
      failure: 'Failed to update deadline',
    },
    {
      name: 'toBacklog',
      setup: (ok: boolean) => okOrFail(fakeApi.updateTask, ok, mockTask({ status: 'backlog' })),
      invoke: (a: TaskActionsT) => a.toBacklog('task-1'),
      assertCall: () =>
        expect(fakeApi.updateTask).toHaveBeenCalledWith('task-1', { status: 'backlog' }),
      success: 'Task moved to backlog',
      failure: 'Failed to move task to backlog',
    },
    {
      name: 'activate',
      setup: (ok: boolean) => okOrFail(fakeApi.updateTask, ok, mockTask({ status: 'pending' })),
      invoke: (a: TaskActionsT) => a.activate('task-1'),
      assertCall: () =>
        expect(fakeApi.updateTask).toHaveBeenCalledWith('task-1', { status: 'pending' }),
      success: 'Task activated',
      failure: 'Failed to activate task',
    },
  ];

  // All fakeApi accesses are inside closures — called after beforeEach, not at
  // describe-body evaluation time (when fakeApi is still undefined).
  const confirmedVerbs = () => [
    {
      name: 'completeTask',
      setup: () => fakeApi.completeTask.mockResolvedValue(mockTask({ status: 'completed' })),
      invoke: (a: TaskActionsT) => a.completeTask('task-1', 'My task'),
      assertCall: () => expect(fakeApi.completeTask).toHaveBeenCalledWith('task-1'),
      assertNotCalled: () => expect(fakeApi.completeTask).not.toHaveBeenCalled(),
      success: 'Task completed',
      destructive: false,
    },
    {
      name: 'cancelTask',
      setup: () => fakeApi.cancelTask.mockResolvedValue(mockTask({ status: 'cancelled' })),
      invoke: (a: TaskActionsT) => a.cancelTask('task-1', 'My task'),
      assertCall: () => expect(fakeApi.cancelTask).toHaveBeenCalledWith('task-1'),
      assertNotCalled: () => expect(fakeApi.cancelTask).not.toHaveBeenCalled(),
      success: 'Task cancelled',
      destructive: true,
    },
    {
      name: 'deleteTask',
      setup: () => fakeApi.deleteTask.mockResolvedValue(undefined),
      invoke: (a: TaskActionsT) => a.deleteTask('task-1', 'My task', false),
      assertCall: () => expect(fakeApi.deleteTask).toHaveBeenCalledWith('task-1'),
      assertNotCalled: () => expect(fakeApi.deleteTask).not.toHaveBeenCalled(),
      success: 'Task deleted',
      destructive: true,
    },
  ];

  beforeEach(() => {
    fakeApi = {
      completeTask: vi.fn(),
      cancelTask: vi.fn(),
      getTask: vi.fn(),
      listChunksForTask: vi.fn(),
      createFixedChunk: vi.fn(),
      updateTask: vi.fn(),
      deleteTask: vi.fn(),
      apiErrorMessage: (e, f) => apiErrorMessage(e, f),
      completeChunk: vi.fn(),
      reopenChunk: vi.fn(),
      lockChunk: vi.fn(),
      unlockChunk: vi.fn(),
      deleteFixedChunk: vi.fn(),
    };
    toastState.reset();
    host = {
      refresh: vi.fn<() => void>(),
      confirm: vi.fn<(spec: ConfirmSpec) => Promise<boolean>>(async () => true),
      openTaskEditor: vi.fn<(taskId: string) => void>(),
      openTemplateEditor: vi.fn<(templateId: string) => void>(),
    };
    actions = new TaskActions(host, fakeApi);
  });

  describe('simple verbs', () => {
    it.each(simpleVerbs())('$name: calls the api, toasts, refreshes', async (row) => {
      row.setup(true);

      await row.invoke(actions);

      row.assertCall();
      expect(toastState.items[0]?.level).toBe('success');
      expect(toastState.items[0]?.text).toBe(row.success);
      expect(host.refresh).toHaveBeenCalledOnce();
    });

    it.each(simpleVerbs())('$name: failure toasts and skips refresh', async (row) => {
      row.setup(false);

      await row.invoke(actions);

      expect(toastState.items[0]?.level).toBe('error');
      expect(toastState.items[0]?.text).toBe(row.failure);
      expect(host.refresh).not.toHaveBeenCalled();
    });

    it('surfaces backend validation messages verbatim', async () => {
      fakeApi.lockChunk.mockRejectedValue({
        error: 'validation',
        message: 'completed chunks cannot be locked',
      });

      await actions.lockChunk('chunk-1');

      expect(toastState.items[0]?.text).toBe('completed chunks cannot be locked');
    });
  });

  describe('confirmed verbs', () => {
    it.each(confirmedVerbs())('$name: proceeds after confirmation', async (row) => {
      row.setup();

      await row.invoke(actions);

      expect(host.confirm).toHaveBeenCalledOnce();
      const spec = host.confirm.mock.calls[0]?.[0];
      expect(spec?.message).toContain('My task');
      expect(spec?.destructive).toBe(row.destructive);
      row.assertCall();
      expect(toastState.items[0]?.text).toBe(row.success);
      expect(host.refresh).toHaveBeenCalledOnce();
    });

    it.each(confirmedVerbs())('$name: does nothing when declined', async (row) => {
      row.setup();
      host.confirm.mockResolvedValue(false);

      await row.invoke(actions);

      row.assertNotCalled();
      expect(toastState.items).toHaveLength(0);
      expect(host.refresh).not.toHaveBeenCalled();
    });

    it('deleteTask warns about occurrence-cancel semantics for recurring instances', async () => {
      fakeApi.deleteTask.mockResolvedValue(undefined);

      await actions.deleteTask('task-1', 'My task', true);

      const spec = host.confirm.mock.calls[0]?.[0];
      expect(spec?.message).toMatch(/occurrence/i);
      expect(spec?.message).toMatch(/template/i);
      expect(spec?.destructive).toBe(true);
    });
  });

  describe('doNow', () => {
    it('creates a fixed chunk from now for the remaining minutes', async () => {
      fakeApi.getTask.mockResolvedValue(
        mockTask({ duration_minutes: 120, time_logged_minutes: 30 }),
      );
      // Fixed chunks of any status count against the budget; auto chunks don't.
      fakeApi.listChunksForTask.mockResolvedValue([
        mockChunk({
          id: 'c-fixed',
          is_fixed: true,
          start_time: '2026-07-08T10:00:00.000Z',
          end_time: '2026-07-08T10:30:00.000Z',
        }),
        mockChunk({
          id: 'c-fixed-done',
          is_fixed: true,
          status: 'completed',
          start_time: '2026-07-06T10:00:00.000Z',
          end_time: '2026-07-06T10:15:00.000Z',
        }),
        mockChunk({
          id: 'c-auto',
          is_fixed: false,
          start_time: '2026-07-09T10:00:00.000Z',
          end_time: '2026-07-09T11:00:00.000Z',
        }),
      ]);
      fakeApi.createFixedChunk.mockResolvedValue([mockChunk(), mockTask()]);

      await actions.doNow('task-1', TEST_NOW);

      // remaining = 120 − 30 logged − (30 + 15) fixed = 45
      expect(fakeApi.createFixedChunk).toHaveBeenCalledWith(
        'task-1',
        '2026-01-01T12:00:00.000Z',
        '2026-01-01T12:45:00.000Z',
      );
      expect(toastState.items[0]?.text).toBe('Scheduled to start now');
      expect(host.refresh).toHaveBeenCalledOnce();
    });

    it('rejects when nothing remains to schedule', async () => {
      fakeApi.getTask.mockResolvedValue(
        mockTask({ duration_minutes: 60, time_logged_minutes: 60 }),
      );
      fakeApi.listChunksForTask.mockResolvedValue([]);

      await actions.doNow('task-1', TEST_NOW);

      expect(fakeApi.createFixedChunk).not.toHaveBeenCalled();
      expect(toastState.items[0]?.level).toBe('error');
      expect(host.refresh).not.toHaveBeenCalled();
    });

    it.each([
      {
        label: 'getTask rejects',
        setup: () => {
          fakeApi.getTask.mockRejectedValue(new Error('network'));
        },
      },
      {
        label: 'createFixedChunk rejects after pre-check succeeds',
        setup: () => {
          fakeApi.getTask.mockResolvedValue(mockTask({ duration_minutes: 60 }));
          fakeApi.listChunksForTask.mockResolvedValue([]);
          fakeApi.createFixedChunk.mockRejectedValue(new Error('network'));
        },
      },
    ])('shows a single error toast when $label', async ({ setup }) => {
      setup();

      await actions.doNow('task-1', TEST_NOW);

      expect(toastState.items).toHaveLength(1);
      expect(toastState.items[0]?.level).toBe('error');
      expect(toastState.items[0]?.text).toBe('Failed to schedule task');
      expect(host.refresh).not.toHaveBeenCalled();
    });
  });

  describe('editor delegation', () => {
    it('editTask opens the host task editor', () => {
      actions.editTask('task-1');
      expect(host.openTaskEditor).toHaveBeenCalledWith('task-1');
    });

    it('editTemplate opens the host template editor', () => {
      actions.editTemplate('tpl-1');
      expect(host.openTemplateEditor).toHaveBeenCalledWith('tpl-1');
    });
  });

  describe('deadlineExtendItems', () => {
    it('returns four items with exact label format', () => {
      const items = deadlineExtendItems('task-1', actions, TEST_NOW);
      expect(items).toHaveLength(4);
      expect(items[0]?.label).toBe(`Extend to today (${formatDateTime(todayDeadline(TEST_NOW))})`);
    });
  });

  describe('chunkContextMenuItems', () => {
    it.each([
      {
        name: 'scheduled auto chunk',
        item: () => mockItem(),
        expected: [
          'Complete chunk',
          'Complete task',
          'Do now',
          'Lock chunk',
          'Edit task',
          'Cancel task',
        ],
        expectedDestructive: ['Cancel task'],
      },
      {
        name: 'scheduled fixed chunk',
        item: () => mockItem({ is_fixed: true }),
        expected: [
          'Complete chunk',
          'Complete task',
          'Do now',
          'Unlock chunk',
          'Delete fixed chunk',
          'Edit task',
          'Cancel task',
        ],
        expectedDestructive: ['Delete fixed chunk', 'Cancel task'],
      },
      {
        name: 'completed chunk',
        item: () => mockItem({ status: 'completed' }),
        expected: ['Reopen chunk', 'Edit task'],
        expectedDestructive: [],
      },
      {
        name: 'recurring instance chunk',
        item: () => mockItem({}, 'tpl-1'),
        expected: [
          'Complete chunk',
          'Complete task',
          'Do now',
          'Lock chunk',
          'Edit task',
          'Edit template',
          'Cancel task',
        ],
        expectedDestructive: ['Cancel task'],
      },
    ])(
      '$name: offers the state-appropriate verbs and marks removals destructive',
      ({ item, expected, expectedDestructive }) => {
        const items = chunkContextMenuItems(item(), actions, TEST_NOW);
        expect(items.map((i) => i.label)).toEqual(expected);
        expect(items.filter((e) => Boolean(e.destructive)).map((e) => e.label)).toEqual(
          expectedDestructive,
        );
      },
    );

    it.each([
      {
        name: 'Lock chunk',
        setup: () => fakeApi.lockChunk.mockResolvedValue(mockChunk({ is_fixed: true })),
        item: () => mockItem(),
        api: () => fakeApi.lockChunk,
      },
      {
        name: 'Delete fixed chunk',
        setup: () => fakeApi.deleteFixedChunk.mockResolvedValue(mockChunk({ is_fixed: true })),
        item: () => mockItem({ is_fixed: true }),
        api: () => fakeApi.deleteFixedChunk,
      },
    ])('wires $name to the chunk id', async ({ setup, item, api, name }) => {
      setup();
      const items = chunkContextMenuItems(item(), actions, TEST_NOW);
      await items.find((i) => i.label === name)?.action?.();
      expect(api()).toHaveBeenCalledWith('chunk-1');
    });

    it('wires Edit template to the instance template id', () => {
      const items = chunkContextMenuItems(mockItem({}, 'tpl-1'), actions, TEST_NOW);

      void items.find((i) => i.label === 'Edit template')?.action?.();

      expect(host.openTemplateEditor).toHaveBeenCalledWith('tpl-1');
    });

    describe('deadline-extend items', () => {
      const PAST_DEADLINE = '2026-01-01T00:00:00Z';
      const FUTURE_DEADLINE = '2026-02-15T00:00:00Z';

      it.each([
        { label: 'null deadline', item: (): AgendaItem => mockItem() },
        {
          label: 'future deadline',
          item: (): AgendaItem => ({ ...mockItem(), task_deadline: FUTURE_DEADLINE }),
        },
      ])('$label: no Extend-to items appear', ({ item }) => {
        const labels = chunkContextMenuItems(item(), actions, TEST_NOW).map((i) => i.label);
        expect(labels.filter((l) => l.startsWith('Extend to'))).toHaveLength(0);
      });

      it('past deadline: adds four extend items in order', () => {
        const item: AgendaItem = { ...mockItem(), task_deadline: PAST_DEADLINE };
        const labels = chunkContextMenuItems(item, actions, TEST_NOW).map((i) => i.label);
        const extendLabels = labels.filter((l) => l.startsWith('Extend to'));
        expect(extendLabels).toHaveLength(4);
        expect(extendLabels[0]).toMatch(/today/i);
        expect(extendLabels[1]).toMatch(/tomorrow/i);
        expect(extendLabels[2]).toMatch(/next week/i);
        expect(extendLabels[3]).toMatch(/next month/i);
      });

      it('clicking Extend to today calls extendDeadline with todayDeadline(now)', async () => {
        const item: AgendaItem = { ...mockItem(), task_deadline: PAST_DEADLINE };
        fakeApi.updateTask.mockResolvedValue(mockTask());
        const items = chunkContextMenuItems(item, actions, TEST_NOW);
        const todayItem = items.find((i) => i.label.startsWith('Extend to today'));
        await todayItem?.action?.();
        expect(fakeApi.updateTask).toHaveBeenCalledWith('task-1', {
          deadline: todayDeadline(TEST_NOW),
        });
      });
    });
  });

  describe('taskContextMenuItems', () => {
    it.each([
      {
        name: 'backlog task',
        task: () => mockTask({ status: 'backlog' }),
        expected: ['Activate', 'Edit task', 'Cancel task', 'Delete task'],
      },
      {
        name: 'pending task',
        task: () => mockTask({ status: 'pending' }),
        expected: ['Do now', 'Move to backlog', 'Edit task', 'Cancel task', 'Delete task'],
      },
      {
        name: 'scheduled task',
        task: () => mockTask({ status: 'scheduled' }),
        expected: [
          'Complete task',
          'Do now',
          'Move to backlog',
          'Edit task',
          'Cancel task',
          'Delete task',
        ],
      },
      {
        name: 'completed task',
        task: () => mockTask({ status: 'completed' }),
        expected: ['Edit task', 'Delete task'],
      },
      {
        name: 'cancelled task',
        task: () => mockTask({ status: 'cancelled' }),
        expected: ['Edit task', 'Delete task'],
      },
      {
        name: 'scheduled recurring instance',
        task: () => mockTask({ status: 'scheduled', recurring_template_id: 'tpl-1' }),
        expected: [
          'Complete task',
          'Do now',
          'Move to backlog',
          'Edit task',
          'Edit template',
          'Cancel task',
          'Delete task',
        ],
      },
    ])('$name: offers the state-appropriate verbs', ({ task, expected }) => {
      const labels = taskContextMenuItems(task(), actions, TEST_NOW).map((i) => i.label);
      expect(labels).toEqual(expected);
    });

    it('marks only Cancel task and Delete task destructive', () => {
      const actualDestructive = taskContextMenuItems(
        mockTask({ status: 'scheduled' }),
        actions,
        TEST_NOW,
      )
        .filter((e) => Boolean(e.destructive))
        .map((e) => e.label);
      expect(actualDestructive).toEqual(['Cancel task', 'Delete task']);
    });

    it('wires task verbs to the task id', async () => {
      fakeApi.updateTask.mockResolvedValue(mockTask({ status: 'backlog' }));
      const items = taskContextMenuItems(mockTask({ status: 'scheduled' }), actions, TEST_NOW);

      await items.find((i) => i.label === 'Move to backlog')?.action?.();

      expect(fakeApi.updateTask).toHaveBeenCalledWith('task-1', { status: 'backlog' });
    });

    it('wires Edit template to the template id', () => {
      const items = taskContextMenuItems(
        mockTask({ status: 'scheduled', recurring_template_id: 'tpl-1' }),
        actions,
        TEST_NOW,
      );

      void items.find((i) => i.label === 'Edit template')?.action?.();

      expect(host.openTemplateEditor).toHaveBeenCalledWith('tpl-1');
    });

    it('passes recurring-instance semantics to deleteTask', async () => {
      fakeApi.deleteTask.mockResolvedValue(undefined);
      const items = taskContextMenuItems(
        mockTask({ status: 'scheduled', recurring_template_id: 'tpl-1' }),
        actions,
        TEST_NOW,
      );

      await items.find((i) => i.label === 'Delete task')?.action?.();

      expect(host.confirm.mock.calls[0]?.[0]?.message).toMatch(/occurrence/i);
    });
  });
});
