// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach, type Mock, type Mocked } from 'vitest';
import { render, cleanup, fireEvent, waitFor } from '@testing-library/svelte';
import { tick, type ComponentProps } from 'svelte';
import type { Task, Schedule, UpdateTaskInput } from '../../types';
import type { TaskDetailApi } from './taskDetailShared';
import { statusCases, chunkFixture } from '../../testFixtures';
import { taskState } from '../../stores/tasks.svelte';
import { scheduleState } from '../../stores/schedules.svelte';

let removeTaskSpy: Mock;
let updateTaskSpy: Mock;

beforeEach(() => {
  removeTaskSpy = vi.spyOn(taskState, 'remove').mockImplementation(() => Promise.resolve()) as Mock;
  updateTaskSpy = vi.spyOn(taskState, 'update').mockImplementation(() => Promise.resolve()) as Mock;
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.clearAllMocks();
  scheduleState.items = [];
});

async function clickLastButton(
  utils: { getAllByRole: (role: string, options?: { name: string }) => HTMLElement[] },
  name: string,
): Promise<HTMLElement> {
  const buttons = utils.getAllByRole('button', { name });
  const last = buttons[buttons.length - 1];
  await fireEvent.click(last);
  return last;
}

function baseTask(overrides: Partial<Task> = {}): Task {
  return {
    id: 'task-1',
    title: 'Test Task',
    description: 'A detailed description',
    duration_minutes: 90,
    time_logged_minutes: 30,
    priority: 'High',
    status: 'pending',
    start_date: '2026-05-01T09:00:00.000Z',
    deadline: '2026-06-15T12:00:00.000Z',
    schedule_id: 'sched-1',
    min_chunk_minutes: 30,
    no_split: false,
    recurring_template_id: null,
    labels: ['backend', 'urgent'],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function baseSchedule(overrides: Partial<Schedule> = {}): Schedule {
  return {
    id: 'sched-1',
    name: 'Work Hours',
    is_default: true,
    windows: [],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

async function importDetail() {
  const mod = await import('./TaskDetail.svelte');
  return mod.default;
}

function makeApiClient(
  overrides: Partial<{ [K in keyof TaskDetailApi]: Mock }> = {},
): Mocked<TaskDetailApi> {
  const stub: { [K in keyof TaskDetailApi]: Mock } = {
    completeTask: vi.fn(),
    cancelTask: vi.fn(),
    listChunksForTask: vi.fn().mockResolvedValue([]),
    listComments: vi.fn().mockResolvedValue([]),
    createComment: vi.fn(),
    updateComment: vi.fn(),
    deleteComment: vi.fn(),
    ...overrides,
  };
  return stub as Mocked<TaskDetailApi>;
}

type DetailProps = ComponentProps<Awaited<ReturnType<typeof importDetail>>>;

async function renderDetail(props: Partial<DetailProps> = {}) {
  const TaskDetail = await importDetail();
  const utils = render(TaskDetail, {
    task: baseTask(),
    onclose: vi.fn(),
    onedit: vi.fn(),
    apiClient: makeApiClient(),
    ...props,
  });
  await tick();
  return utils;
}

/** Drain the mount effect that fetches chunks (tick → microtask → tick). */
async function settle() {
  await tick();
  await Promise.resolve();
  await tick();
}

describe('TaskDetail — renders task metadata', () => {
  const metadataCases: Array<{ label: string; overrides: Partial<Task>; expected: string }> = [
    { label: 'title', overrides: { title: 'My Important Task' }, expected: 'My Important Task' },
    {
      label: 'description',
      overrides: { description: 'A detailed description' },
      expected: 'A detailed description',
    },
    { label: 'priority badge', overrides: { priority: 'Critical' }, expected: 'Critical' },
    { label: 'status badge', overrides: { status: 'scheduled' }, expected: 'Scheduled' },
    { label: 'duration', overrides: { duration_minutes: 90 }, expected: '1h 30m' },
    {
      label: 'logged time',
      overrides: { duration_minutes: 60, time_logged_minutes: 30 },
      expected: '30m (50%)',
    },
  ];

  it.each(metadataCases)('renders $label', async ({ overrides, expected }) => {
    const { getByText } = await renderDetail({ task: baseTask(overrides) });
    expect(getByText(expected)).toBeTruthy();
  });

  it('renders labels as chips', async () => {
    const { getByText } = await renderDetail({ task: baseTask({ labels: ['backend', 'urgent'] }) });
    expect(getByText('backend')).toBeTruthy();
    expect(getByText('urgent')).toBeTruthy();
  });

  it('renders no-split value as Yes or No', async () => {
    const { getByText } = await renderDetail({ task: baseTask({ no_split: true }) });
    expect(getByText('Yes')).toBeTruthy();
  });
});

describe('TaskDetail — Markdown description rendering', () => {
  it('renders bold markdown inside .detail-description', async () => {
    const { container } = await renderDetail({ task: baseTask({ description: '**bold** text' }) });

    const descEl = container.querySelector('.detail-description') as HTMLElement;
    expect(descEl).toBeTruthy();
    expect(descEl.querySelector('strong')).toBeTruthy();
    expect(descEl.querySelector('strong')!.textContent).toBe('bold');
  });

  it('null description still renders "No description"', async () => {
    const { getByText } = await renderDetail({ task: baseTask({ description: null }) });
    expect(getByText('No description')).toBeTruthy();
  });
});

describe('TaskDetail — edge cases: null/empty fields', () => {
  it('renders "—" for null deadline', async () => {
    const { getAllByText } = await renderDetail({ task: baseTask({ deadline: null }) });
    const dashes = getAllByText('—');
    expect(dashes.length).toBeGreaterThan(0);
  });

  it('does not render labels section when labels array is empty', async () => {
    const { container } = await renderDetail({ task: baseTask({ labels: [] }) });
    expect(container.querySelector('.labels-chips')).toBeNull();
  });

  it.each([
    { duration_minutes: 0, time_logged_minutes: 0, expected: '0m (0%)' },
    { duration_minutes: 60, time_logged_minutes: 120, expected: '2h (100%)' },
  ])(
    'logged time boundary: $duration_minutes min duration, $time_logged_minutes min logged → $expected',
    async ({ duration_minutes, time_logged_minutes, expected }) => {
      const { getByText } = await renderDetail({
        task: baseTask({ duration_minutes, time_logged_minutes }),
      });
      expect(getByText(expected)).toBeTruthy();
    },
  );
});

describe('TaskDetail — schedule assignment display', () => {
  it('shows resolved schedule name when scheduleState contains the schedule', async () => {
    scheduleState.items = [baseSchedule({ id: 'sched-1', name: 'Work Hours' })];

    const { getByText } = await renderDetail({ task: baseTask({ schedule_id: 'sched-1' }) });
    expect(getByText('Work Hours')).toBeTruthy();
  });

  it('falls back to raw schedule_id when schedule is not in scheduleState', async () => {
    const { getByText } = await renderDetail({ task: baseTask({ schedule_id: 'unknown-sched' }) });
    expect(getByText('unknown-sched')).toBeTruthy();
  });
});

describe('TaskDetail — recurring task', () => {
  it.each([
    { recurring: null as string | null, visible: false },
    { recurring: 'template-42', visible: true },
  ])(
    'Edit Template and Recurring badge visible=$visible for recurring_template_id=$recurring',
    async ({ recurring, visible }) => {
      const { queryByText } = await renderDetail({
        task: baseTask({ recurring_template_id: recurring }),
      });
      expect(queryByText('Edit Template') !== null).toBe(visible);
      expect(queryByText('Recurring') !== null).toBe(visible);
    },
  );

  it('calls onedittemplate with the template id when "Edit Template" is clicked', async () => {
    const onedittemplate = vi.fn();
    const { getByText } = await renderDetail({
      task: baseTask({ recurring_template_id: 'template-42' }),
      onedittemplate,
    });
    await fireEvent.click(getByText('Edit Template'));
    expect(onedittemplate).toHaveBeenCalledTimes(1);
    expect(onedittemplate).toHaveBeenCalledWith('template-42');
  });

  it('does not throw when "Edit Template" is clicked and onedittemplate is not provided', async () => {
    const { getByText } = await renderDetail({
      task: baseTask({ recurring_template_id: 'template-42' }),
      // onedittemplate intentionally omitted
    });
    // Should not throw — optional callback is guarded with ?.
    await expect(fireEvent.click(getByText('Edit Template'))).resolves.not.toThrow();
  });
});

describe('TaskDetail — chunk list', () => {
  it('shows "No chunks scheduled" when chunk list is empty', async () => {
    const { getByText } = await renderDetail({
      apiClient: makeApiClient({ listChunksForTask: vi.fn().mockResolvedValue([]) }),
    });
    await settle();
    expect(getByText('No chunks scheduled')).toBeTruthy();
  });

  it('renders fetched chunks', async () => {
    const chunk = chunkFixture({
      start_time: '2026-05-10T09:00:00.000Z',
      end_time: '2026-05-10T10:00:00.000Z',
    });
    const { container } = await renderDetail({
      apiClient: makeApiClient({ listChunksForTask: vi.fn().mockResolvedValue([chunk]) }),
    });
    await settle();

    const chunkItems = container.querySelectorAll('.chunk-item');
    expect(chunkItems).toHaveLength(1);
  });

  it('renders chunk duration for each chunk', async () => {
    const chunk = chunkFixture({
      start_time: '2026-05-10T09:00:00.000Z',
      end_time: '2026-05-10T09:45:00.000Z', // 45 minutes — distinct from task duration
    });
    const { container } = await renderDetail({
      task: baseTask({ duration_minutes: 90 }),
      apiClient: makeApiClient({ listChunksForTask: vi.fn().mockResolvedValue([chunk]) }),
    });
    await settle();
    const durationEl = container.querySelector('.chunk-duration');
    expect(durationEl).toBeTruthy();
    expect(durationEl!.textContent).toBe('45m');
  });

  it.each([{ method: 'listChunksForTask' as const }, { method: 'listComments' as const }])(
    'calls $method with the task id via injected apiClient',
    async ({ method }) => {
      const apiCall = vi.fn().mockResolvedValue([]);
      const overrides: Partial<{ [K in keyof TaskDetailApi]: Mock }> = {};
      overrides[method] = apiCall;
      await renderDetail({
        task: baseTask({ id: 'task-xyz' }),
        apiClient: makeApiClient(overrides),
      });
      await settle();
      expect(apiCall).toHaveBeenCalledWith('task-xyz');
    },
  );

  it('renders multiple chunks', async () => {
    const { container } = await renderDetail({
      apiClient: makeApiClient({
        listChunksForTask: vi.fn().mockResolvedValue([
          chunkFixture({
            id: 'c1',
            start_time: '2026-05-10T09:00:00Z',
            end_time: '2026-05-10T10:00:00Z',
          }),
          chunkFixture({
            id: 'c2',
            start_time: '2026-05-11T09:00:00Z',
            end_time: '2026-05-11T10:00:00Z',
          }),
        ]),
      }),
    });
    await settle();

    expect(container.querySelectorAll('.chunk-item')).toHaveLength(2);
  });
});

describe('TaskDetail — close and edit callbacks', () => {
  it('calls onclose when close button is clicked', async () => {
    const onclose = vi.fn();
    const { getByLabelText } = await renderDetail({ onclose });
    await fireEvent.click(getByLabelText('Close detail panel'));
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('calls onedit with task when Edit button is clicked', async () => {
    const onedit = vi.fn();
    const task = baseTask();
    const { getByRole } = await renderDetail({ task, onedit });
    await fireEvent.click(getByRole('button', { name: 'Edit' }));
    expect(onedit).toHaveBeenCalledTimes(1);
    expect(onedit).toHaveBeenCalledWith(task);
  });
});

describe('TaskDetail — complete action', () => {
  it('completes the task through the task-level complete API', async () => {
    const completeTask = vi
      .fn()
      .mockResolvedValue(baseTask({ status: 'completed', time_logged_minutes: 90 }));
    const utils = await renderDetail({
      task: baseTask({ status: 'scheduled' }),
      apiClient: makeApiClient({
        listChunksForTask: vi.fn().mockResolvedValue([chunkFixture()]),
        completeTask,
      }),
    });

    await fireEvent.click(utils.getByRole('button', { name: 'Complete' }));
    await tick();
    await clickLastButton(utils, 'Complete');

    await waitFor(() => {
      expect(completeTask).toHaveBeenCalledWith('task-1');
    });
  });
});

describe('TaskDetail — cancel action', () => {
  it('shows confirm dialog when "Cancel task" button is clicked', async () => {
    const { getByRole } = await renderDetail({ task: baseTask({ status: 'pending' }) });
    await fireEvent.click(getByRole('button', { name: 'Cancel task' }));
    await tick();
    expect(getByRole('alertdialog')).toBeTruthy();
  });

  it.each([
    { status: 'cancelled' as Task['status'], visible: false },
    { status: 'completed' as Task['status'], visible: false },
    { status: 'pending' as Task['status'], visible: true },
    { status: 'scheduled' as Task['status'], visible: true },
  ])('"Cancel task" button visible for "$status": $visible', async ({ status, visible }) => {
    const { queryByRole } = await renderDetail({ task: baseTask({ status }) });
    expect(queryByRole('button', { name: 'Cancel task' }) !== null).toBe(visible);
  });

  it('calls cancelTask API when confirm dialog is confirmed', async () => {
    const cancelTask = vi.fn().mockResolvedValue(baseTask({ status: 'cancelled' }));
    const utils = await renderDetail({
      task: baseTask({ status: 'pending' }),
      apiClient: makeApiClient({ cancelTask }),
    });

    await fireEvent.click(utils.getByRole('button', { name: 'Cancel task' }));
    await tick();
    await clickLastButton(utils, 'Cancel task');
    await tick();

    expect(cancelTask).toHaveBeenCalledWith('task-1');
  });
});

describe('TaskDetail — delete action', () => {
  it('shows confirm dialog when Delete button is clicked', async () => {
    const { getByRole, getByText } = await renderDetail();
    await fireEvent.click(getByRole('button', { name: 'Delete' }));
    await tick();
    expect(getByText('Delete task')).toBeTruthy();
  });

  it('calls deleteTask API when delete is confirmed', async () => {
    const utils = await renderDetail();

    await fireEvent.click(utils.getByRole('button', { name: 'Delete' }));
    await tick();
    await clickLastButton(utils, 'Delete');
    await tick();

    expect(removeTaskSpy).toHaveBeenCalledWith('task-1');
  });
});

describe('TaskDetail — inline quick-edit', () => {
  it('reveals a priority select with all options when Edit priority is clicked', async () => {
    const { getByRole, getByLabelText } = await renderDetail({
      task: baseTask({ priority: 'High' }),
    });

    await fireEvent.click(getByRole('button', { name: 'Edit priority' }));

    const select = getByLabelText('Priority') as HTMLSelectElement;
    expect(select.value).toBe('High');
    const options = Array.from(select.querySelectorAll('option')).map((o) => o.value);
    expect(options).toEqual(['Low', 'Medium', 'High', 'Critical']);
  });

  it('patches priority, closes the editor, and reloads chunks', async () => {
    const listChunksForTask = vi.fn().mockResolvedValue([]);
    const { getByRole, getByLabelText, queryByLabelText } = await renderDetail({
      task: baseTask({ priority: 'High' }),
      apiClient: makeApiClient({ listChunksForTask }),
    });
    await settle();
    expect(listChunksForTask).toHaveBeenCalledTimes(1);

    await fireEvent.click(getByRole('button', { name: 'Edit priority' }));
    await fireEvent.change(getByLabelText('Priority'), { target: { value: 'Low' } });
    await settle();

    expect(updateTaskSpy).toHaveBeenCalledWith('task-1', { priority: 'Low' });
    expect(queryByLabelText('Priority')).toBeNull();
    // Priority affects scheduling order — the chunk list must refetch.
    expect(listChunksForTask).toHaveBeenCalledTimes(2);
  });

  it('reveals a deadline picker when Edit deadline is clicked', async () => {
    const { getByRole } = await renderDetail({
      task: baseTask({ deadline: '2026-06-15T12:00:00.000Z' }),
    });

    await fireEvent.click(getByRole('button', { name: 'Edit deadline' }));

    expect(getByRole('button', { name: /selected/i })).toBeTruthy();
  });

  it('toggling Edit deadline again closes the picker without an update', async () => {
    const { getByRole, queryByRole } = await renderDetail();

    await fireEvent.click(getByRole('button', { name: 'Edit deadline' }));
    await fireEvent.click(getByRole('button', { name: 'Edit deadline' }));

    expect(queryByRole('button', { name: /selected/i })).toBeNull();
    expect(updateTaskSpy).not.toHaveBeenCalled();
  });

  it('patches the deadline through the picker and closes the editor', async () => {
    const listChunksForTask = vi.fn().mockResolvedValue([]);
    const { container, getByRole, queryByRole } = await renderDetail({
      task: baseTask({ deadline: '2026-06-15T12:00:00.000Z' }),
      apiClient: makeApiClient({ listChunksForTask }),
    });
    await settle();

    await fireEvent.click(getByRole('button', { name: 'Edit deadline' }));
    await fireEvent.click(getByRole('button', { name: /selected/i }));
    await fireEvent.click(getByRole('button', { name: 'Time' }));
    await fireEvent.click(
      container.querySelector('.time-menu button[data-time="23:59"]') as HTMLButtonElement,
    );
    await settle();

    expect(updateTaskSpy).toHaveBeenCalledTimes(1);
    const [id, input] = updateTaskSpy.mock.calls[0] as [string, UpdateTaskInput];
    expect(id).toBe('task-1');
    const patched = new Date(input.deadline!);
    expect(patched.getHours()).toBe(23);
    expect(patched.getMinutes()).toBe(59);
    // Editor closed and chunks refetched (deadline moves chunks).
    expect(queryByRole('button', { name: /selected/i })).toBeNull();
    expect(listChunksForTask).toHaveBeenCalledTimes(2);
  });
});

describe('TaskDetail — status badge for each TaskStatus', () => {
  it.each(statusCases)('renders "$status" status badge as "$label"', async ({ status, label }) => {
    const { getByText } = await renderDetail({ task: baseTask({ status }) });
    expect(getByText(label)).toBeTruthy();
  });
});

describe('TaskDetail — priority badge for each Priority', () => {
  const priorityCases: Array<{ priority: Task['priority'] }> = [
    { priority: 'Low' },
    { priority: 'Medium' },
    { priority: 'High' },
    { priority: 'Critical' },
  ];

  it.each(priorityCases)('renders "$priority" priority badge', async ({ priority }) => {
    const { getByText } = await renderDetail({ task: baseTask({ priority }) });
    expect(getByText(priority)).toBeTruthy();
  });
});

describe('TaskDetail — inline description editor', () => {
  let listChunksForTask: Mock;

  beforeEach(() => {
    listChunksForTask = vi.fn().mockResolvedValue([]);
  });

  it('default: shows MarkdownView and an Edit description button', async () => {
    const { container, getByRole } = await renderDetail({
      task: baseTask({ description: '**bold** text' }),
      apiClient: makeApiClient({ listChunksForTask }),
    });
    expect(container.querySelector('.detail-description')).toBeTruthy();
    expect(getByRole('button', { name: 'Edit description' })).toBeTruthy();
  });

  it('clicking Edit description shows textarea with current description', async () => {
    const { getByRole, container } = await renderDetail({
      task: baseTask({ description: 'My description' }),
      apiClient: makeApiClient({ listChunksForTask }),
    });
    await fireEvent.click(getByRole('button', { name: 'Edit description' }));
    await tick();
    const textarea = container.querySelector('.description-textarea') as HTMLTextAreaElement;
    expect(textarea).toBeTruthy();
    expect(textarea.value).toBe('My description');
  });

  it('Save calls api.updateTask, refetches chunks, and returns to preview', async () => {
    const { getByRole, container } = await renderDetail({
      task: baseTask({ description: 'Original' }),
      apiClient: makeApiClient({ listChunksForTask }),
    });
    await settle();
    const chunkCallsBefore = listChunksForTask.mock.calls.length;

    await fireEvent.click(getByRole('button', { name: 'Edit description' }));
    await tick();

    const textarea = container.querySelector('.description-textarea') as HTMLTextAreaElement;
    await fireEvent.input(textarea, { target: { value: 'Updated text' } });
    await fireEvent.click(getByRole('button', { name: 'Save' }));
    await settle();

    expect(updateTaskSpy).toHaveBeenCalledWith('task-1', { description: 'Updated text' });
    // Description save triggers a backend reschedule; refetch chunks to reflect new placement.
    expect(listChunksForTask.mock.calls).toHaveLength(chunkCallsBefore + 1);
    // Returned to preview (description editor textarea gone).
    expect(container.querySelector('.description-textarea')).toBeNull();
  });

  it.each([['Save'], ['Cancel']] as const)(
    '%s without text change skips api.updateTask and returns to preview',
    async (btn) => {
      const { getByRole, container } = await renderDetail({
        task: baseTask({ description: 'Original' }),
        apiClient: makeApiClient({ listChunksForTask }),
      });

      await fireEvent.click(getByRole('button', { name: 'Edit description' }));
      await tick();
      await fireEvent.click(getByRole('button', { name: btn }));
      await tick();

      expect(updateTaskSpy).not.toHaveBeenCalled();
      expect(container.querySelector('.description-textarea')).toBeNull();
    },
  );

  it('closing the panel mid-edit auto-saves the draft and calls onclose', async () => {
    const onclose = vi.fn();
    const { getByRole, container } = await renderDetail({
      task: baseTask({ description: 'Original' }),
      onclose,
      apiClient: makeApiClient({ listChunksForTask }),
    });

    await fireEvent.click(getByRole('button', { name: 'Edit description' }));
    await tick();

    const textarea = container.querySelector('.description-textarea') as HTMLTextAreaElement;
    await fireEvent.input(textarea, { target: { value: 'Draft text' } });
    await fireEvent.click(getByRole('button', { name: 'Close detail panel' }));
    await settle();

    expect(updateTaskSpy).toHaveBeenCalledWith('task-1', { description: 'Draft text' });
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('null description shows a "No description" button that opens edit mode', async () => {
    const { getByRole, container } = await renderDetail({
      task: baseTask({ description: null }),
      apiClient: makeApiClient({ listChunksForTask }),
    });

    const placeholder = getByRole('button', { name: 'No description' });
    expect(placeholder).toBeTruthy();
    await fireEvent.click(placeholder);
    await tick();

    const textarea = container.querySelector('.description-textarea') as HTMLTextAreaElement;
    expect(textarea).toBeTruthy();
    expect(textarea.value).toBe('');
  });
});
