// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { tick } from 'svelte';
import {
  CALENDAR_TEST_NOW,
  installCalendarHooks,
  importCalendarView,
  modeButton,
  openCompletionDialog,
  scheduledAgendaItem,
  completedAgendaItem,
  fixedAgendaItem,
  twoScheduledChunks,
  dragEmptySlot,
  parseTimeRange,
  calendarApiFake,
} from './testFixtures';
import { activeShortcuts, resetShortcutsForTest } from '../../shortcuts.svelte';
import { toastState } from '../../stores/toast.svelte';
import { taskState } from '../../stores/tasks.svelte';

installCalendarHooks();

let fake: ReturnType<typeof calendarApiFake>;

beforeEach(() => {
  fake = calendarApiFake();
  toastState.items = [];
});

afterEach(() => {
  toastState.items = [];
});

async function flush(): Promise<void> {
  await tick();
  await Promise.resolve();
  await tick();
}

async function renderCalendarViewBase(deepFlush: boolean) {
  const CalendarView = await importCalendarView();
  const utils = render(CalendarView, {
    props: { apiClient: fake, getNow: () => CALENDAR_TEST_NOW },
  });
  await (deepFlush ? flush() : tick());
  return utils;
}
const renderWithAgenda = () => renderCalendarViewBase(true);
const renderCalendarView = () => renderCalendarViewBase(false);

function mockScheduledWithChunks() {
  fake.getAgenda.mockResolvedValue([scheduledAgendaItem()]);
  fake.listChunksForTask.mockResolvedValue(twoScheduledChunks());
}

describe('CalendarView — chunk click editing', () => {
  it('opens the task edit form when a chunk is clicked', async () => {
    mockScheduledWithChunks();

    const { container, getByText } = await renderWithAgenda();

    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block).toBeTruthy();
    await fireEvent.click(block!);
    await flush();

    expect(fake.getTask).toHaveBeenCalledWith('task-1');
    expect(getByText('Edit Task')).toBeTruthy();
  });
});

describe('CalendarView — chunk completion dialog', () => {
  it('opens the completion dialog with chunk selected by default', async () => {
    mockScheduledWithChunks();

    const { container, getByText } = await openCompletionDialog(fake);

    expect(getByText('Complete Work')).toBeTruthy();
    const chunkRadio = container.querySelector(
      'input[name="completion-target"][value="chunk"]',
    ) as HTMLInputElement | null;
    expect(chunkRadio?.checked).toBe(true);
  });

  it('completes only the clicked chunk when confirmed with the default selection', async () => {
    mockScheduledWithChunks();

    const { getByText } = await openCompletionDialog(fake);

    await fireEvent.click(getByText('Complete'));
    await Promise.resolve();
    await tick();

    expect(fake.completeChunk).toHaveBeenCalledWith('chunk-1');
    await waitFor(() => {
      expect(
        toastState.items.some((m) => m.level === 'success' && m.text === 'Chunk completed'),
      ).toBe(true);
    });
  });

  it('can switch to whole-task completion and complete all scheduled chunks', async () => {
    mockScheduledWithChunks();

    const { container, getByText } = await openCompletionDialog(fake);

    const taskRadio = container.querySelector(
      'input[name="completion-target"][value="task"]',
    ) as HTMLInputElement | null;
    expect(taskRadio).toBeTruthy();
    await fireEvent.click(taskRadio!);
    await tick();
    expect(taskRadio?.checked).toBe(true);

    await fireEvent.click(getByText('Complete'));
    await Promise.resolve();
    await Promise.resolve();
    await tick();

    expect(fake.completeTask).toHaveBeenCalledWith('task-1');
    await waitFor(() => {
      expect(
        toastState.items.some((m) => m.level === 'success' && m.text === 'Task completed'),
      ).toBe(true);
    });
  });

  it('skips the dialog and completes only the clicked chunk when it is the last scheduled chunk', async () => {
    fake.getAgenda.mockResolvedValue([scheduledAgendaItem()]);
    fake.listChunksForTask.mockResolvedValue([
      {
        ...scheduledAgendaItem().chunk,
      },
    ]);

    const { container, queryByText } = await renderWithAgenda();

    const toggle = container.querySelector('.complete-toggle') as HTMLElement | null;
    expect(toggle).toBeTruthy();
    await fireEvent.click(toggle!);

    await waitFor(() => {
      expect(fake.completeChunk).toHaveBeenCalledWith('chunk-1');
    });
    expect(fake.completeTask).not.toHaveBeenCalled();
    expect(queryByText('Complete Work')).toBeNull();
    expect(
      toastState.items.some((m) => m.level === 'success' && m.text === 'Chunk completed'),
    ).toBe(true);
  });

  it('shows a checked toggle for completed chunks and reopens only the clicked chunk', async () => {
    fake.getAgenda.mockResolvedValue([completedAgendaItem()]);

    const { container, queryByText } = await renderWithAgenda();

    const toggle = container.querySelector('.complete-toggle') as HTMLElement | null;
    expect(toggle).toBeTruthy();
    expect(toggle?.getAttribute('aria-checked')).toBe('true');

    await fireEvent.click(toggle!);
    await waitFor(() => {
      expect(fake.reopenChunk).toHaveBeenCalledWith('chunk-1');
    });
    expect(queryByText('Complete Work')).toBeNull();
    expect(toastState.items.some((m) => m.level === 'success' && m.text === 'Chunk reopened')).toBe(
      true,
    );
  });
});

describe('CalendarView — chunk context menu', () => {
  async function renderWithMenu(item: ReturnType<typeof scheduledAgendaItem>) {
    fake.getAgenda.mockResolvedValue([item]);
    const { container } = await renderWithAgenda();

    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block).toBeTruthy();
    await fireEvent.contextMenu(block!, { clientX: 100, clientY: 200 });
    await tick();
    expect(container.querySelector('[role="menu"]')).toBeTruthy();
    return container;
  }

  function menuButton(container: HTMLElement, label: string): HTMLElement | null {
    return (
      Array.from(container.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
        (b) => b.textContent?.trim() === label,
      ) ?? null
    );
  }

  it.each([
    {
      label: 'unlocks a fixed chunk',
      item: fixedAgendaItem(),
      menuLabel: 'Unlock chunk',
      expectCall: () => expect(fake.unlockChunk).toHaveBeenCalledWith('chunk-1'),
    },
    {
      label: 'locks an auto chunk',
      item: scheduledAgendaItem(),
      menuLabel: 'Lock chunk',
      expectCall: () => expect(fake.lockChunk).toHaveBeenCalledWith('chunk-1'),
    },
  ])('$label via the context menu', async ({ item, menuLabel, expectCall }) => {
    const container = await renderWithMenu(item);
    const btn = menuButton(container, menuLabel);
    expect(btn).toBeTruthy();
    await fireEvent.click(btn!);
    await waitFor(() => {
      expectCall();
    });
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it.each([
    {
      label: 'cancels on confirm',
      clickSelector: '.btn-danger' as const,
      expectCall: () => expect(fake.cancelTask).toHaveBeenCalledWith('task-1'),
    },
    {
      label: 'leaves task untouched on decline',
      clickSelector: '.btn-cancel' as const,
      expectCall: () => expect(fake.cancelTask).not.toHaveBeenCalled(),
    },
  ])('cancel task dialog: $label', async ({ clickSelector, expectCall }) => {
    const container = await renderWithMenu(scheduledAgendaItem());

    await fireEvent.click(menuButton(container, 'Cancel task')!);
    await tick();

    expect(fake.cancelTask).not.toHaveBeenCalled();
    await fireEvent.click(container.querySelector(clickSelector)!);
    await tick();

    await waitFor(() => {
      expectCall();
    });
    expect(container.querySelector('.btn-danger')).toBeNull();
  });

  it('Edit template routes to the tasks view with a template edit request', async () => {
    const container = await renderWithMenu(scheduledAgendaItem('tpl-1'));

    await fireEvent.click(menuButton(container, 'Edit template')!);
    await tick();

    expect(taskState.templateEditRequestId).toBe('tpl-1');
    expect(window.location.hash).toBe('#/tasks');
  });
});

describe('CalendarView — chunk lock button', () => {
  async function clickLockBtn(container: HTMLElement): Promise<void> {
    const btn = container.querySelector('.lock-btn') as HTMLElement | null;
    expect(btn).toBeTruthy();
    await fireEvent.click(btn!);
  }

  it.each([
    { item: fixedAgendaItem(), expectedCall: 'unlockChunk' as const },
    { item: scheduledAgendaItem(), expectedCall: 'lockChunk' as const },
  ])('lock button calls $expectedCall', async ({ item, expectedCall }) => {
    fake.getAgenda.mockResolvedValue([item]);
    const { container } = await renderWithAgenda();
    await clickLockBtn(container);
    await waitFor(() => {
      expect(fake[expectedCall]).toHaveBeenCalledWith('chunk-1');
    });
  });
});

describe('CalendarView — empty-slot create flow', () => {
  it('opens the create form from an empty slot click and creates a fixed chunk on submit', async () => {
    const { container, getByRole, getByPlaceholderText } = await renderCalendarView();

    await fireEvent.click(modeButton(getByRole, 'Day'));
    await tick();

    await dragEmptySlot(container);

    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    titleInput.value = 'New fixed task';
    await fireEvent.input(titleInput);

    await fireEvent.submit(container.querySelector('.task-form')!);

    await waitFor(() => {
      expect(fake.createTask).toHaveBeenCalledTimes(1);
      expect(fake.createFixedChunk).toHaveBeenCalledTimes(1);
    });

    const createInput = fake.createTask.mock.calls[0]?.[0];
    expect(createInput?.duration_minutes).toBe(30);

    const [taskId, start, end] = fake.createFixedChunk.mock.calls[0] as [string, string, string];
    expect(taskId).toBe('task-new');
    expect(parseTimeRange(start, end)).toEqual({
      hours: 9,
      minutes: 0,
      durationMs: 30 * 60 * 1000,
    });
  });
});

describe('CalendarView — keyboard shortcuts', () => {
  afterEach(() => {
    resetShortcutsForTest();
  });

  function binding(key: string) {
    const b = activeShortcuts().find((s) => s.key === key && s.group === 'Calendar');
    if (!b) throw new Error(`No Calendar binding for key "${key}"`);
    return b;
  }

  it('t jumps the header back to today after navigating forward', async () => {
    const { container } = await renderCalendarView();

    const headerBefore = container.querySelector('.date-header')!.textContent ?? '';

    binding('ArrowRight').handler();
    await tick();
    const headerAfter = container.querySelector('.date-header')!.textContent ?? '';
    expect(headerAfter).not.toBe(headerBefore);

    binding('t').handler();
    await tick();
    expect(container.querySelector('.date-header')!.textContent).toBe(headerBefore);
  });

  it.each(['ArrowLeft', 'ArrowRight'])('%s changes the header label', async (key) => {
    const { container } = await renderCalendarView();

    const headerBefore = container.querySelector('.date-header')!.textContent ?? '';
    binding(key).handler();
    await tick();
    expect(container.querySelector('.date-header')!.textContent).not.toBe(headerBefore);
  });

  it('d switches to day mode (Day button becomes active)', async () => {
    const { getByRole } = await renderCalendarView();

    binding('d').handler();
    await tick();

    expect(modeButton(getByRole, 'Day').getAttribute('aria-pressed')).toBe('true');
  });

  it('w switches back to week mode', async () => {
    const { getByRole } = await renderCalendarView();

    binding('d').handler();
    await tick();
    binding('w').handler();
    await tick();

    expect(modeButton(getByRole, 'Week').getAttribute('aria-pressed')).toBe('true');
  });

  it('r triggers reschedule; a second r while in-flight is ignored', async () => {
    fake.triggerReschedule.mockReturnValue(new Promise(() => {}));

    await renderCalendarView();

    binding('r').handler();
    binding('r').handler();

    expect(fake.triggerReschedule).toHaveBeenCalledTimes(1);
  });

  it('n opens the task form without a fixed-chunk slot — submitting calls createTask but not createFixedChunk', async () => {
    const { getByText, getByPlaceholderText, container } = await renderCalendarView();

    binding('n').handler();
    await tick();

    expect(getByText('Create Task')).toBeTruthy();

    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    titleInput.value = 'Shortcut task';
    await fireEvent.input(titleInput);

    await fireEvent.submit(container.querySelector('.task-form')!);

    await waitFor(() => {
      expect(fake.createTask).toHaveBeenCalledTimes(1);
    });
    expect(fake.createFixedChunk).not.toHaveBeenCalled();
  });

  it('unmounting CalendarView unregisters its Calendar bindings', async () => {
    await renderCalendarView();

    expect(activeShortcuts().some((b) => b.key === 't' && b.group === 'Calendar')).toBe(true);

    cleanup();

    expect(activeShortcuts().some((b) => b.key === 't' && b.group === 'Calendar')).toBe(false);
  });
});
