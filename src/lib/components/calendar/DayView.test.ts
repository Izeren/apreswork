// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { AgendaItem, Chunk, ExternalEvent, ScheduleWindow } from '../../types';
import type { CalendarViewCommonProps } from './calendarViewShared';
import {
  externalEventFixture,
  installViewTestHooks,
  localDate,
  dragEmptySlot,
  chunkStatusCases,
  hasTwoColumnOverlapLayout,
  isOpenAfterClick,
  isExternalEventClickableAndOpens,
  soleBlockHasClass,
  parseTimeRange,
} from './testFixtures';

installViewTestHooks();

type DayProps = CalendarViewCommonProps & { date: Date };

const defaultChunk: Chunk = {
  id: 'chunk-1',
  task_id: 'task-1',
  start_time: '2026-03-28T09:00:00.000Z',
  end_time: '2026-03-28T10:00:00.000Z',
  status: 'scheduled',
  is_fixed: false,
  logged_minutes: null,
  completed_at: null,
  google_event_id: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

/** Build an agenda item; chunk overrides merge onto the default chunk. */
function baseItem(
  overrides: Partial<Omit<AgendaItem, 'chunk'>> & { chunk?: Partial<Chunk> } = {},
): AgendaItem {
  const { chunk, ...rest } = overrides;
  return {
    chunk: { ...defaultChunk, ...chunk },
    task_title: 'Test Task',
    task_priority: 'Medium',
    task_labels: [],
    task_recurring_template_id: null,
    task_deadline: null,
    ...rest,
  };
}

function localISO(year: number, month: number, day: number, hour: number, minute = 0): string {
  return new Date(year, month - 1, day, hour, minute, 0).toISOString();
}

function timedExternal(overrides: Partial<ExternalEvent> = {}): ExternalEvent {
  return externalEventFixture({
    start_time: localISO(2026, 3, 28, 12),
    end_time: localISO(2026, 3, 28, 13),
    ...overrides,
  });
}

async function importDayView() {
  const mod = await import('./DayView.svelte');
  return mod.default;
}

const FROZEN_TODAY = localDate(2026, 3, 28);

async function renderDay(overrides: Partial<DayProps> = {}) {
  const DayView = await importDayView();
  return render(DayView, {
    date: localDate(2026, 3, 28),
    now: FROZEN_TODAY,
    items: [] as AgendaItem[],
    ...overrides,
  });
}

/** Render DayView at a specific `now` time, then flush the past-wash effect. */
async function renderDayAt(now: Date, overrides: Partial<DayProps> = {}) {
  const result = await renderDay({ now, ...overrides });
  await tick();
  return result;
}

describe('DayView — empty state', () => {
  it.each([
    { label: 'empty items array', items: [] as AgendaItem[] },
    {
      label: 'items for a different day',
      items: [
        baseItem({
          chunk: { start_time: '2026-03-29T09:00:00.000Z', end_time: '2026-03-29T10:00:00.000Z' },
        }),
      ],
    },
  ])('shows "No chunks scheduled" ($label)', async ({ items }) => {
    const { getByText } = await renderDay({ items });
    expect(getByText('No chunks scheduled')).toBeTruthy();
  });
});

describe('DayView — chunk rendering', () => {
  it.each([
    {
      label: 'single chunk on matching day',
      items: [baseItem({ chunk: { start_time: '2026-03-28T12:00:00.000Z' } })],
      expectedCount: 1,
    },
    {
      label: 'multiple chunks on same day',
      items: [
        baseItem({
          chunk: { id: 'c1', start_time: '2026-03-28T09:00:00.000Z' },
          task_title: 'Task A',
        }),
        baseItem({
          chunk: { id: 'c2', start_time: '2026-03-28T14:00:00.000Z' },
          task_title: 'Task B',
        }),
      ],
      expectedCount: 2,
    },
    {
      label: 'chunk from different day excluded',
      items: [
        baseItem({
          chunk: { id: 'c1', start_time: '2026-03-28T12:00:00.000Z' },
          task_title: 'Task A',
        }),
        baseItem({
          chunk: { id: 'c2', start_time: '2026-03-29T12:00:00.000Z' },
          task_title: 'Task B',
        }),
      ],
      expectedCount: 1,
    },
  ])('renders chunk-block count=$expectedCount ($label)', async ({ items, expectedCount }) => {
    const { container } = await renderDay({ items });
    expect(container.querySelectorAll('.chunk-block')).toHaveLength(expectedCount);
  });

  it('does not show empty state when items are present for the day', async () => {
    const item = baseItem({ chunk: { start_time: '2026-03-28T12:00:00.000Z' } });
    const { queryByText } = await renderDay({ items: [item] });
    expect(queryByText('No chunks scheduled')).toBeNull();
  });

  it('displays task title in the chunk block', async () => {
    const item = baseItem({
      chunk: { start_time: '2026-03-28T12:00:00.000Z', end_time: '2026-03-28T13:00:00.000Z' },
      task_title: 'My Important Task',
    });
    const { getByText } = await renderDay({ items: [item] });
    expect(getByText('My Important Task')).toBeTruthy();
  });

  it('handles empty task_title gracefully', async () => {
    const item = baseItem({ chunk: { start_time: '2026-03-28T12:00:00.000Z' }, task_title: '' });
    const { container } = await renderDay({ items: [item] });
    expect(container.querySelectorAll('.chunk-block')).toHaveLength(1);
  });

  it('passes chunk clicks through to onchunkopen', async () => {
    const onchunkopen = vi.fn();
    const item = baseItem({
      chunk: { start_time: '2026-03-28T12:00:00.000Z', end_time: '2026-03-28T13:00:00.000Z' },
    });
    const opened = await isOpenAfterClick(
      () => renderDay({ items: [item], onchunkopen }),
      onchunkopen,
    );
    expect(opened).toBe(true);
  });

  it('renders overlapping chunks side by side and keeps the titles readable', async () => {
    const items = [
      baseItem({
        chunk: {
          id: 'c1',
          start_time: localISO(2026, 3, 28, 9, 0),
          end_time: localISO(2026, 3, 28, 10, 0),
        },
        task_title: 'Deep Work Block',
      }),
      baseItem({
        chunk: {
          id: 'c2',
          start_time: localISO(2026, 3, 28, 9, 30),
          end_time: localISO(2026, 3, 28, 10, 30),
        },
        task_title: 'Review Session',
      }),
    ];

    const { container, getByText } = await renderDay({ items });

    const blocks = Array.from(container.querySelectorAll('.chunk-block')) as HTMLElement[];
    expect(hasTwoColumnOverlapLayout(blocks)).toBe(true);
    expect(getByText('Deep Work Block')).toBeTruthy();
    expect(getByText('Review Session')).toBeTruthy();
    expect(container.querySelectorAll('.chunk-time')).toHaveLength(0);
    expect(container.querySelectorAll('.chunk-duration')).toHaveLength(0);
  });

  it('clicking an empty slot creates a default 30-minute selection', async () => {
    const oncreatechunk = vi.fn();
    const { container } = await renderDay({ items: [], oncreatechunk });

    await dragEmptySlot(container);

    expect(oncreatechunk).toHaveBeenCalledTimes(1);
    const [start, end] = oncreatechunk.mock.calls[0] as [string, string];
    expect(parseTimeRange(start, end)).toEqual({
      hours: 9,
      minutes: 0,
      durationMs: 30 * 60 * 1000,
    });
  });
});

describe('DayView — chunk status classes', () => {
  it.each(chunkStatusCases)(
    'chunk with status "$status" has class "$expectedClass"',
    async ({ status, expectedClass }) => {
      const item = baseItem({ chunk: { status, start_time: '2026-03-28T12:00:00.000Z' } });
      const { container } = await renderDay({ items: [item] });
      expect(soleBlockHasClass(container, expectedClass)).toBe(true);
    },
  );

  it.each([
    { isFixed: true, expected: true },
    { isFixed: false, expected: false },
  ])('is_fixed=$isFixed → is-fixed class: $expected', async ({ isFixed, expected }) => {
    const item = baseItem({ chunk: { is_fixed: isFixed, start_time: '2026-03-28T12:00:00.000Z' } });
    const { container } = await renderDay({ items: [item] });
    expect(soleBlockHasClass(container, 'is-fixed')).toBe(expected);
  });
});

describe('DayView — day header', () => {
  it('renders the day-header element', async () => {
    const { container } = await renderDay();
    expect(container.querySelector('.day-header')).toBeTruthy();
  });

  it('day header text includes the day number', async () => {
    const { container } = await renderDay();
    const header = container.querySelector('.day-label');
    expect(header!.textContent).toMatch(/28/);
  });
});

describe('DayView — accessibility', () => {
  it('each chunk block has an aria-label with task title and time', async () => {
    const item = baseItem({
      chunk: { start_time: '2026-03-28T12:00:00.000Z' },
      task_title: 'Workout',
    });
    const { container } = await renderDay({ items: [item] });
    const block = container.querySelector('.chunk-block');
    expect(block).toBeTruthy();
    const label = block!.getAttribute('aria-label') ?? '';
    expect(label).toContain('Workout');
  });
});

describe('DayView — past-wash', () => {
  const frozenNow = new Date(2026, 2, 25, 10, 30, 0);

  it.each([
    { label: 'past date', date: localDate(2026, 3, 24), expectHeight: '1440px' },
    { label: 'today', date: localDate(2026, 3, 25), expectHeight: '630px' },
    { label: 'future date', date: localDate(2026, 3, 26), expectHeight: null },
  ])('$label shows past-wash correctly', async ({ date, expectHeight }) => {
    const { container } = await renderDayAt(frozenNow, { date });
    const wash = container.querySelector('.past-wash') as HTMLElement | null;
    expect(wash?.style.height ?? null).toBe(expectHeight);
  });

  it('past-wash is aria-hidden', async () => {
    const { container } = await renderDayAt(frozenNow, { date: localDate(2026, 3, 24) });
    const wash = container.querySelector('.past-wash');
    expect(wash?.getAttribute('aria-hidden')).toBe('true');
  });
});

function makeSatWindow(id: string): ScheduleWindow {
  return {
    id,
    schedule_id: 'sched-1',
    day_of_week: 'Sat',
    start_time: '09:00:00',
    end_time: '11:00:00',
  };
}

describe('DayView — schedule window overlay', () => {
  it.each([
    { label: 'matching window', windows: [makeSatWindow('w1')], expected: 1 },
    {
      label: 'non-matching window',
      windows: [{ ...makeSatWindow('w1'), day_of_week: 'Mon' as ScheduleWindow['day_of_week'] }],
      expected: 0,
    },
    { label: 'omitted windows', windows: undefined, expected: 0 },
    {
      label: 'multiple matching windows',
      windows: [makeSatWindow('w1'), makeSatWindow('w2')],
      expected: 2,
    },
  ])('$label — schedule-window-band count', async ({ windows, expected }) => {
    const { container } = await renderDay({ windows });
    expect(container.querySelectorAll('.schedule-window-band')).toHaveLength(expected);
  });
});

describe('DayView — external events', () => {
  it.each([
    { label: 'matching date', ext: timedExternal(), expectedCount: 1 },
    {
      label: 'different date',
      ext: externalEventFixture({
        start_time: localISO(2026, 3, 29, 12),
        end_time: localISO(2026, 3, 29, 13),
      }),
      expectedCount: 0,
    },
  ])('renders external-event count=$expectedCount ($label)', async ({ ext, expectedCount }) => {
    const { container } = await renderDay({ externalEvents: [ext] });
    expect(container.querySelectorAll('.external-event')).toHaveLength(expectedCount);
  });

  it('empty-state text is still shown when only external events are present (no chunks)', async () => {
    const { getByText } = await renderDay({ externalEvents: [timedExternal()] });
    expect(getByText('No chunks scheduled')).toBeTruthy();
  });
});

function allDayExternal(overrides: Partial<ExternalEvent> = {}): ExternalEvent {
  return externalEventFixture({
    all_day: true,
    title: 'Vacation',
    start_time: localISO(2026, 3, 28, 0),
    end_time: localISO(2026, 3, 29, 0),
    ...overrides,
  });
}

describe('DayView — all-day lane', () => {
  it('renders an all-day event in the all-day lane, not the timed grid', async () => {
    const { container } = await renderDay({ externalEvents: [allDayExternal()] });

    const lane = container.querySelector('.all-day-lane');
    expect(lane).toBeTruthy();
    expect(lane!.querySelectorAll('.external-event--allday')).toHaveLength(1);

    // The timed grid must not carry the all-day block (would render 24h tall).
    const grid = container.querySelector('[aria-label="Time grid"]') as HTMLElement;
    expect(grid.querySelectorAll('.external-event')).toHaveLength(0);
  });

  it('does not render an all-day lane when there are no all-day events', async () => {
    const { container } = await renderDay({ externalEvents: [timedExternal()] });
    expect(container.querySelector('.all-day-lane')).toBeNull();
  });

  it('keeps a timed external in the grid alongside an all-day one in the lane', async () => {
    const { container } = await renderDay({
      externalEvents: [
        timedExternal({ event_id: 'timed-1' }),
        allDayExternal({ event_id: 'allday-1' }),
      ],
    });
    expect(
      container.querySelector('.all-day-lane')!.querySelectorAll('.external-event'),
    ).toHaveLength(1);
    const grid = container.querySelector('[aria-label="Time grid"]') as HTMLElement;
    expect(grid.querySelectorAll('.external-event')).toHaveLength(1);
  });
});

describe('DayView — external event editing', () => {
  it.each([
    {
      calendarId: 'primary',
      editableCalendarId: 'primary' as string | null,
      expectedRole: 'button',
    },
    {
      calendarId: 'other-cal',
      editableCalendarId: 'primary' as string | null,
      expectedRole: 'img',
    },
    { calendarId: 'primary', editableCalendarId: null, expectedRole: 'img' },
  ])(
    'timed external role=$expectedRole when calendarId=$calendarId editableCalendarId=$editableCalendarId',
    async ({ calendarId, editableCalendarId, expectedRole }) => {
      const ext = timedExternal({ calendar_id: calendarId });
      const { container } = await renderDay({
        externalEvents: [ext],
        oneventopen: vi.fn(),
        editableCalendarId,
      });
      expect(container.querySelector('.external-event')!.getAttribute('role')).toBe(expectedRole);
    },
  );

  it('primary-calendar timed external forwards oneventopen on click', async () => {
    const oneventopen = vi.fn();
    const ext = timedExternal({ calendar_id: 'primary' });
    const { container } = await renderDay({
      externalEvents: [ext],
      oneventopen,
      editableCalendarId: 'primary',
    });
    const clickable = await isExternalEventClickableAndOpens(container, oneventopen, 'primary');
    expect(clickable).toBe(true);
  });

  it('makes a primary-calendar all-day external clickable in the lane', async () => {
    const oneventopen = vi.fn();
    const { container } = await renderDay({
      externalEvents: [allDayExternal({ calendar_id: 'primary' })],
      oneventopen,
      editableCalendarId: 'primary',
    });
    const laneBlock = container.querySelector('.all-day-lane .external-event') as HTMLElement;
    expect(laneBlock.getAttribute('role')).toBe('button');
    await fireEvent.click(laneBlock);
    expect(oneventopen).toHaveBeenCalledTimes(1);
  });
});
