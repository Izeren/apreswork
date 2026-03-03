// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import type { TaskStatus } from '../../types';
import TaskRow from './TaskRow.svelte';
import { baseTask } from './testFixtures';
import { statusCases } from '../../testFixtures';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const progressCases: Array<{
  time_logged_minutes: number;
  duration_minutes: number;
  expectedPct: string;
  label: string;
}> = [
  { time_logged_minutes: 0, duration_minutes: 60, expectedPct: 'width: 0%', label: '0%' },
  { time_logged_minutes: 30, duration_minutes: 60, expectedPct: 'width: 50%', label: '50%' },
  { time_logged_minutes: 60, duration_minutes: 60, expectedPct: 'width: 100%', label: '100%' },
  {
    time_logged_minutes: 90,
    duration_minutes: 60,
    expectedPct: 'width: 100%',
    label: 'capped at 100% when over',
  },
  {
    time_logged_minutes: 10,
    duration_minutes: 0,
    expectedPct: 'width: 0%',
    label: 'shows 0% when duration_minutes is 0 (no divide-by-zero)',
  },
];

describe('TaskRow — rendering', () => {
  it.each([
    {
      prop: { title: 'My Important Task' },
      expectedText: 'My Important Task',
      label: 'task title',
    },
    { prop: { priority: 'Critical' as const }, expectedText: 'Critical', label: 'priority badge' },
    {
      prop: { status: 'scheduled' as TaskStatus },
      expectedText: 'Scheduled',
      label: 'status badge with capitalised label',
    },
  ])('renders $label', ({ prop, expectedText }) => {
    const task = baseTask(prop);
    const { getByText } = render(TaskRow, { task, onselect: vi.fn() });
    expect(getByText(expectedText)).toBeTruthy();
  });

  it.each([
    {
      label: 'formatted date when deadline is set',
      deadline: '2026-06-15T00:00:00Z' as string | null,
      check: (el: Element) => {
        // formatShortDate returns "Jun 15" (locale-dependent) or similar short form
        expect(el.textContent).not.toBe('');
      },
    },
    {
      label: '"—" when deadline is null',
      deadline: null as string | null,
      check: (el: Element) => {
        expect(el.textContent).toBe('—');
      },
    },
  ])('deadline: $label', ({ deadline, check }) => {
    const task = baseTask({ deadline });
    const { container } = render(TaskRow, { task, onselect: vi.fn() });
    const deadlineEl = container.querySelector('.row-deadline');
    expect(deadlineEl).toBeTruthy();
    check(deadlineEl!);
  });

  it.each([
    { labels: ['frontend', 'urgent'], expectedCount: 2 },
    { labels: [], expectedCount: 0 },
  ])('renders $expectedCount label chip(s)', ({ labels, expectedCount }) => {
    const task = baseTask({ labels });
    const { container } = render(TaskRow, { task, onselect: vi.fn() });
    expect(container.querySelectorAll('.label-chip')).toHaveLength(expectedCount);
  });

  it.each([
    { logged: 60, total: 120, expected: '1h / 2h' },
    { logged: 0, total: 120, expected: '0m / 2h' },
  ])('progress text: $logged/$total min → "$expected"', ({ logged, total, expected }) => {
    const task = baseTask({ duration_minutes: total, time_logged_minutes: logged });
    const { getByText } = render(TaskRow, { task, onselect: vi.fn() });
    expect(getByText(expected)).toBeTruthy();
  });

  it.each([
    { status: 'backlog' as TaskStatus, expectedChecked: 'false' as string | null },
    { status: 'pending' as TaskStatus, expectedChecked: 'true' as string | null },
    { status: 'scheduled' as TaskStatus, expectedChecked: 'true' as string | null },
    { status: 'completed' as TaskStatus, expectedChecked: null },
    { status: 'cancelled' as TaskStatus, expectedChecked: null },
  ])('scheduling switch for $status', ({ status, expectedChecked }) => {
    const task = baseTask({ status });
    const { queryByRole } = render(TaskRow, { task, onselect: vi.fn(), ontoggleschedule: vi.fn() });
    const sw = queryByRole('switch');
    expect(sw?.getAttribute('aria-checked') ?? null).toBe(expectedChecked);
  });
});

describe('TaskRow — progress bar', () => {
  it.each(progressCases)(
    'progress $label: logged=$time_logged_minutes / total=$duration_minutes → fill $expectedPct',
    ({ time_logged_minutes, duration_minutes, expectedPct }) => {
      const task = baseTask({ time_logged_minutes, duration_minutes });
      const { container } = render(TaskRow, { task, onselect: vi.fn() });
      const fill = container.querySelector('.progress-fill') as HTMLElement;
      expect(fill).toBeTruthy();
      expect(fill.style.width).toBe(expectedPct.replace('width: ', ''));
    },
  );
});

describe('TaskRow — selected state', () => {
  it.each([
    { selected: false, expectClass: false },
    { selected: true, expectClass: true },
  ])('applies selected class when selected=$selected', ({ selected, expectClass }) => {
    const task = baseTask();
    const { container } = render(TaskRow, { task, onselect: vi.fn(), selected });
    expect(container.querySelector('.task-row')!.classList.contains('selected')).toBe(expectClass);
  });
});

describe('TaskRow — onselect callback', () => {
  it.each([
    { label: 'click', trigger: (el: Element) => fireEvent.click(el), expectedCalls: 1 },
    {
      label: 'Enter key',
      trigger: (el: Element) => fireEvent.keyDown(el, { key: 'Enter' }),
      expectedCalls: 1,
    },
    {
      label: 'Space key',
      trigger: (el: Element) => fireEvent.keyDown(el, { key: ' ' }),
      expectedCalls: 1,
    },
    {
      label: 'Tab key (no-op)',
      trigger: (el: Element) => fireEvent.keyDown(el, { key: 'Tab' }),
      expectedCalls: 0,
    },
  ])('onselect calls on $label: $expectedCalls time(s)', async ({ trigger, expectedCalls }) => {
    const onselect = vi.fn();
    const task = baseTask();
    const { container } = render(TaskRow, { task, onselect });
    await trigger(container.querySelector('.task-row')!);
    expect(onselect).toHaveBeenCalledTimes(expectedCalls);
  });

  it('clicking the scheduling switch toggles schedule without selecting the row', async () => {
    const onselect = vi.fn();
    const ontoggleschedule = vi.fn();
    const task = baseTask({ status: 'backlog' });
    const { getByRole } = render(TaskRow, { task, onselect, ontoggleschedule });

    await fireEvent.click(getByRole('switch'));

    expect(onselect).not.toHaveBeenCalled();
    expect(ontoggleschedule).toHaveBeenCalledWith(task, 'pending');
  });
});

describe('TaskRow — kebab menu', () => {
  it('renders no kebab when onmenu is not provided', () => {
    const { queryByRole } = render(TaskRow, { task: baseTask(), onselect: vi.fn() });
    expect(queryByRole('button', { name: /task actions/i })).toBeNull();
  });

  it('clicking the kebab opens the menu without selecting the row', async () => {
    const onselect = vi.fn();
    const onmenu = vi.fn();
    const task = baseTask();
    const { getByRole } = render(TaskRow, { task, onselect, onmenu });

    await fireEvent.click(getByRole('button', { name: /task actions/i }));

    expect(onmenu).toHaveBeenCalledTimes(1);
    expect(onmenu.mock.calls[0]?.[0]).toEqual(task);
    expect(onselect).not.toHaveBeenCalled();
  });

  it.each([
    {
      label: 'Shift+F10',
      trigger: (el: Element) => fireEvent.keyDown(el, { key: 'F10', shiftKey: true }),
      x: 0,
      y: 0,
    },
    {
      label: 'ContextMenu key',
      trigger: (el: Element) => fireEvent.keyDown(el, { key: 'ContextMenu' }),
      x: 0,
      y: 0,
    },
    {
      label: 'right-click at pointer',
      trigger: (el: Element) => fireEvent.contextMenu(el, { clientX: 120, clientY: 240 }),
      x: 120,
      y: 240,
    },
  ])('$label opens the menu without selecting', async ({ trigger, x, y }) => {
    const onselect = vi.fn();
    const onmenu = vi.fn();
    const task = baseTask();
    const { container } = render(TaskRow, { task, onselect, onmenu });
    await trigger(container.querySelector('.task-row')!);
    expect(onmenu).toHaveBeenCalledWith(task, x, y);
    expect(onselect).not.toHaveBeenCalled();
  });

  it('right-click without onmenu does not throw', async () => {
    const task = baseTask();
    const { container } = render(TaskRow, { task, onselect: vi.fn() });

    await fireEvent.contextMenu(container.querySelector('.task-row')!);

    // Still rendered — the handler tolerated the missing callback.
    expect(container.querySelector('.task-row')).toBeTruthy();
  });
});

describe('TaskRow — status badge for each TaskStatus', () => {
  it.each(statusCases)('renders "$status" status badge as "$label"', ({ status, label }) => {
    const task = baseTask({ status });
    const { getByText } = render(TaskRow, { task, onselect: vi.fn() });
    expect(getByText(label)).toBeTruthy();
  });
});
