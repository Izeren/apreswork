// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Priority, TaskStatus } from '../../types';
import { PRIORITIES, TASK_STATUSES } from '../../types';
import FilterDropdown from './FilterDropdown.svelte';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function renderStatusDropdown(selected: TaskStatus[] = []) {
  const onchange = vi.fn();
  const utils = render(FilterDropdown, {
    props: { label: 'Status', options: TASK_STATUSES, selected, onchange },
  });
  return { ...utils, onchange };
}

describe('FilterDropdown — summary button', () => {
  it.each([
    { selected: [] as TaskStatus[], summary: 'Status: All' },
    { selected: ['scheduled'] as TaskStatus[], summary: 'Status: Scheduled' },
    { selected: ['backlog'] as TaskStatus[], summary: 'Status: Backlog' },
    { selected: ['backlog', 'pending'] as TaskStatus[], summary: 'Status: 2' },
    {
      selected: ['backlog', 'pending', 'scheduled', 'completed', 'cancelled'] as TaskStatus[],
      summary: 'Status: 5',
    },
  ])('summarizes $selected as "$summary"', ({ selected, summary }) => {
    const { getByRole } = renderStatusDropdown(selected);
    expect(getByRole('button', { name: summary })).toBeTruthy();
  });

  it('is collapsed initially and expands on click', async () => {
    const { getByRole, queryByRole } = renderStatusDropdown();
    const button = getByRole('button');
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(queryByRole('group', { name: /filter by status/i })).toBeNull();

    await fireEvent.click(button);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    expect(getByRole('group', { name: /filter by status/i })).toBeTruthy();
  });
});

describe('FilterDropdown — checkbox popover', () => {
  async function openDropdown(selected: TaskStatus[] = []) {
    const utils = renderStatusDropdown(selected);
    await fireEvent.click(utils.getByRole('button', { name: /^status:/i }));
    return utils;
  }

  it('renders one checkbox per option, ticking the selected ones', async () => {
    const { getAllByRole, getByRole } = await openDropdown(['pending', 'cancelled']);

    expect(getAllByRole('checkbox')).toHaveLength(5);
    for (const { name, checked } of [
      { name: 'Backlog', checked: false },
      { name: 'Pending', checked: true },
      { name: 'Scheduled', checked: false },
      { name: 'Completed', checked: false },
      { name: 'Cancelled', checked: true },
    ]) {
      expect((getByRole('checkbox', { name }) as HTMLInputElement).checked).toBe(checked);
    }
  });

  it.each([
    {
      name: 'ticking adds the option',
      selected: ['scheduled'] as TaskStatus[],
      toggle: 'Backlog',
      expected: ['scheduled', 'backlog'],
    },
    {
      name: 'unticking removes the option',
      selected: ['scheduled', 'backlog'] as TaskStatus[],
      toggle: 'Backlog',
      expected: ['scheduled'],
    },
    {
      name: 'unticking the last option yields the empty (All) selection',
      selected: ['scheduled'] as TaskStatus[],
      toggle: 'Scheduled',
      expected: [],
    },
  ])('$name', async ({ selected, toggle, expected }) => {
    const { getByRole, onchange } = await openDropdown(selected);

    await fireEvent.click(getByRole('checkbox', { name: toggle }));

    expect(onchange).toHaveBeenCalledExactlyOnceWith(expected);
  });

  it('closes on Escape and returns focus to the button', async () => {
    const { getByRole, queryByRole } = await openDropdown();

    await fireEvent.keyDown(window, { key: 'Escape' });
    await tick();

    expect(queryByRole('group', { name: /filter by status/i })).toBeNull();
    const button = getByRole('button', { name: /^status:/i });
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(button);
  });

  it('closes on pointerdown outside', async () => {
    const { getByRole, queryByRole } = await openDropdown();

    await fireEvent.pointerDown(document.body);
    await tick();

    expect(queryByRole('group', { name: /filter by status/i })).toBeNull();
    expect(getByRole('button').getAttribute('aria-expanded')).toBe('false');
  });

  it('stays open on pointerdown inside the popover', async () => {
    const { getByRole } = await openDropdown();

    await fireEvent.pointerDown(getByRole('checkbox', { name: 'Backlog' }));
    await tick();

    expect(getByRole('group', { name: /filter by status/i })).toBeTruthy();
  });
});

describe('FilterDropdown — priority instance', () => {
  function renderPriorityDropdown(selected: Priority[] = []) {
    const onchange = vi.fn();
    const utils = render(FilterDropdown, {
      props: { label: 'Priority', options: PRIORITIES, selected, onchange },
    });
    return { ...utils, onchange };
  }

  it.each([
    { selected: [] as Priority[], summary: 'Priority: All' },
    { selected: ['High'] as Priority[], summary: 'Priority: High' },
    { selected: ['High', 'Critical'] as Priority[], summary: 'Priority: 2' },
  ])('summarizes $selected as "$summary"', ({ selected, summary }) => {
    const { getByRole } = renderPriorityDropdown(selected);
    expect(getByRole('button', { name: summary })).toBeTruthy();
  });

  it('lists every priority in display order and reports toggles', async () => {
    const { getByRole, getAllByRole, onchange } = renderPriorityDropdown(['High']);

    await fireEvent.click(getByRole('button', { name: /^priority:/i }));
    expect(getByRole('group', { name: /filter by priority/i })).toBeTruthy();
    const checkboxes = getAllByRole('checkbox');
    expect(checkboxes).toHaveLength(PRIORITIES.length);

    await fireEvent.click(getByRole('checkbox', { name: 'Critical' }));
    expect(onchange).toHaveBeenCalledExactlyOnceWith(['High', 'Critical']);
  });
});
