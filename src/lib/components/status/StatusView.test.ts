// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup, fireEvent, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Chunk, Comment, CreateCommentInput, Task } from '../../types';
import { isoToLocalDate } from '../shared/dateTimePickerShared';
import { clickConfirmAndExpect, flushReactivity } from '../../testFixtures';
import { TEST_NOW, baseChunk, baseTask } from '../tasks/testFixtures';
import { apiErrorMessage } from '../../api';
import type { StatusViewApi } from './statusViewShared';
import type { TaskFormApi } from '../tasks/taskFormShared';
import { router } from '../../router.svelte';
import { toastState } from '../../stores/toast.svelte';
import { warningState } from '../../stores/warnings.svelte';
import StatusView from './StatusView.svelte';
import { DEADLINE_WARNING, BLOCKING_WARNING } from './testFixtures';

// router.svelte.ts exports a singleton constructed at import time, which reads
// window.location.hash and registers a hashchange listener. There is no seam to inject
// through until the router is made injectable the way the api clients were; until then
// this is the only module still stubbed here.
// eslint-disable-next-line no-restricted-syntax -- router singleton, no injection seam yet
vi.mock('../../router.svelte', () => ({
  router: { current: 'status', navigate: vi.fn() },
}));

const TASK: Task = baseTask({ status: 'scheduled', deadline: '2026-07-01T10:00:00Z' });

const CHUNK: Chunk = baseChunk({
  start_time: '2026-07-17T15:00:00.000Z',
  end_time: '2026-07-17T16:00:00.000Z',
  is_fixed: true,
});

const COMMENT: Comment = {
  id: 'comment-1',
  task_id: 'task-1',
  author: 'test',
  content: 'test comment',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

let mockApi: StatusViewApi;
let fakeTaskFormApi: TaskFormApi;

beforeEach(() => {
  mockApi = {
    triggerReschedule: vi.fn().mockResolvedValue({
      placed_chunks: [],
      warnings: [DEADLINE_WARNING, BLOCKING_WARNING],
    }),
    getTask: vi.fn().mockResolvedValue(TASK),
    updateTask: vi.fn().mockResolvedValue(TASK),
    completeTask: vi.fn().mockResolvedValue({ ...TASK, status: 'completed' }),
    cancelTask: vi.fn().mockResolvedValue({ ...TASK, status: 'cancelled' }),
    listChunksForTask: vi.fn().mockResolvedValue([]),
    createFixedChunk: vi.fn().mockResolvedValue([CHUNK, TASK]),
    apiErrorMessage,
  };
  fakeTaskFormApi = {
    listComments: vi.fn<(taskId: string) => Promise<Comment[]>>().mockResolvedValue([]),
    createComment: vi
      .fn<(input: CreateCommentInput) => Promise<Comment>>()
      .mockResolvedValue(COMMENT),
    updateComment: vi
      .fn<(id: string, content: string) => Promise<Comment>>()
      .mockResolvedValue(COMMENT),
    deleteComment: vi.fn<(id: string) => Promise<void>>().mockResolvedValue(undefined),
    listChunksForTask: vi.fn<(taskId: string) => Promise<Chunk[]>>().mockResolvedValue([]),
    unlockChunk: vi.fn<(chunkId: string) => Promise<Chunk>>().mockResolvedValue(CHUNK),
    deleteFixedChunk: vi.fn<(chunkId: string) => Promise<Chunk>>().mockResolvedValue(CHUNK),
  } satisfies TaskFormApi;
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  warningState.items = [];
  toastState.items = [];
});

async function renderStatusView() {
  const result = render(StatusView, { apiClient: mockApi, taskFormApiClient: fakeTaskFormApi });
  await flushReactivity(2);
  return result;
}

/** Open one warning row's resolution menu and return the given item. */
async function openResolution(container: HTMLElement, taskTitle: string, label: string) {
  const trigger = Array.from(container.querySelectorAll<HTMLElement>('.resolve-trigger')).find(
    (el) => el.getAttribute('aria-label') === `Resolve ${taskTitle}`,
  );
  expect(trigger).toBeTruthy();
  await fireEvent.click(trigger!);
  await tick();
  // Preset labels carry a formatted date suffix — match on the stable prefix.
  const item = Array.from(container.querySelectorAll<HTMLElement>('[role="menuitem"]')).find((el) =>
    el.textContent?.trim().startsWith(label),
  );
  expect(item).toBeTruthy();
  return item!;
}

async function clickResolution(container: HTMLElement, taskTitle: string, label: string) {
  const item = await openResolution(container, taskTitle, label);
  await fireEvent.click(item);
  await tick();
}

/** Wait for the deadline-update API call and success toast shared by both deadline flows. */
async function expectDeadlineUpdated() {
  await waitFor(() => {
    expect(mockApi.updateTask).toHaveBeenCalledWith('task-1', {
      deadline: expect.any(String),
    });
    expect(toastState.items.find((t) => t.level === 'success')?.text).toBe('Deadline updated');
  });
}

describe('StatusView — warnings list', () => {
  it('derives warnings from a fresh reschedule on mount and renders a row per warning', async () => {
    const { getByText, getByRole } = await renderStatusView();

    expect(mockApi.triggerReschedule).toHaveBeenCalledTimes(1);
    expect(getByRole('heading', { name: '2 tasks need attention' })).toBeTruthy();
    expect(getByText('Alpha task')).toBeTruthy();
    expect(getByText('Deadline violation')).toBeTruthy();
    expect(getByText(/is earlier than the earliest completion/)).toBeTruthy();
    expect(getByText('Beta task')).toBeTruthy();
    expect(getByText('Unschedulable')).toBeTruthy();
    expect(getByText('No schedule windows are available.')).toBeTruthy();
  });

  it('marks only the unschedulable row with the blocking chip style', async () => {
    const { getByText } = await renderStatusView();

    expect(getByText('Unschedulable').classList.contains('warning-kind--blocking')).toBe(true);
    expect(getByText('Deadline violation').classList.contains('warning-kind--blocking')).toBe(
      false,
    );
  });

  it('shows the empty state when the reschedule reports no warnings', async () => {
    mockApi.triggerReschedule = vi.fn().mockResolvedValue({ placed_chunks: [], warnings: [] });

    const { getByText } = await renderStatusView();

    expect(getByText('All tasks fit the schedule')).toBeTruthy();
    expect(getByText(/No scheduling warnings/)).toBeTruthy();
  });

  it('keeps the previous warnings and shows a toast when the refresh fails', async () => {
    warningState.items = [DEADLINE_WARNING];
    mockApi.triggerReschedule = vi.fn().mockRejectedValue({ error: 'internal', message: 'boom' });

    const { getByText } = await renderStatusView();

    expect(toastState.items.find((t) => t.level === 'error')?.text).toBe(
      'Failed to refresh schedule warnings',
    );
    expect(getByText('Alpha task')).toBeTruthy();
  });

  it('embedded variant drops the page kicker for the shell modal host', async () => {
    const { container, queryByText, getByRole } = render(StatusView, {
      embedded: true,
      apiClient: mockApi,
      taskFormApiClient: fakeTaskFormApi,
    });
    await flushReactivity(2);

    expect(container.querySelector('.status-view--embedded')).toBeTruthy();
    expect(queryByText('Scheduling status')).toBeNull();
    expect(getByRole('heading', { name: '2 tasks need attention' })).toBeTruthy();
  });
});

describe('StatusView — task title editing', () => {
  it('clicking a warning title opens the task editor and calls listComments on the injected taskFormApiClient', async () => {
    const { getByRole, getByText } = await renderStatusView();

    await fireEvent.click(getByRole('button', { name: 'Alpha task' }));
    await flushReactivity(2);

    expect(mockApi.getTask).toHaveBeenCalledWith('task-1');
    expect(getByText('Edit Task')).toBeTruthy();
    expect(vi.mocked(router.navigate)).not.toHaveBeenCalled();
    expect(fakeTaskFormApi.listComments).toHaveBeenCalledWith('task-1');
  });

  it('a task load failure shows a toast and leaves the editor closed', async () => {
    mockApi.getTask = vi.fn().mockRejectedValue({ error: 'not_found', message: 'gone' });
    const { getByRole, queryByText } = await renderStatusView();

    await fireEvent.click(getByRole('button', { name: 'Alpha task' }));
    await flushReactivity(2);

    expect(toastState.items.find((t) => t.level === 'error')?.text).toBe('Failed to load task');
    expect(queryByText('Edit Task')).toBeNull();
  });

  it('submitting the editor updates the task, closes the form, and re-derives warnings', async () => {
    const { getByRole, getByText, queryByText, getByPlaceholderText, container } =
      await renderStatusView();

    await fireEvent.click(getByRole('button', { name: 'Alpha task' }));
    await flushReactivity(2);

    // Dirty the form first — a pristine edit form closes without saving.
    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    titleInput.value = 'Alpha task renamed';
    await fireEvent.input(titleInput);

    mockApi.triggerReschedule = vi.fn().mockResolvedValue({ placed_chunks: [], warnings: [] });
    await fireEvent.submit(container.querySelector('.task-form')!);

    await waitFor(() => {
      expect(mockApi.updateTask).toHaveBeenCalledWith(
        'task-1',
        expect.objectContaining({ title: 'Alpha task renamed' }),
      );
      expect(toastState.items.find((t) => t.level === 'success')?.text).toBe('Task updated');
    });
    expect(queryByText('Edit Task')).toBeNull();
    await waitFor(() => {
      expect(getByText('All tasks fit the schedule')).toBeTruthy();
    });
  });
});

describe('StatusView — resolution actions', () => {
  it('a successful deadline extension refreshes the list and the resolved row disappears', async () => {
    const { queryByText, container } = await renderStatusView();

    mockApi.triggerReschedule = vi.fn().mockResolvedValue({ placed_chunks: [], warnings: [] });
    await clickResolution(container, 'Alpha task', 'Extend to next week');

    await expectDeadlineUpdated();
    await waitFor(() => {
      expect(queryByText('Alpha task')).toBeNull();
    });
  });

  it('a failed resolution shows a toast and leaves the warning list unchanged', async () => {
    mockApi.updateTask = vi.fn().mockRejectedValue({ error: 'internal', message: 'boom' });
    const { getByText, container } = await renderStatusView();

    await clickResolution(container, 'Alpha task', 'Extend to next week');

    await waitFor(() => {
      expect(toastState.items.find((t) => t.level === 'error')?.text).toBe(
        'Failed to update deadline',
      );
    });
    // No refresh on failure: only the mount reschedule ran, the row stays.
    expect(mockApi.triggerReschedule).toHaveBeenCalledTimes(1);
    expect(getByText('Alpha task')).toBeTruthy();
  });

  describe('"Do now" — fixed instant', () => {
    beforeEach(() => {
      vi.useFakeTimers({ toFake: ['Date'] });
      vi.setSystemTime(TEST_NOW);
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it('books the full remaining duration starting now', async () => {
      const { container } = await renderStatusView();

      await clickResolution(container, 'Alpha task', 'Do now');

      await waitFor(() => {
        expect(mockApi.createFixedChunk).toHaveBeenCalledTimes(1);
      });
      const [taskId, start, end] = vi.mocked(mockApi.createFixedChunk).mock.calls[0];
      expect(taskId).toBe('task-1');
      expect(Date.parse(end) - Date.parse(start)).toBe(60 * 60_000);
      expect(Math.abs(Date.parse(start) - TEST_NOW.getTime())).toBeLessThan(5_000);
      expect(toastState.items.find((t) => t.level === 'success')?.text).toBe(
        'Scheduled to start now',
      );
    });
  });

  it.each<{
    label: string;
    taskTitle: string;
    resolution: string;
    taskId: string;
    apiMethod: 'completeTask' | 'cancelTask';
    confirmBtnSelector: string;
    shouldConfirm: boolean;
    toastText: string | null;
    confirmBtnText?: string;
    confirmBodyText?: RegExp;
  }>([
    {
      label: '"Complete task" confirm completes the task',
      taskTitle: 'Beta task',
      resolution: 'Complete task',
      taskId: 'task-2',
      apiMethod: 'completeTask',
      confirmBtnSelector: '.confirm-actions .btn-primary',
      shouldConfirm: true,
      toastText: 'Task completed',
      confirmBodyText: /All remaining time will be logged as done/,
    },
    {
      label: 'declining the completion confirm leaves the task untouched',
      taskTitle: 'Beta task',
      resolution: 'Complete task',
      taskId: 'task-2',
      apiMethod: 'completeTask',
      confirmBtnSelector: '.confirm-actions .btn-cancel',
      shouldConfirm: false,
      toastText: null,
    },
    {
      label: '"Cancel task" uses a destructive confirm button',
      taskTitle: 'Alpha task',
      resolution: 'Cancel task',
      taskId: 'task-1',
      apiMethod: 'cancelTask',
      confirmBtnSelector: '.confirm-actions .btn-danger',
      shouldConfirm: true,
      toastText: 'Task cancelled',
      confirmBtnText: 'Cancel task',
    },
  ])(
    'confirm-gate: $label',
    async ({
      taskTitle,
      resolution,
      taskId,
      apiMethod,
      confirmBtnSelector,
      shouldConfirm,
      toastText,
      confirmBtnText,
      confirmBodyText,
    }) => {
      const { getByText, container } = await renderStatusView();
      await clickResolution(container, taskTitle, resolution);
      expect(mockApi[apiMethod]).not.toHaveBeenCalled();
      if (confirmBodyText) expect(getByText(confirmBodyText)).toBeTruthy();
      if (shouldConfirm) {
        const confirmBtn = container.querySelector(confirmBtnSelector);
        expect(confirmBtn).toBeTruthy();
        if (confirmBtnText) expect(confirmBtn?.textContent?.trim()).toBe(confirmBtnText);
        await clickConfirmAndExpect(confirmBtn, () => {
          expect(mockApi[apiMethod]).toHaveBeenCalledWith(taskId);
          if (toastText)
            expect(toastState.items.find((t) => t.level === 'success')?.text).toBe(toastText);
        });
      } else {
        await fireEvent.click(container.querySelector(confirmBtnSelector)!);
        await flushReactivity();
        expect(mockApi[apiMethod]).not.toHaveBeenCalled();
        expect(getByText(taskTitle)).toBeTruthy();
      }
    },
  );
});

describe('StatusView — custom deadline submenu', () => {
  it('one day click in the hover calendar extends the deadline, resetting the time to end of day', async () => {
    const { container } = await renderStatusView();

    const custom = await openResolution(container, 'Alpha task', 'Custom deadline');
    await fireEvent.mouseEnter(custom);
    await tick();

    // Calendar opens on the existing deadline's month with it marked selected.
    const selectedDate = isoToLocalDate('2026-07-01T10:00:00Z');
    expect(container.querySelector('.calendar-day-btn--selected')?.getAttribute('data-date')).toBe(
      selectedDate,
    );

    mockApi.triggerReschedule = vi.fn().mockResolvedValue({ placed_chunks: [], warnings: [] });
    const targetDate = `${selectedDate.slice(0, 8)}15`;
    await fireEvent.click(container.querySelector(`[data-date="${targetDate}"]`)!);
    await tick();

    await expectDeadlineUpdated();
    // The picked day resets the time to end of day, even though the existing deadline had a time set.
    const deadline = vi.mocked(mockApi.updateTask).mock.calls[0][1].deadline as string;
    expect(new Date(deadline).getHours()).toBe(23);
    expect(new Date(deadline).getMinutes()).toBe(59);
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it('dismissing the menu without picking keeps the deadline untouched', async () => {
    const { container } = await renderStatusView();

    const custom = await openResolution(container, 'Alpha task', 'Custom deadline');
    await fireEvent.mouseEnter(custom);
    await tick();
    expect(container.querySelector('.submenu-panel')).toBeTruthy();

    await fireEvent.pointerDown(document.body);
    await flushReactivity();

    expect(container.querySelector('[role="menu"]')).toBeNull();
    expect(mockApi.updateTask).not.toHaveBeenCalled();
  });
});
