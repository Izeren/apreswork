// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { baseTask } from './testFixtures';
import {
  installTaskListLifecycle,
  taskListViewFakeTasksClient,
  renderTaskListView,
  type MockedTasksClient,
} from './taskListViewTestSupport';
import { type TaskState } from '../../stores/tasks.svelte';
import type { Task } from '../../types';

installTaskListLifecycle();

// Per-test fakes and store — recreated in setupTaskListView (called from beforeEach).
let fakeTasksClient: MockedTasksClient;
let store: TaskState;

/** Flush the microtask queue and a Svelte tick. */
async function settle() {
  await Promise.resolve();
  await tick();
}

/**
 * Create fresh fakes for one test. Does NOT render; each test calls renderView()
 * when it is ready so it can mutate fakes between setup and first paint.
 */
function setupTaskListView(listResponse: Task[] = []) {
  fakeTasksClient = taskListViewFakeTasksClient(listResponse);
}

/**
 * Mount the view and await the load settle. Updates the module-level `store`
 * so test assertions can reach the injected TaskState without threading it
 * through every helper's return value.
 */
async function renderView() {
  const result = await renderTaskListView(fakeTasksClient);
  store = result.store;
  return result;
}

async function toggleFilterOption(
  getByRole: (role: string, options?: object) => HTMLElement,
  menu: 'status' | 'priority',
  name: string,
) {
  await fireEvent.click(getByRole('button', { name: new RegExp(`^${menu}:`, 'i') }));
  await tick();
  await fireEvent.click(getByRole('checkbox', { name }));
  await settle();
}

async function clickClearFilters(getByText: (matcher: RegExp) => HTMLElement) {
  await fireEvent.click(getByText(/clear filters/i));
  await settle();
}

async function renderWithWorkSelected() {
  const view = await renderView();
  await fireEvent.click(view.getByRole('button', { name: /work/ }));
  await settle();
  expect(store.filter.labels).toEqual(['work']);
  return view;
}

/**
 * Apply `gesture` to the `work` chip, then simulate a refetch whose visible tasks
 * no longer carry `work`; returns the now-zero-count chip so the caller can assert
 * its persisted pressed-state.
 */
async function selectThenDropWork(
  getByRole: (role: string, options?: object) => HTMLElement,
  gesture: (chip: HTMLElement) => Promise<unknown>,
) {
  await gesture(getByRole('button', { name: /work/ }));
  await settle();

  // Refetch results no longer carry 'work'.
  store.items = [baseTask({ id: 'lt-9', title: 'Task D', labels: ['errand'] })];
  await tick();

  return getByRole('button', { name: /work/ });
}

/**
 * Apply `gesture` to the `work` chip, click Clear filters, and assert the chip
 * reset to neutral and the filter fell back to the scheduled-only default.
 */
async function expectClearResetsWorkChip(
  view: Awaited<ReturnType<typeof renderView>>,
  gesture: (chip: HTMLElement) => Promise<unknown>,
) {
  const { getByRole, getByText, queryByText } = view;
  await gesture(getByRole('button', { name: /work/ }));
  await settle();
  expect(getByText(/clear filters/i)).toBeTruthy();

  await clickClearFilters(getByText);

  expect(queryByText(/clear filters/i)).toBeNull();
  expect(getByRole('button', { name: /work/ }).getAttribute('aria-pressed')).toBe('false');
  expect(store.filter).toEqual({ statuses: ['scheduled'] });
}

describe('TaskListView — clearFilters click', () => {
  beforeEach(async () => {
    await setupTaskListView();
  });

  it('clicking "Clear filters" hides the button and restores the default statuses', async () => {
    const { getByRole, getByText, queryByText } = await renderView();

    await toggleFilterOption(getByRole, 'status', 'Pending');

    const clearBtn = getByText(/clear filters/i);
    expect(clearBtn).toBeTruthy();

    await clickClearFilters(getByText);

    // Button disappears and the status selection is back to the baseline.
    expect(queryByText(/clear filters/i)).toBeNull();
    expect(getByRole('button', { name: 'Status: Scheduled' })).toBeTruthy();
    expect(store.filter).toEqual({ statuses: ['scheduled'] });
  });

  it('clicking "Clear filters" triggers a load call', async () => {
    const { getByRole, getByText } = await renderView();
    const callsAfterMount = fakeTasksClient.listTasks.mock.calls.length;

    await toggleFilterOption(getByRole, 'status', 'Backlog');

    await clickClearFilters(getByText);

    expect(fakeTasksClient.listTasks.mock.calls.length).toBeGreaterThan(callsAfterMount);
  });
});

describe('TaskListView — status filter defaults and persistence', () => {
  beforeEach(async () => {
    await setupTaskListView();
  });

  it('applies the scheduled-only default filter on mount', async () => {
    await renderView();

    expect(fakeTasksClient.listTasks).toHaveBeenCalledWith(
      expect.objectContaining({ statuses: ['scheduled'] }),
    );
  });

  it.each([
    { name: 'a multi-status selection', raw: '["backlog","pending"]', summary: 'Status: 2' },
    { name: 'the empty (All) selection', raw: '[]', summary: 'Status: All' },
    { name: 'junk (falls back to Scheduled)', raw: '["evil"]', summary: 'Status: Scheduled' },
  ])('restores $name from localStorage on mount', async ({ raw, summary }) => {
    window.localStorage.setItem('apreswork.taskList.statuses', raw);

    const { getByRole } = await renderView();

    expect(getByRole('button', { name: summary })).toBeTruthy();
  });

  it('restored empty selection places no status constraint on the load', async () => {
    window.localStorage.setItem('apreswork.taskList.statuses', '[]');

    await renderView();

    expect(fakeTasksClient.listTasks).toHaveBeenCalledWith(
      expect.objectContaining({ statuses: null }),
    );
  });

  it('persists the selection and refetches when a status is toggled', async () => {
    const { getByRole } = await renderView();

    await toggleFilterOption(getByRole, 'status', 'Backlog');

    expect(window.localStorage.getItem('apreswork.taskList.statuses')).toBe(
      '["scheduled","backlog"]',
    );
    expect(store.filter.statuses).toEqual(['scheduled', 'backlog']);
  });

  it('unticking the last status switches to All with no status constraint', async () => {
    const { getByRole } = await renderView();

    await toggleFilterOption(getByRole, 'status', 'Scheduled');

    expect(getByRole('button', { name: 'Status: All' })).toBeTruthy();
    expect(store.filter.statuses ?? null).toBeNull();
  });
});

describe('TaskListView — priority filter defaults and persistence', () => {
  beforeEach(async () => {
    await setupTaskListView();
  });

  it('defaults to All (no priority constraint) on a fresh profile', async () => {
    const { getByRole } = await renderView();

    expect(getByRole('button', { name: 'Priority: All' })).toBeTruthy();
    expect(fakeTasksClient.listTasks).toHaveBeenCalledWith(
      expect.objectContaining({ statuses: ['scheduled'] }),
    );
    expect(store.filter.priorities ?? null).toBeNull();
  });

  it.each([
    { name: 'a multi-priority selection', raw: '["High","Critical"]', summary: 'Priority: 2' },
    { name: 'junk (falls back to All)', raw: '["Urgent"]', summary: 'Priority: All' },
  ])('restores $name from localStorage on mount', async ({ raw, summary }) => {
    window.localStorage.setItem('apreswork.taskList.priorities', raw);

    const { getByRole } = await renderView();

    expect(getByRole('button', { name: summary })).toBeTruthy();
  });

  it('persists the selection and refetches when a priority is toggled', async () => {
    const { getByRole } = await renderView();

    await toggleFilterOption(getByRole, 'priority', 'High');

    expect(window.localStorage.getItem('apreswork.taskList.priorities')).toBe('["High"]');
    expect(store.filter.priorities).toEqual(['High']);
  });

  it('toggling a priority preserves the scheduled-only status filter', async () => {
    const { getByRole } = await renderView();

    await toggleFilterOption(getByRole, 'priority', 'Critical');

    expect(store.filter).toEqual({
      search_text: null,
      statuses: ['scheduled'],
      priorities: ['Critical'],
      labels: null,
      excluded_labels: null,
      unlabeled: null,
    });
    expect(getByRole('button', { name: 'Status: Scheduled' })).toBeTruthy();
  });

  it('Clear filters resets the priority selection to All and persists it', async () => {
    const { getByRole, getByText, queryByText } = await renderView();

    await toggleFilterOption(getByRole, 'priority', 'High');
    expect(getByText(/clear filters/i)).toBeTruthy();

    await clickClearFilters(getByText);

    expect(queryByText(/clear filters/i)).toBeNull();
    expect(getByRole('button', { name: 'Priority: All' })).toBeTruthy();
    expect(window.localStorage.getItem('apreswork.taskList.priorities')).toBe('[]');
    expect(store.filter).toEqual({ statuses: ['scheduled'] });
  });
});

describe('TaskListView — label filter chips', () => {
  // Chip counts derive from the visible (filtered) task set — there is no
  // separate labels endpoint call — so they always reflect active filters.
  const labelledTasks = [
    baseTask({ id: 'lt-1', title: 'Task A', labels: ['work'] }),
    baseTask({ id: 'lt-2', title: 'Task B', labels: ['work'] }),
    baseTask({ id: 'lt-3', title: 'Task C', labels: ['work', 'errand'] }),
  ];

  beforeEach(async () => {
    await setupTaskListView(labelledTasks);
  });

  it.each([
    { label: 'work', count: 3 },
    { label: 'errand', count: 1 },
  ])('derives the $label chip with count $count from visible tasks', async ({ label, count }) => {
    const { getByRole } = await renderView();
    const chip = getByRole('button', { name: new RegExp(label) });
    expect(chip.textContent).toContain(String(count));
  });

  it('renders no chip row when visible tasks carry no labels', async () => {
    fakeTasksClient.listTasks.mockResolvedValue([baseTask({ id: 'plain', title: 'Task P' })]);
    const { queryByRole } = await renderView();

    expect(queryByRole('group', { name: /filter by label/i })).toBeNull();
  });

  it('selecting chips builds a match-all label filter', async () => {
    const { getByRole } = await renderWithWorkSelected();
    expect(getByRole('button', { name: /work/ }).getAttribute('aria-pressed')).toBe('true');

    await fireEvent.click(getByRole('button', { name: /errand/ }));
    await settle();
    expect(store.filter.labels).toEqual(['work', 'errand']);
  });

  it('toggling a label chip preserves the scheduled-only status filter', async () => {
    const { getByRole } = await renderView();

    await fireEvent.click(getByRole('button', { name: /work/ }));
    await settle();

    expect(store.filter).toEqual({
      search_text: null,
      statuses: ['scheduled'],
      priorities: null,
      labels: ['work'],
      excluded_labels: null,
      unlabeled: null,
    });
    expect(getByRole('button', { name: 'Status: Scheduled' })).toBeTruthy();
  });

  it('deselecting the last chip clears the label filter', async () => {
    const { getByRole } = await renderView();

    await fireEvent.click(getByRole('button', { name: /work/ }));
    await fireEvent.click(getByRole('button', { name: /work/ }));
    await settle();

    expect(store.filter.labels ?? null).toBeNull();
    expect(getByRole('button', { name: /work/ }).getAttribute('aria-pressed')).toBe('false');
  });

  it('Clear filters resets selected chips', async () => {
    await expectClearResetsWorkChip(await renderView(), (c) => fireEvent.click(c));
  });

  it('keeps a selected label visible at zero when it leaves the results', async () => {
    const { getByRole } = await renderView();
    const chip = await selectThenDropWork(getByRole, (c) => fireEvent.click(c));
    expect(chip.getAttribute('aria-pressed')).toBe('true');
    expect(chip.textContent).toContain('0');
    expect(getByRole('button', { name: /errand/ }).textContent).toContain('1');
  });
});

describe('TaskListView — label exclusion chips', () => {
  const labelledTasks = [
    baseTask({ id: 'lt-1', title: 'Task A', labels: ['work'] }),
    baseTask({ id: 'lt-2', title: 'Task B', labels: ['work', 'errand'] }),
  ];

  beforeEach(async () => {
    await setupTaskListView(labelledTasks);
  });

  it.each([
    { gesture: 'right-click', apply: (chip: HTMLElement) => fireEvent.contextMenu(chip) },
    {
      gesture: 'ctrl+click',
      apply: (chip: HTMLElement) => fireEvent.click(chip, { ctrlKey: true }),
    },
  ])('$gesture excludes a neutral chip and sends the match-none filter', async ({ apply }) => {
    const { getByRole } = await renderView();

    const chip = getByRole('button', { name: /work/ });
    await apply(chip);
    await settle();

    expect(chip.getAttribute('aria-pressed')).toBe('mixed');
    expect(chip.classList.contains('excluded')).toBe(true);
    expect(store.filter).toEqual({
      search_text: null,
      statuses: ['scheduled'],
      priorities: null,
      labels: null,
      excluded_labels: ['work'],
      unlabeled: null,
    });
  });

  it('right-click on an included chip flips include → exclude (never both)', async () => {
    const { getByRole } = await renderWithWorkSelected();

    await fireEvent.contextMenu(getByRole('button', { name: /work/ }));
    await settle();

    expect(store.filter.labels ?? null).toBeNull();
    expect(store.filter.excluded_labels).toEqual(['work']);
  });

  it('left click on an excluded chip clears it; the next click includes it', async () => {
    const { getByRole } = await renderView();

    await fireEvent.contextMenu(getByRole('button', { name: /work/ }));
    await settle();

    await fireEvent.click(getByRole('button', { name: /work/ }));
    await settle();
    expect(store.filter.excluded_labels ?? null).toBeNull();
    expect(store.filter.labels ?? null).toBeNull();
    expect(getByRole('button', { name: /work/ }).getAttribute('aria-pressed')).toBe('false');

    await fireEvent.click(getByRole('button', { name: /work/ }));
    await settle();
    expect(store.filter.labels).toEqual(['work']);
  });

  it('right-click on an excluded chip clears the exclusion', async () => {
    const { getByRole } = await renderView();

    await fireEvent.contextMenu(getByRole('button', { name: /work/ }));
    await settle();
    await fireEvent.contextMenu(getByRole('button', { name: /work/ }));
    await settle();

    expect(store.filter.excluded_labels ?? null).toBeNull();
    expect(getByRole('button', { name: /work/ }).getAttribute('aria-pressed')).toBe('false');
  });

  it('keeps an excluded label visible at zero when its carriers drop out', async () => {
    const { getByRole } = await renderView();
    const chip = await selectThenDropWork(getByRole, (c) => fireEvent.contextMenu(c));
    expect(chip.getAttribute('aria-pressed')).toBe('mixed');
    expect(chip.textContent).toContain('0');
  });

  it('Clear filters resets exclusions', async () => {
    await expectClearResetsWorkChip(await renderView(), (c) => fireEvent.contextMenu(c));
  });
});

describe('TaskListView — unlabeled pseudo-chip', () => {
  const mixedTasks = [
    baseTask({ id: 'ul-1', title: 'Labeled task', labels: ['work'] }),
    baseTask({ id: 'ul-2', title: 'Plain task' }),
  ];

  beforeEach(async () => {
    await setupTaskListView(mixedTasks);
  });

  function unlabeledChip(getByRole: (role: string, options?: object) => HTMLElement) {
    return getByRole('button', { name: /unlabeled/ });
  }

  it('renders pinned first with the count of visible unlabeled tasks', async () => {
    const { getByRole } = await renderView();

    const group = getByRole('group', { name: /filter by label/i });
    const chips = Array.from(group.querySelectorAll('button.label-filter-chip'));
    expect(chips[0]?.classList.contains('meta')).toBe(true);
    expect(chips[0]?.textContent).toContain('unlabeled');
    expect(chips[0]?.textContent).toContain('1');
    expect(chips[0]?.getAttribute('aria-pressed')).toBe('false');
  });

  it.each([
    {
      gesture: 'left-click',
      apply: (chip: HTMLElement) => fireEvent.click(chip),
      expectedPressed: 'true',
      expectedExcluded: false,
      expectedUnlabeled: true as boolean | null,
    },
    {
      gesture: 'right-click',
      apply: (chip: HTMLElement) => fireEvent.contextMenu(chip),
      expectedPressed: 'mixed',
      expectedExcluded: true,
      expectedUnlabeled: false as boolean | null,
    },
  ])(
    '$gesture on unlabeled chip: aria-pressed=$expectedPressed, excluded=$expectedExcluded',
    async ({ apply, expectedPressed, expectedExcluded, expectedUnlabeled }) => {
      const { getByRole } = await renderView();

      await apply(unlabeledChip(getByRole));
      await settle();

      const chip = unlabeledChip(getByRole);
      expect(chip.getAttribute('aria-pressed')).toBe(expectedPressed);
      expect(chip.classList.contains('excluded')).toBe(expectedExcluded);
      expect(store.filter).toEqual({
        search_text: null,
        statuses: ['scheduled'],
        priorities: null,
        labels: null,
        excluded_labels: null,
        unlabeled: expectedUnlabeled,
      });
    },
  );

  it('a second left click clears the presence filter', async () => {
    const { getByRole } = await renderView();

    await fireEvent.click(unlabeledChip(getByRole));
    await settle();
    await fireEvent.click(unlabeledChip(getByRole));
    await settle();

    expect(unlabeledChip(getByRole).getAttribute('aria-pressed')).toBe('false');
    expect(store.filter.unlabeled ?? null).toBeNull();
  });

  it('keeps the chip bar up when no visible task carries a label', async () => {
    const { getByRole } = await renderView();

    await fireEvent.click(unlabeledChip(getByRole));
    await settle();

    // Simulate the refetch: only the unlabeled task remains visible.
    store.items = [mixedTasks[1]];
    await tick();

    expect(getByRole('group', { name: /filter by label/i })).toBeTruthy();
    expect(unlabeledChip(getByRole).getAttribute('aria-pressed')).toBe('true');
  });

  it('Clear filters resets the pseudo-chip', async () => {
    const { getByRole, getByText } = await renderView();

    await fireEvent.contextMenu(unlabeledChip(getByRole));
    await settle();

    await clickClearFilters(getByText);

    expect(unlabeledChip(getByRole).getAttribute('aria-pressed')).toBe('false');
    expect(store.filter).toEqual({ statuses: ['scheduled'] });
  });
});

describe('TaskListView — label chip top-N collapse', () => {
  // Labels arrive alphabetically with ascending counts, so the count-desc
  // order the UI must apply is the exact reverse (highest first).
  const makeLabels = (n: number) =>
    Array.from({ length: n }, (_, i) => ({
      label: `label-${String(i + 1).padStart(2, '0')}`,
      task_count: i + 1,
    }));

  // Expected chip order for makeLabels(total): counts descend from `total`.
  const countDesc = (total: number, take: number) =>
    Array.from({ length: take }, (_, i) => `label-${String(total - i).padStart(2, '0')}`);

  // Real label chips only — the pinned "unlabeled" pseudo-chip (.meta) is
  // outside the sort/collapse machinery under test here.
  const chipNames = (group: HTMLElement) =>
    Array.from(group.querySelectorAll('button.label-filter-chip:not(.meta)')).map(
      (b) => b.textContent?.trim().split(/\s+/)[0] ?? '',
    );

  // Smallest task set realizing the given per-label counts: task k (1-based)
  // carries every label whose count is ≥ k, so label i appears in exactly
  // `task_count` tasks while only max(count) rows render.
  const tasksCarrying = (labels: { label: string; task_count: number }[]) => {
    const maxCount = Math.max(0, ...labels.map((l) => l.task_count));
    return Array.from({ length: maxCount }, (_, k) =>
      baseTask({
        id: `fixture-${k + 1}`,
        title: `Fixture ${k + 1}`,
        labels: labels.filter((l) => l.task_count > k).map((l) => l.label),
      }),
    );
  };

  async function renderWithLabels(labels: { label: string; task_count: number }[]) {
    fakeTasksClient.listTasks.mockResolvedValue(tasksCarrying(labels));
    return renderView();
  }

  /**
   * From the 15-label fixture: expand the overflow, apply `gesture` to `label-01`
   * (an overflow chip), then collapse — asserting the now-active label-01 ranks
   * first ahead of the top neutral chips.
   */
  async function frontRanksOverflowLabel(
    getByRole: (role: string, options?: object) => HTMLElement,
    gesture: (chip: HTMLElement) => Promise<unknown>,
  ) {
    await fireEvent.click(getByRole('button', { name: '+3 more' }));
    await tick();
    await gesture(getByRole('button', { name: /label-01/ }));
    await settle();
    await fireEvent.click(getByRole('button', { name: 'Show less' }));
    await tick();

    const group = getByRole('group', { name: /filter by label/i });
    expect(chipNames(group)).toEqual(['label-01', ...countDesc(15, 11)]);
  }

  beforeEach(async () => {
    await setupTaskListView();
  });

  it.each([
    { total: 11, hidden: 0 },
    { total: 12, hidden: 0 },
    { total: 13, hidden: 1 },
    { total: 15, hidden: 3 },
  ])(
    'with $total labels shows the top set count-desc and hides $hidden behind the expander',
    async ({ total, hidden }) => {
      const { getByRole, queryByRole } = await renderWithLabels(makeLabels(total));

      const group = getByRole('group', { name: /filter by label/i });
      expect(chipNames(group)).toEqual(countDesc(total, total - hidden));
      if (hidden === 0) {
        expect(queryByRole('button', { name: /more$/ })).toBeNull();
        expect(queryByRole('button', { name: 'Show less' })).toBeNull();
      } else {
        expect(getByRole('button', { name: `+${hidden} more` })).toBeTruthy();
      }
    },
  );

  it('breaks equal counts alphabetically', async () => {
    const { getByRole } = await renderWithLabels([
      { label: 'beta', task_count: 2 },
      { label: 'alpha', task_count: 2 },
      { label: 'gamma', task_count: 5 },
    ]);

    const group = getByRole('group', { name: /filter by label/i });
    expect(chipNames(group)).toEqual(['gamma', 'alpha', 'beta']);
  });

  it('"+N more" expands to all labels and "Show less" collapses again', async () => {
    const { getByRole, queryByRole } = await renderWithLabels(makeLabels(15));

    await fireEvent.click(getByRole('button', { name: '+3 more' }));
    await tick();

    const group = getByRole('group', { name: /filter by label/i });
    expect(chipNames(group)).toEqual(countDesc(15, 15));
    expect(getByRole('button', { name: 'Show less' })).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: 'Show less' }));
    await tick();

    expect(chipNames(group)).toEqual(countDesc(15, 12));
    expect(queryByRole('button', { name: /label-01/ })).toBeNull();
  });

  it.each([
    { kind: 'selected', gesture: (c: HTMLElement) => fireEvent.click(c), expectedPressed: 'true' },
    {
      kind: 'excluded',
      gesture: (c: HTMLElement) => fireEvent.contextMenu(c),
      expectedPressed: 'mixed',
    },
  ])(
    'moves a $kind overflow label to the front when collapsed',
    async ({ gesture, expectedPressed }) => {
      const { getByRole } = await renderWithLabels(makeLabels(15));
      await frontRanksOverflowLabel(getByRole, gesture);
      expect(getByRole('button', { name: /label-01/ }).getAttribute('aria-pressed')).toBe(
        expectedPressed,
      );
      expect(getByRole('button', { name: '+3 more' })).toBeTruthy();
    },
  );

  it('orders chips included first, excluded second, then neutral by usage', async () => {
    const { getByRole } = await renderWithLabels(makeLabels(5));

    // label-01 and label-02 have the LOWEST counts — only their active state
    // can rank them ahead of the neutral chips.
    await fireEvent.click(getByRole('button', { name: /label-01/ }));
    await settle();
    await fireEvent.contextMenu(getByRole('button', { name: /label-02/ }));
    await settle();

    const group = getByRole('group', { name: /filter by label/i });
    expect(chipNames(group)).toEqual(['label-01', 'label-02', ...countDesc(5, 3)]);
  });
});
