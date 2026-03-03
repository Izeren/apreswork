// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach, type MockInstance } from 'vitest';
import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Task, ScheduleResult } from '../../types';
import { warningState } from '../../stores/warnings.svelte';
import { baseTask, baseTemplate, baseSchedule } from './testFixtures';
import {
  installTaskListLifecycle,
  taskListViewFakeApi,
  taskListViewFakeTasksClient,
  taskListViewFakeTemplatesClient,
  renderTaskListView,
  type MockedTasksClient,
  type MockedTemplatesClient,
} from './taskListViewTestSupport';
import { activeShortcuts, resetShortcutsForTest } from '../../shortcuts.svelte';

installTaskListLifecycle(resetShortcutsForTest);

let fakeTasksClient: MockedTasksClient;
let fakeApi: ReturnType<typeof taskListViewFakeApi>;

beforeEach(() => {
  fakeTasksClient = taskListViewFakeTasksClient([]);
  fakeApi = taskListViewFakeApi();
});

async function settle() {
  await Promise.resolve();
  await tick();
}

async function renderView(opts: { fakeTemplatesClient?: MockedTemplatesClient } = {}) {
  return renderTaskListView(fakeTasksClient, { fakeApi, ...opts });
}

async function renderWithTasks(...tasks: Task[]) {
  fakeTasksClient.listTasks.mockResolvedValue(tasks);
  return renderView();
}

async function openTaskActions() {
  await fireEvent.click(screen.getByRole('button', { name: /task actions/i }));
  await tick();
}

function sortButton(toolbar: HTMLElement, text: string) {
  return Array.from(toolbar.querySelectorAll('button')).find((b) => b.textContent?.includes(text));
}

async function renderSortToolbar() {
  const utils = await renderView();
  return { ...utils, toolbar: utils.getByRole('toolbar', { name: /sort controls/i }) };
}

async function renderReschedule(warnings: ScheduleResult['warnings']) {
  fakeApi.triggerReschedule.mockResolvedValueOnce({ placed_chunks: [], warnings });
  const { getByRole } = await renderView();
  await fireEvent.click(getByRole('button', { name: /reschedule tasks/i }));
  await settle();
}

describe('TaskListView — loading and empty states', () => {
  it('shows loading message while load() is in flight', async () => {
    fakeTasksClient.listTasks.mockImplementation(() => new Promise(() => {}));
    const { getByText } = await renderView();
    expect(getByText(/loading tasks/i)).toBeTruthy();
  });

  it('shows empty state when not loading and no tasks', async () => {
    const { getByText } = await renderView();
    expect(getByText(/no tasks found/i)).toBeTruthy();
  });

  it('renders task rows when items are present', async () => {
    const { getByText } = await renderWithTasks(
      baseTask({ id: 'task-1', title: 'Task One' }),
      baseTask({ id: 'task-2', title: 'Task Two' }),
    );
    expect(getByText('Task One')).toBeTruthy();
    expect(getByText('Task Two')).toBeTruthy();
  });

  it('renders filter bar with search input', async () => {
    const { getByRole } = await renderView();
    expect(getByRole('searchbox', { name: /search tasks/i })).toBeTruthy();
  });

  it.each([
    { name: 'Status: Scheduled', label: 'status filter' },
    { name: 'Priority: All', label: 'priority filter' },
  ])('renders $label dropdown', async ({ name }) => {
    const { getByRole } = await renderView();
    expect(getByRole('button', { name })).toBeTruthy();
  });

  it('does not show "Clear filters" button when no filters are active', async () => {
    const { queryByText } = await renderView();
    expect(queryByText(/clear filters/i)).toBeNull();
  });

  it('shows "Clear filters" button when search text is entered', async () => {
    const { getByRole, getByText } = await renderView();
    const searchInput = getByRole('searchbox');
    await fireEvent.input(searchInput, { target: { value: 'something' } });
    expect(getByText(/clear filters/i)).toBeTruthy();
  });

  it.each([
    { status: 'backlog', target: 'pending', switchName: /add to scheduling/i },
    { status: 'pending', target: 'backlog', switchName: /remove from scheduling/i },
    { status: 'scheduled', target: 'backlog', switchName: /remove from scheduling/i },
  ] as const)(
    'toggles a $status task to $target from the task list switch',
    async ({ status, target, switchName }) => {
      const task = baseTask({ id: `task-${status}`, status, title: `${status} task` });
      fakeTasksClient.updateTask.mockResolvedValue({ ...task, status: target });

      const { getByRole } = await renderWithTasks(task);
      await fireEvent.click(getByRole('switch', { name: switchName }));
      await settle();

      expect(fakeTasksClient.updateTask).toHaveBeenCalledWith(`task-${status}`, { status: target });
    },
  );
});

describe('TaskListView — reschedule warnings', () => {
  beforeEach(() => {
    warningState.items = [];
  });

  const sampleWarning: ScheduleResult['warnings'][number] = {
    task_id: 'task-1',
    task_title: 'Alpha task',
    kind: {
      DeadlineViolation: {
        deadline: '2026-06-01T00:00:00Z',
        earliest_completion: '2026-06-03T00:00:00Z',
      },
    },
  };

  it.each([
    {
      desc: 'stores warnings when reschedule returns some',
      warnings: [sampleWarning],
      expectedItems: [sampleWarning],
    },
    { desc: 'clears warnings when reschedule returns none', warnings: [], expectedItems: [] },
  ])('$desc', async ({ warnings, expectedItems }) => {
    await renderReschedule(warnings);
    expect(warningState.items).toEqual(expectedItems);
  });
});

describe('TaskListView — debounce timing', () => {
  let listTasksSpy: MockInstance;

  beforeEach(() => {
    vi.useFakeTimers();
    listTasksSpy = fakeTasksClient.listTasks;
    listTasksSpy.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
    cleanup();
    vi.clearAllMocks();
  });

  async function renderAndSearch(value: string) {
    const { getByRole } = await renderView();
    const callsAfterMount = listTasksSpy.mock.calls.length;
    await fireEvent.input(getByRole('searchbox'), { target: { value } });
    return callsAfterMount;
  }

  it('does not call listTasks immediately when search text is entered', async () => {
    const callsAfterMount = await renderAndSearch('hello');
    expect(listTasksSpy.mock.calls.slice(callsAfterMount)).toHaveLength(0);
  });

  it('calls listTasks after 300ms debounce delay', async () => {
    const callsAfterMount = await renderAndSearch('hello');

    vi.advanceTimersByTime(300);
    await settle();

    expect(listTasksSpy.mock.calls.length).toBeGreaterThan(callsAfterMount);
  });
});

describe('TaskListView — sort toggle', () => {
  it('Priority button is active by default', async () => {
    const { toolbar } = await renderSortToolbar();
    const priorityBtn = sortButton(toolbar, 'Priority');
    expect(priorityBtn).toBeTruthy();
    expect(priorityBtn!.classList.contains('active')).toBe(true);
  });

  it('clicking Deadline button makes it active', async () => {
    const { toolbar } = await renderSortToolbar();
    const deadlineBtn = sortButton(toolbar, 'Deadline');
    expect(deadlineBtn).toBeTruthy();
    await fireEvent.click(deadlineBtn!);
    await tick();

    expect(deadlineBtn!.classList.contains('active')).toBe(true);
  });

  it('clicking the active sort button toggles direction (indicator changes)', async () => {
    const { toolbar } = await renderSortToolbar();
    const priorityBtn = sortButton(toolbar, 'Priority');
    expect(priorityBtn).toBeTruthy();
    const initialText = priorityBtn!.textContent ?? '';

    await fireEvent.click(priorityBtn!);
    await tick();

    expect(priorityBtn!.textContent).not.toBe(initialText);
  });
});

describe('TaskListView — sort persistence', () => {
  it.each(['Status', 'Logged'])('renders %s sort button', async (buttonText) => {
    const { toolbar } = await renderSortToolbar();
    expect(sortButton(toolbar, buttonText)).toBeTruthy();
  });

  it('persists the whole sort stack to localStorage (clicked field promoted)', async () => {
    const { toolbar } = await renderSortToolbar();

    await fireEvent.click(sortButton(toolbar, 'Deadline')!);
    await tick();

    expect(JSON.parse(window.localStorage.getItem('apreswork.taskList.sort')!)).toEqual([
      { field: 'deadline', direction: 'asc' },
      { field: 'priority', direction: 'desc' },
    ]);
  });

  it('clicking fields in sequence keeps earlier keys as tie-breakers', async () => {
    const { toolbar } = await renderSortToolbar();

    await fireEvent.click(sortButton(toolbar, 'Title')!);
    await fireEvent.click(sortButton(toolbar, 'Logged')!);
    await tick();

    expect(JSON.parse(window.localStorage.getItem('apreswork.taskList.sort')!)).toEqual([
      { field: 'logged', direction: 'asc' },
      { field: 'title', direction: 'asc' },
      { field: 'priority', direction: 'desc' },
      { field: 'deadline', direction: 'asc' },
    ]);
  });

  it('restores the persisted sort stack on mount', async () => {
    window.localStorage.setItem(
      'apreswork.taskList.sort',
      JSON.stringify([{ field: 'logged', direction: 'desc' }]),
    );

    const { toolbar } = await renderSortToolbar();
    expect(sortButton(toolbar, 'Logged')!.classList.contains('active')).toBe(true);
  });

  it.each([
    { name: 'junk field', raw: '[{"field":"evil","direction":"asc"}]' },
    // The pre-stack single-key shape fails validation once and resets.
    { name: 'pre-stack single-key shape', raw: '{"field":"logged","direction":"desc"}' },
  ])('ignores an invalid persisted sort ($name) and falls back to the default', async ({ raw }) => {
    window.localStorage.setItem('apreswork.taskList.sort', raw);

    const { toolbar } = await renderSortToolbar();
    expect(sortButton(toolbar, 'Priority')!.classList.contains('active')).toBe(true);
  });
});

describe('TaskListView — row kebab menu', () => {
  const pendingTestTask = baseTask({ id: 'task-1', title: 'Alpha task', status: 'pending' });

  it('opens the task verb menu from the row kebab and runs a verb', async () => {
    const task = pendingTestTask;
    fakeApi.updateTask.mockResolvedValue({ ...task, status: 'backlog' });

    await renderWithTasks(task);
    await openTaskActions();

    await fireEvent.click(screen.getByRole('menuitem', { name: 'Move to backlog' }));
    await settle();

    expect(fakeApi.updateTask).toHaveBeenCalledWith('task-1', { status: 'backlog' });
  });

  it('routes destructive verbs through the confirm dialog', async () => {
    const task = pendingTestTask;

    await renderWithTasks(task);
    await openTaskActions();
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Delete task' }));
    await tick();

    expect(fakeApi.deleteTask).not.toHaveBeenCalled();
    expect(screen.getByText(/removed permanently/i)).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await settle();

    expect(fakeApi.deleteTask).toHaveBeenCalledWith('task-1');
  });
});

describe('TaskListView — recurring tab', () => {
  let fakeTemplatesClient: MockedTemplatesClient;

  beforeEach(async () => {
    fakeTemplatesClient = taskListViewFakeTemplatesClient();

    const { scheduleState } = await import('../../stores/schedules.svelte');
    scheduleState.items = [];
    scheduleState.loading = false;
  });

  it('defaults to the tasks tab and shows the recurring view after switching tabs', async () => {
    fakeTemplatesClient.listTemplates.mockResolvedValue([baseTemplate()]);
    const { scheduleState } = await import('../../stores/schedules.svelte');
    scheduleState.items = [baseSchedule()];

    const { getByRole, queryByRole, queryByText, getByText } = await renderView({
      fakeTemplatesClient,
    });

    expect(getByRole('searchbox', { name: /search tasks/i })).toBeTruthy();
    expect(queryByText('Recurring templates')).toBeNull();

    await fireEvent.click(getByRole('tab', { name: 'Recurring' }));
    await settle();

    expect(getByText('Recurring templates')).toBeTruthy();
    expect(queryByRole('searchbox', { name: /search tasks/i })).toBeNull();
  });

  it('switches to recurring and opens the matching template editor from task detail', async () => {
    const recurringTask = baseTask({
      id: 'task-recurring',
      title: 'Prepare weekly review',
      recurring_template_id: 'template-42',
    });
    const template = baseTemplate({
      id: 'template-42',
      title: 'Weekly review',
      description: 'Keep the loop closed',
    });

    fakeTasksClient.listTasks.mockResolvedValue([recurringTask]);
    fakeTemplatesClient.listTemplates.mockResolvedValue([template]);
    const { scheduleState } = await import('../../stores/schedules.svelte');
    scheduleState.items = [baseSchedule()];

    const { getByText, getByRole } = await renderView({ fakeTemplatesClient });

    await fireEvent.click(getByText('Prepare weekly review'));
    await settle();

    await fireEvent.click(getByText('Edit Template'));
    await Promise.resolve();
    await settle();

    expect(getByRole('tab', { name: 'Recurring' }).getAttribute('aria-selected')).toBe('true');
    expect(getByText('Edit recurring template')).toBeTruthy();
    expect((getByRole('textbox', { name: /title/i }) as HTMLInputElement).value).toBe(
      'Weekly review',
    );
  });

  it('"Make recurring" from the create form opens the recurring create editor', async () => {
    fakeTemplatesClient.createTemplate.mockResolvedValue(
      baseTemplate({ id: 'new-1', title: 'Gym' }),
    );

    const { scheduleState } = await import('../../stores/schedules.svelte');
    scheduleState.items = [baseSchedule()];
    scheduleState.loaded = true;

    const { getByText, getByPlaceholderText, getByRole } = await renderView({
      fakeTemplatesClient,
    });

    await fireEvent.click(getByText('+ New Task'));
    await tick();

    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    titleInput.value = 'Gym';
    await fireEvent.input(titleInput);
    await tick();

    await fireEvent.click(getByText('Make recurring'));
    await settle();

    expect(getByRole('tab', { name: 'Recurring' }).getAttribute('aria-selected')).toBe('true');
    expect(getByText('New recurring template')).toBeTruthy();

    await fireEvent.click(getByText('Create template'));
    await settle();

    expect(fakeTemplatesClient.createTemplate).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Gym', schedule_id: 'sched-1' }),
    );
  });
});

describe('TaskListView — keyboard shortcut n opens the create form', () => {
  it('invoking the n binding opens the create task form', async () => {
    const { getByText } = await renderView();

    const nBinding = activeShortcuts().find((b) => b.key === 'n' && b.group === 'Tasks');
    expect(nBinding).toBeTruthy();

    nBinding!.handler();
    await tick();

    expect(getByText('Create Task')).toBeTruthy();
  });
});
