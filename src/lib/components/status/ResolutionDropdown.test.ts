// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Snippet } from 'svelte';
import ResolutionDropdown, { resolutionMenuItems } from './ResolutionDropdown.svelte';
import {
  nextWeekDeadline,
  nextMonthDeadline,
  todayDeadline,
  tomorrowDeadline,
  customDeadlineIso,
} from '../shared/deadlinePresets';
import type { ContextMenuItem, TaskActions } from '../../actions/taskActions';
import type { ScheduleWarning } from '../../types';
import { formatDateTime } from '../../utils';
import { isoToLocalDate } from '../shared/dateTimePickerShared';
import { DEADLINE_WARNING, BLOCKING_WARNING } from './testFixtures';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const DEADLINE_ISO = '2026-07-01T10:00:00Z';
const EARLIEST_ISO = '2026-07-20T18:00:00Z';

/** Menu-open instant for the pure policy tests (local-time constructor). */
const NOW = new Date(2026, 6, 17, 15, 0, 0);

/** The pure builders only store the snippet — any opaque value works. */
const fakeSnippet = (() => {}) as unknown as Snippet;

function fakeTaskActions() {
  const mock = {
    extendDeadline: vi.fn().mockResolvedValue(undefined),
    doNow: vi.fn().mockResolvedValue(undefined),
    completeTask: vi.fn().mockResolvedValue(undefined),
    cancelTask: vi.fn().mockResolvedValue(undefined),
  };
  return mock as unknown as TaskActions & typeof mock;
}

/** Preset labels carry a formatted date suffix — match on the stable prefix. */
function itemByLabel(items: ContextMenuItem[], label: string): ContextMenuItem {
  const item = items.find((i) => i.label.startsWith(label));
  if (!item) throw new Error(`No menu item labelled "${label}"`);
  return item;
}

// Local-time constructors + local-component assertions keep these cases
// timezone-independent (the helpers preserve local wall-clock time).
describe('deadline presets', () => {
  it('nextWeekDeadline adds seven days, keeping the time of day', () => {
    const result = new Date(nextWeekDeadline(new Date(2026, 6, 10, 18, 30)));
    expect([result.getFullYear(), result.getMonth(), result.getDate()]).toEqual([2026, 6, 17]);
    expect([result.getHours(), result.getMinutes()]).toEqual([18, 30]);
  });

  it.each([
    {
      label: 'normal month transition',
      input: new Date(2026, 6, 10, 18, 30),
      expectedYMD: [2026, 7, 10],
      expectedHM: [18, 30],
    },
    {
      label: 'clamps to last day of shorter month',
      input: new Date(2026, 0, 31, 12, 0),
      expectedYMD: [2026, 1, 28],
      expectedHM: [12, 0],
    },
  ])('nextMonthDeadline $label', ({ input, expectedYMD, expectedHM }) => {
    const result = new Date(nextMonthDeadline(input));
    expect([result.getFullYear(), result.getMonth(), result.getDate()]).toEqual(expectedYMD);
    expect([result.getHours(), result.getMinutes()]).toEqual(expectedHM);
  });
});

describe('todayDeadline', () => {
  it('returns end-of-day today (23:59)', () => {
    const result = new Date(todayDeadline(new Date(2026, 6, 17, 15, 30)));
    expect([result.getFullYear(), result.getMonth(), result.getDate()]).toEqual([2026, 6, 17]);
    expect([result.getHours(), result.getMinutes()]).toEqual([23, 59]);
  });
});

describe('tomorrowDeadline', () => {
  it.each([
    {
      label: 'normal day',
      input: new Date(2026, 6, 17, 15, 30),
      expectedYMD: [2026, 6, 18],
    },
    {
      label: 'month-end rolls over',
      input: new Date(2026, 0, 31, 8, 0),
      expectedYMD: [2026, 1, 1],
    },
    {
      label: 'year-end rolls over',
      input: new Date(2026, 11, 31, 22, 0),
      expectedYMD: [2027, 0, 1],
    },
  ])('tomorrowDeadline $label returns end-of-day (23:59)', ({ input, expectedYMD }) => {
    const result = new Date(tomorrowDeadline(input));
    expect([result.getFullYear(), result.getMonth(), result.getDate()]).toEqual(expectedYMD);
    expect([result.getHours(), result.getMinutes()]).toEqual([23, 59]);
  });
});

describe('customDeadlineIso', () => {
  it('always anchors the picked local date at end of day (23:59)', () => {
    const result = new Date(customDeadlineIso('2026-07-25'));
    expect([result.getFullYear(), result.getMonth(), result.getDate()]).toEqual([2026, 6, 25]);
    expect([result.getHours(), result.getMinutes()]).toEqual([23, 59]);
  });
});

describe('resolutionMenuItems', () => {
  it.each([
    {
      label: 'deadline violation',
      warning: DEADLINE_WARNING,
      expectedLabels: [
        `Extend to today (${formatDateTime(todayDeadline(NOW))})`,
        `Extend to tomorrow (${formatDateTime(tomorrowDeadline(NOW))})`,
        `Extend to next week (${formatDateTime(nextWeekDeadline(NOW))})`,
        `Extend to next month (${formatDateTime(nextMonthDeadline(NOW))})`,
        `Extend to scheduled date (${formatDateTime(EARLIEST_ISO)})`,
        'Custom deadline',
        'Do now',
        'Complete task',
        'Cancel task',
      ],
    },
    {
      label: 'unschedulable task (blocking)',
      warning: BLOCKING_WARNING,
      expectedLabels: [
        `Extend to today (${formatDateTime(todayDeadline(NOW))})`,
        `Extend to tomorrow (${formatDateTime(tomorrowDeadline(NOW))})`,
        `Extend to next week (${formatDateTime(nextWeekDeadline(NOW))})`,
        `Extend to next month (${formatDateTime(nextMonthDeadline(NOW))})`,
        'Custom deadline',
        'Do now',
        'Complete task',
        'Cancel task',
      ],
    },
  ])(
    'resolutionMenuItems hides scheduled-date option for $label',
    ({ warning, expectedLabels }) => {
      const items = resolutionMenuItems(warning, fakeTaskActions(), fakeSnippet, NOW);
      expect(items.map((i) => i.label)).toEqual(expectedLabels);
    },
  );

  it.each([
    { verb: 'today', preset: todayDeadline },
    { verb: 'tomorrow', preset: tomorrowDeadline },
    { verb: 'next week', preset: nextWeekDeadline },
    { verb: 'next month', preset: nextMonthDeadline },
  ])(
    'Extend to $verb preset action sends exactly the datetime shown in the label',
    ({ verb, preset }) => {
      const fake = fakeTaskActions();
      const items = resolutionMenuItems(DEADLINE_WARNING, fake, fakeSnippet, NOW);
      void itemByLabel(items, `Extend to ${verb}`).action?.();
      expect(fake.extendDeadline).toHaveBeenCalledWith('task-1', preset(NOW));
    },
  );

  it('"Extend to scheduled date" sends earliest_completion verbatim', () => {
    const fake = fakeTaskActions();
    const items = resolutionMenuItems(DEADLINE_WARNING, fake, fakeSnippet, NOW);

    void itemByLabel(items, 'Extend to scheduled date').action?.();

    expect(fake.extendDeadline).toHaveBeenCalledWith('task-1', EARLIEST_ISO);
  });

  it('"Custom deadline" carries the calendar submenu instead of a verb', () => {
    const fake = fakeTaskActions();
    const items = resolutionMenuItems(DEADLINE_WARNING, fake, fakeSnippet, NOW);

    const custom = itemByLabel(items, 'Custom deadline');

    expect(custom.submenu).toBe(fakeSnippet);
    expect(custom.action).toBeUndefined();
    expect(fake.extendDeadline).not.toHaveBeenCalled();
  });

  it.each([
    {
      verb: 'Do now',
      getItem: (items: ContextMenuItem[]) => itemByLabel(items, 'Do now'),
      getAction: (fake: ReturnType<typeof fakeTaskActions>) => fake.doNow,
      expectedArgs: ['task-1', expect.any(Date)] as unknown[],
      expectDestructive: false,
    },
    {
      verb: 'Complete task',
      getItem: (items: ContextMenuItem[]) => itemByLabel(items, 'Complete task'),
      getAction: (fake: ReturnType<typeof fakeTaskActions>) => fake.completeTask,
      expectedArgs: ['task-1', 'Alpha task'] as unknown[],
      expectDestructive: false,
    },
    {
      verb: 'Cancel task',
      getItem: (items: ContextMenuItem[]) => itemByLabel(items, 'Cancel task'),
      getAction: (fake: ReturnType<typeof fakeTaskActions>) => fake.cancelTask,
      expectedArgs: ['task-1', 'Alpha task'] as unknown[],
      expectDestructive: true,
    },
  ])(
    'dispatches $verb with the task id and title',
    ({ getItem, getAction, expectedArgs, expectDestructive }) => {
      const fake = fakeTaskActions();
      const items = resolutionMenuItems(DEADLINE_WARNING, fake, fakeSnippet, NOW);
      const item = getItem(items);
      void item.action?.();
      expect(getAction(fake)).toHaveBeenCalledWith(...expectedArgs);
      expect(item.destructive ?? false).toBe(expectDestructive);
    },
  );
});

describe('ResolutionDropdown', () => {
  function renderDropdown(warning: ScheduleWarning = DEADLINE_WARNING) {
    const fake = fakeTaskActions();
    const result = render(ResolutionDropdown, {
      warning,
      actions: fake,
    });
    return { fake, ...result };
  }

  function menuItems(container: HTMLElement): HTMLElement[] {
    return Array.from(container.querySelectorAll<HTMLElement>('[role="menuitem"]'));
  }

  async function verifyCustomDeadlinePick(opts: {
    warning: ScheduleWarning;
    triggerName: string;
    taskId: string;
    expectedSelectedDate: string | null;
    targetDate: string;
  }): Promise<void> {
    const { warning, triggerName, taskId, expectedSelectedDate, targetDate } = opts;
    const { fake, getByRole, container } = renderDropdown(warning);
    await fireEvent.click(getByRole('button', { name: triggerName }));
    await tick();
    const custom = menuItems(container).find((el) =>
      el.textContent?.trim().startsWith('Custom deadline'),
    );
    expect(container.querySelector('.submenu-panel')).toBeNull();
    await fireEvent.mouseEnter(custom!);
    await tick();
    if (expectedSelectedDate !== null) {
      expect(
        container.querySelector('.calendar-day-btn--selected')?.getAttribute('data-date'),
      ).toBe(expectedSelectedDate);
    } else {
      expect(container.querySelector('.calendar-day-btn--selected')).toBeNull();
    }
    await fireEvent.click(container.querySelector(`[data-date="${targetDate}"]`)!);
    await tick();
    expect(fake.extendDeadline).toHaveBeenCalledWith(taskId, customDeadlineIso(targetDate));
    expect(container.querySelector('[role="menu"]')).toBeNull();
  }

  it('renders a closed trigger labelled with the task title', () => {
    const { getByRole, container } = renderDropdown();

    const trigger = getByRole('button', { name: 'Resolve Alpha task' });
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it('opens the menu on click and closes it on a second click', async () => {
    const { getByRole, container } = renderDropdown();
    const trigger = getByRole('button', { name: 'Resolve Alpha task' });

    await fireEvent.click(trigger);
    await tick();
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    const items = menuItems(container);
    expect(items).toHaveLength(9);
    expect(items[0]?.textContent).toContain('Extend to today (');
    const cancelItem = items.find((el) => el.textContent?.trim() === 'Cancel task');
    expect(cancelItem?.classList.contains('menu-item--destructive')).toBe(true);

    await fireEvent.click(trigger);
    await tick();
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it('clicking an item runs its verb and closes the menu', async () => {
    const { fake, getByRole, container } = renderDropdown();

    await fireEvent.click(getByRole('button', { name: 'Resolve Alpha task' }));
    await tick();
    const doNow = menuItems(container).find((el) => el.textContent?.trim() === 'Do now');
    await fireEvent.click(doNow!);
    await tick();

    expect(fake.doNow).toHaveBeenCalledWith('task-1', expect.any(Date));
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it.each([
    {
      label: 'with existing deadline',
      warning: DEADLINE_WARNING,
      triggerName: 'Resolve Alpha task',
      taskId: 'task-1',
      expectedSelectedDate: isoToLocalDate(DEADLINE_ISO),
      targetDate: `${isoToLocalDate(DEADLINE_ISO).slice(0, 8)}15`,
    },
    {
      label: 'without existing deadline',
      warning: BLOCKING_WARNING,
      triggerName: 'Resolve Beta task',
      taskId: 'task-2',
      expectedSelectedDate: null,
      targetDate: '2026-07-15',
    },
  ])(
    'custom deadline pick $label anchors at end of day',
    async ({ warning, triggerName, taskId, expectedSelectedDate, targetDate }) => {
      vi.useFakeTimers({ toFake: ['Date'] });
      try {
        vi.setSystemTime(new Date('2026-07-15T12:00:00Z'));
        await verifyCustomDeadlinePick({
          warning,
          triggerName,
          taskId,
          expectedSelectedDate,
          targetDate,
        });
      } finally {
        vi.useRealTimers();
      }
    },
  );
});
