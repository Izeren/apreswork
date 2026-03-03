// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import { tick, type ComponentProps } from 'svelte';
import type { AgendaItem, ChunkStatus, ExternalEvent, ScheduleWindow } from '../../types';
import { dragState } from './dragState.svelte';
import { WEEK_FLIP_DWELL_MS } from './weekEdgeFlip';
import {
  externalEventFixture,
  installViewTestHooks,
  localDate,
  chunkStatusCases,
  hasTwoColumnOverlapLayout,
  isOpenAfterClick,
  isExternalEventClickableAndOpens,
  soleBlockHasClass,
} from './testFixtures';

installViewTestHooks();

const ONE_HOUR_MS = 60 * 60 * 1000;

/** ISO string at noon local time — avoids DST edge cases. */
function noonLocalISO(year: number, month: number, day: number): string {
  return new Date(year, month - 1, day, 12, 0, 0).toISOString();
}

function baseChunk(overrides: {
  id?: string;
  start_time?: string;
  end_time?: string;
  status?: ChunkStatus;
  is_fixed?: boolean;
}) {
  const startTime = overrides.start_time ?? noonLocalISO(2026, 3, 23);
  return {
    id: overrides.id ?? 'chunk-1',
    task_id: 'task-1',
    start_time: startTime,
    end_time:
      overrides.end_time ?? new Date(new Date(startTime).getTime() + ONE_HOUR_MS).toISOString(),
    status: (overrides.status ?? 'scheduled') as ChunkStatus,
    is_fixed: overrides.is_fixed ?? false,
    logged_minutes: null,
    completed_at: null,
    google_event_id: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  };
}

function baseItem(overrides: {
  id?: string;
  start_time?: string;
  end_time?: string;
  task_title?: string;
  status?: ChunkStatus;
  is_fixed?: boolean;
}): AgendaItem {
  return {
    chunk: baseChunk({
      id: overrides.id,
      start_time: overrides.start_time,
      end_time: overrides.end_time,
      status: overrides.status,
      is_fixed: overrides.is_fixed,
    }),
    task_title: overrides.task_title ?? 'Test Task',
    task_priority: 'Medium',
    task_labels: [],
    task_recurring_template_id: null,
    task_deadline: null,
  };
}

/** Mon–Sun, 2026-03-23 → 2026-03-29 */
const WEEK_DAYS = Array.from({ length: 7 }, (_, i) => localDate(2026, 3, 23 + i));

/** Wednesday 2026-03-25, 10:30 local */
const FROZEN_TODAY = new Date(2026, 2, 25, 10, 30, 0);

async function importWeekView() {
  const mod = await import('./WeekView.svelte');
  return mod.default;
}

type WeekProps = ComponentProps<Awaited<ReturnType<typeof importWeekView>>>;

/** Render WeekView with the standard week + empty items; `props` overrides win. */
async function renderWeek(props: Partial<WeekProps> = {}) {
  const WeekView = await importWeekView();
  const utils = render(WeekView, { days: WEEK_DAYS, items: [], now: FROZEN_TODAY, ...props });
  await tick();
  return utils;
}

async function renderWeekAt(now: Date, props: Partial<WeekProps> = {}) {
  return renderWeek({ now, ...props });
}

/** 7-day window centred so today sits at index 3 */
function daysAroundToday(today: Date): Date[] {
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(today);
    d.setDate(today.getDate() + i - 3);
    return d;
  });
}

/** Saturday 2026-03-28 = index 5 in WEEK_DAYS, noon–1pm local */
function satExternal(overrides: Partial<ExternalEvent> = {}): ExternalEvent {
  return externalEventFixture({
    start_time: noonLocalISO(2026, 3, 28),
    end_time: new Date(2026, 2, 28, 13, 0, 0).toISOString(),
    ...overrides,
  });
}

describe('WeekView — column headers', () => {
  it('renders 7 column headers', async () => {
    const { container } = await renderWeek();
    const headers = container.querySelectorAll('.day-header');
    expect(headers).toHaveLength(7);
  });

  const headerCases = [
    { index: 0, expectedDay: 23, label: 'Monday (index 0)' },
    { index: 6, expectedDay: 29, label: 'Sunday (index 6)' },
  ];

  it.each(headerCases)(
    'column header at index $index contains day number $expectedDay ($label)',
    async ({ index, expectedDay }) => {
      const { container } = await renderWeek();
      const headers = container.querySelectorAll('.day-header');
      expect(headers[index].textContent).toMatch(String(expectedDay));
    },
  );

  it('all 7 headers contain different day names from Mon through Sun', async () => {
    const { container } = await renderWeek();
    const headers = Array.from(container.querySelectorAll('.day-header'));
    const texts = headers.map((h) => h.textContent?.trim() ?? '');
    const unique = new Set(texts);
    expect(unique.size).toBe(7);
  });
});

describe('WeekView — today highlighting', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  // JS: 0=Sun, 1=Mon … 6=Sat; shift so Mon=0 to get ISO week start offset.
  const isoOffset = (FROZEN_TODAY.getDay() + 6) % 7;
  const frozenWeekDays = Array.from({ length: 7 }, (_, i) => {
    const d = new Date(FROZEN_TODAY);
    d.setDate(FROZEN_TODAY.getDate() - isoOffset + i);
    return d;
  });

  it.each([
    { label: 'ISO-aligned current week', days: frozenWeekDays, expectedCount: 1 },
    { label: 'daysAroundToday result', days: daysAroundToday(FROZEN_TODAY), expectedCount: 1 },
    {
      label: 'past week excludes today',
      days: Array.from({ length: 7 }, (_, i) => localDate(2020, 1, 1 + i)),
      expectedCount: 0,
    },
  ])('$label — $expectedCount header(s) highlighted', async ({ days, expectedCount }) => {
    const { container } = await renderWeekAt(FROZEN_TODAY, { days });
    expect(container.querySelectorAll('.day-header--today')).toHaveLength(expectedCount);
  });
});

describe('WeekView — TimeGrid', () => {
  it('renders the time-grid element', async () => {
    const { container } = await renderWeek();
    expect(container.querySelector('.time-grid')).toBeTruthy();
  });

  it("shows the current time indicator only in today's column when today is visible in the week but is not the first day", async () => {
    const days = daysAroundToday(FROZEN_TODAY);
    const { container } = await renderWeekAt(FROZEN_TODAY, { days });

    const indicators = container.querySelectorAll('.column-time-indicator');
    expect(indicators).toHaveLength(1);

    const columns = container.querySelectorAll('.day-column');
    expect(columns[3]?.querySelector('.column-time-indicator')).toBeTruthy();
    expect(container.querySelector('.time-indicator')).toBeNull();
  });
});

describe('WeekView — per-column filtering', () => {
  it.each([
    {
      label: 'single Monday item',
      items: [
        baseItem({ id: 'c1', start_time: noonLocalISO(2026, 3, 23), task_title: 'Monday Task' }),
      ],
      titlesByColumn: { 0: ['Monday Task'] } as Record<number, string[]>,
    },
    {
      label: 'single Sunday item',
      items: [
        baseItem({ id: 'c1', start_time: noonLocalISO(2026, 3, 29), task_title: 'Sunday Task' }),
      ],
      titlesByColumn: { 6: ['Sunday Task'] } as Record<number, string[]>,
    },
    {
      label: 'multi-day items',
      items: [
        baseItem({ id: 'c1', start_time: noonLocalISO(2026, 3, 23), task_title: 'Task Mon' }),
        baseItem({ id: 'c2', start_time: noonLocalISO(2026, 3, 25), task_title: 'Task Wed' }),
        baseItem({ id: 'c3', start_time: noonLocalISO(2026, 3, 29), task_title: 'Task Sun' }),
      ],
      titlesByColumn: { 0: ['Task Mon'], 2: ['Task Wed'], 6: ['Task Sun'] } as Record<
        number,
        string[]
      >,
    },
    {
      label: 'multiple items same day',
      items: [
        baseItem({ id: 'c1', start_time: noonLocalISO(2026, 3, 24), task_title: 'Tue Task 1' }),
        baseItem({
          id: 'c2',
          start_time: new Date(2026, 2, 24, 15, 0, 0).toISOString(),
          task_title: 'Tue Task 2',
        }),
      ],
      titlesByColumn: { 1: ['Tue Task 1', 'Tue Task 2'] } as Record<number, string[]>,
    },
  ])('per-column filtering — $label', async ({ items, titlesByColumn }) => {
    const { container } = await renderWeek({ items });
    const columns = container.querySelectorAll('.day-column');
    const allTitles = Object.values(titlesByColumn).flat();
    for (let i = 0; i < 7; i++) {
      const expected = titlesByColumn[i] ?? [];
      const notExpected = allTitles.filter((t) => !expected.includes(t));
      for (const text of expected) expect(columns[i].textContent).toContain(text);
      for (const text of notExpected) expect(columns[i].textContent).not.toContain(text);
    }
  });

  it('passes chunk clicks through to onchunkopen', async () => {
    const onchunkopen = vi.fn();
    const mondayItem = baseItem({
      id: 'mon-chunk',
      start_time: noonLocalISO(2026, 3, 23),
      task_title: 'Monday Task',
    });
    const opened = await isOpenAfterClick(
      () => renderWeek({ items: [mondayItem], onchunkopen }),
      onchunkopen,
    );
    expect(opened).toBe(true);
  });

  it('renders overlapping chunks side by side within a day column', async () => {
    const items: AgendaItem[] = [
      baseItem({
        id: 'c1',
        start_time: new Date(2026, 2, 23, 9, 0, 0).toISOString(),
        end_time: new Date(2026, 2, 23, 10, 0, 0).toISOString(),
        task_title: 'Morning Focus',
      }),
      baseItem({
        id: 'c2',
        start_time: new Date(2026, 2, 23, 9, 30, 0).toISOString(),
        end_time: new Date(2026, 2, 23, 10, 30, 0).toISOString(),
        task_title: 'Standup Follow-up',
      }),
    ];

    const { container } = await renderWeek({ items });

    const mondayColumn = container.querySelectorAll('.day-column')[0] as HTMLElement;
    const blocks = Array.from(mondayColumn.querySelectorAll('.chunk-block')) as HTMLElement[];
    expect(blocks[0]?.dataset.density).toBe('compact');
    expect(hasTwoColumnOverlapLayout(blocks)).toBe(true);
    expect(mondayColumn.querySelectorAll('.chunk-time')).toHaveLength(0);
    expect(mondayColumn.querySelectorAll('.chunk-duration')).toHaveLength(0);
  });
});

describe('WeekView — empty-slot creation', () => {
  it('dragging on an empty column creates a selection for that day', async () => {
    const oncreatechunk = vi.fn();
    const { container } = await renderWeek({ oncreatechunk });

    const hitAreas = container.querySelectorAll('.create-hit-area');
    const wednesdayHitArea = hitAreas[2] as HTMLElement | undefined;
    expect(wednesdayHitArea).toBeTruthy();
    Object.assign(wednesdayHitArea!, {
      setPointerCapture: vi.fn(),
      releasePointerCapture: vi.fn(),
    });

    await fireEvent.pointerDown(wednesdayHitArea!, { button: 0, clientY: 540, pointerId: 1 });
    await fireEvent.pointerMove(wednesdayHitArea!, { clientY: 600, pointerId: 1 });
    await fireEvent.pointerUp(wednesdayHitArea!, { button: 0, clientY: 600, pointerId: 1 });

    expect(oncreatechunk).toHaveBeenCalledTimes(1);
    const [start, end] = oncreatechunk.mock.calls[0] as [string, string];
    const startDate = new Date(start);
    const endDate = new Date(end);
    expect(startDate.getDate()).toBe(25);
    expect(startDate.getHours()).toBe(9);
    expect(endDate.getTime() - startDate.getTime()).toBe(ONE_HOUR_MS);
  });
});

describe('WeekView — empty state', () => {
  it('all columns show empty-state indicator when items array is empty', async () => {
    const { container } = await renderWeek();
    const emptyStates = container.querySelectorAll('.empty-state');
    expect(emptyStates).toHaveLength(7);
  });

  it('empty column shows empty-state, non-empty column shows chunk blocks', async () => {
    const items = [
      baseItem({ id: 'c1', start_time: noonLocalISO(2026, 3, 23), task_title: 'Mon Task' }),
    ];
    const { container } = await renderWeek({ items });
    const emptyStates = container.querySelectorAll('.empty-state');
    const chunkBlocks = container.querySelectorAll('.chunk-block');
    expect(emptyStates).toHaveLength(6);
    expect(chunkBlocks).toHaveLength(1);
  });
});

describe('WeekView — chunk status classes', () => {
  it.each(chunkStatusCases)(
    'chunk with status "$status" has class "$expectedClass"',
    async ({ status, expectedClass }) => {
      const item = baseItem({
        id: 'c1',
        start_time: noonLocalISO(2026, 3, 23),
        status,
      });
      const { container } = await renderWeek({ items: [item] });
      expect(soleBlockHasClass(container, expectedClass)).toBe(true);
    },
  );

  it.each([
    { isFixed: true, expectedHasClass: true },
    { isFixed: false, expectedHasClass: false },
  ])(
    'is_fixed=$isFixed → is-fixed class: $expectedHasClass',
    async ({ isFixed, expectedHasClass }) => {
      const item = baseItem({ id: 'c1', start_time: noonLocalISO(2026, 3, 23), is_fixed: isFixed });
      const { container } = await renderWeek({ items: [item] });
      expect(soleBlockHasClass(container, 'is-fixed')).toBe(expectedHasClass);
    },
  );
});

describe('WeekView — edge cases', () => {
  it('renders with fewer than 7 days without crashing', async () => {
    const threeDays = WEEK_DAYS.slice(0, 3);
    const { container } = await renderWeek({ days: threeDays });
    expect(container.querySelectorAll('.day-column')).toHaveLength(3);
  });

  it('item outside the displayed week does not appear in any column', async () => {
    const outsideItem = baseItem({
      id: 'outside',
      start_time: noonLocalISO(2026, 4, 6), // Next week
      task_title: 'Outside Task',
    });
    const { container } = await renderWeek({ items: [outsideItem] });
    expect(container.textContent).not.toContain('Outside Task');
  });

  it('handles empty task_title gracefully', async () => {
    const item = baseItem({
      id: 'c1',
      start_time: noonLocalISO(2026, 3, 23),
      task_title: '',
    });
    const { container } = await renderWeek({ items: [item] });
    // Block still renders — empty title is a valid edge case
    expect(container.querySelectorAll('.chunk-block')).toHaveLength(1);
  });

  it('renders with empty days array without crashing', async () => {
    const { container } = await renderWeek({ days: [] });
    expect(container.querySelector('.week-view')).toBeTruthy();
    expect(container.querySelectorAll('.day-column')).toHaveLength(0);
  });
});

function makeWindow(id: string, day_of_week: ScheduleWindow['day_of_week']): ScheduleWindow {
  return {
    id,
    schedule_id: 'sched-1',
    day_of_week,
    start_time: '09:00:00',
    end_time: '11:00:00',
  };
}

describe('WeekView — schedule window overlay', () => {
  it.each([
    {
      label: 'Monday window',
      windows: [makeWindow('w1', 'Mon')] as ScheduleWindow[] | undefined,
      bandsByColumn: [1, 0, 0, 0, 0, 0, 0],
    },
    {
      label: 'windows prop omitted',
      windows: undefined as ScheduleWindow[] | undefined,
      bandsByColumn: [0, 0, 0, 0, 0, 0, 0],
    },
    {
      label: 'all 7 weekdays',
      windows: [
        makeWindow('wm', 'Mon'),
        makeWindow('wt', 'Tue'),
        makeWindow('ww', 'Wed'),
        makeWindow('wth', 'Thu'),
        makeWindow('wf', 'Fri'),
        makeWindow('ws', 'Sat'),
        makeWindow('wsu', 'Sun'),
      ] as ScheduleWindow[] | undefined,
      bandsByColumn: [1, 1, 1, 1, 1, 1, 1],
    },
    {
      label: 'empty array',
      windows: [] as ScheduleWindow[] | undefined,
      bandsByColumn: [0, 0, 0, 0, 0, 0, 0],
    },
  ])('schedule window overlay — $label', async ({ windows, bandsByColumn }) => {
    const { container } = await renderWeek(windows !== undefined ? { windows } : {});
    const columns = container.querySelectorAll('.day-column');
    for (let i = 0; i < 7; i++) {
      expect(columns[i].querySelectorAll('.schedule-window-band')).toHaveLength(bandsByColumn[i]);
    }
  });
});

describe('WeekView — data-column-date attribute', () => {
  it('every day column has a data-column-date attribute', async () => {
    const { container } = await renderWeek();
    const columns = container.querySelectorAll('.day-column');
    expect(columns).toHaveLength(7);
    for (const col of columns) {
      expect(col.getAttribute('data-column-date')).not.toBeNull();
    }
  });

  const columnEpochCases = WEEK_DAYS.map((day, index) => ({
    index,
    expectedEpoch: day.getTime(),
    label: day.toDateString(),
  }));

  it.each(columnEpochCases)(
    'column $index ($label) has data-column-date equal to day epoch $expectedEpoch',
    async ({ index, expectedEpoch }) => {
      const { container } = await renderWeek();
      const columns = container.querySelectorAll('.day-column');
      const epoch = Number(columns[index].getAttribute('data-column-date'));
      expect(epoch).toBe(expectedEpoch);
    },
  );

  it('data-column-date epoch can be parsed back to the original date', async () => {
    const { container } = await renderWeek();
    const columns = container.querySelectorAll('.day-column');
    for (let i = 0; i < columns.length; i++) {
      const epoch = Number(columns[i].getAttribute('data-column-date'));
      const recovered = new Date(epoch);
      expect(recovered.getFullYear()).toBe(WEEK_DAYS[i]!.getFullYear());
      expect(recovered.getMonth()).toBe(WEEK_DAYS[i]!.getMonth());
      expect(recovered.getDate()).toBe(WEEK_DAYS[i]!.getDate());
    }
  });

  it('with fewer than 7 days each column still carries a distinct data-column-date', async () => {
    const threeDays = WEEK_DAYS.slice(0, 3);
    const { container } = await renderWeek({ days: threeDays });
    const columns = container.querySelectorAll('.day-column');
    const epochs = Array.from(columns).map((c) => c.getAttribute('data-column-date'));
    const unique = new Set(epochs);
    expect(unique.size).toBe(3);
  });
});

describe('WeekView — past-wash', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('past columns have full-height wash, today partial, future none', async () => {
    // Mon (index 0) and Tue (index 1) are past → height 1440px.
    // Wed (index 2) is today at 10:30 → height 630px.
    // Thu–Sun (indices 3–6) are future → no .past-wash element.
    const { container } = await renderWeekAt(FROZEN_TODAY);

    const columns = container.querySelectorAll('.day-column');
    expect(columns).toHaveLength(7);

    const monWash = columns[0].querySelector('.past-wash') as HTMLElement | null;
    expect(monWash).toBeTruthy();
    expect(monWash!.style.height).toBe('1440px');

    const tueWash = columns[1].querySelector('.past-wash') as HTMLElement | null;
    expect(tueWash).toBeTruthy();
    expect(tueWash!.style.height).toBe('1440px');

    const wedWash = columns[2].querySelector('.past-wash') as HTMLElement | null;
    expect(wedWash).toBeTruthy();
    expect(wedWash!.style.height).toBe('630px');

    for (let i = 3; i < 7; i++) {
      expect(columns[i].querySelector('.past-wash')).toBeNull();
    }
  });

  it('past-wash elements are aria-hidden', async () => {
    const { container } = await renderWeekAt(FROZEN_TODAY);

    const washes = container.querySelectorAll('.past-wash');
    // There should be washes for Mon, Tue, and Wed (today)
    expect(washes).toHaveLength(3);
    for (const w of washes) {
      expect(w.getAttribute('aria-hidden')).toBe('true');
    }
  });
});

function fakeRect(left: number, right: number, top = 0, bottom = 0): DOMRect {
  return {
    left,
    right,
    top,
    bottom,
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
    toJSON: () => ({}),
  } as DOMRect;
}

/** Stub geometry to make drag math work in jsdom. */
function stubRect(el: Element, r: DOMRect): void {
  Object.assign(el, { getBoundingClientRect: () => r });
}

/**
 * Render a single-chunk week and start a (non-self-driven) move drag on it.
 * Returns the day-columns element and the started block.
 */
async function startWeekDrag(
  props: Partial<WeekProps> = {},
): Promise<{ container: HTMLElement; daycols: HTMLElement }> {
  const item = baseItem({
    id: 'c1',
    start_time: noonLocalISO(2026, 3, 23), // Monday noon
    task_title: 'Mon Task',
  });
  const { container } = await renderWeek({ items: [item], ...props });

  const daycols = container.querySelector('.day-columns') as HTMLElement;
  const block = container.querySelector('.chunk-block') as HTMLElement;
  Object.assign(daycols, { setPointerCapture: vi.fn() });
  stubRect(daycols, fakeRect(100, 800));
  // Block has real height so pointerdown is read as a move, not a resize.
  stubRect(block, fakeRect(100, 200, 0, 60));

  await fireEvent.pointerDown(block, { button: 0, pointerId: 1, clientX: 120, clientY: 5 });
  await tick();
  return { container, daycols };
}

async function startMoveDrag(): Promise<{
  container: HTMLElement;
  daycols: HTMLElement;
  onchunkmove: ReturnType<typeof vi.fn>;
  onchunkopen: ReturnType<typeof vi.fn>;
}> {
  const onchunkmove = vi.fn();
  const onchunkopen = vi.fn();
  const { container, daycols } = await startWeekDrag({ onchunkmove, onchunkopen });
  return { container, daycols, onchunkmove, onchunkopen };
}

describe('WeekView — cross-week drag', () => {
  afterEach(() => {
    vi.useRealTimers();
    dragState.cancel();
  });

  it.each([
    { label: 'right', clientX: 790, direction: 1 },
    { label: 'left', clientX: 110, direction: -1 },
  ])('flips $direction after dwelling at the $label edge', async ({ clientX, direction }) => {
    vi.useFakeTimers();
    const onweekflip = vi.fn();
    const { daycols } = await startWeekDrag({ onweekflip });
    await fireEvent.pointerMove(daycols, { pointerId: 1, clientX, clientY: 100 });
    vi.advanceTimersByTime(WEEK_FLIP_DWELL_MS - 1);
    expect(onweekflip).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onweekflip).toHaveBeenCalledWith(direction);
  });

  it('does not flip while the pointer stays in the middle', async () => {
    vi.useFakeTimers();
    const onweekflip = vi.fn();
    const { daycols } = await startWeekDrag({ onweekflip });

    await fireEvent.pointerMove(daycols, { pointerId: 1, clientX: 450, clientY: 100 });
    vi.advanceTimersByTime(WEEK_FLIP_DWELL_MS * 2);
    expect(onweekflip).not.toHaveBeenCalled();
  });

  it('commits a move via onchunkmove when dropped after moving', async () => {
    const { container, daycols, onchunkmove, onchunkopen } = await startMoveDrag();

    // Move down the column (offsetY=5, so clientY 305 → ~5:00) then drop.
    await fireEvent.pointerMove(daycols, { pointerId: 1, clientX: 450, clientY: 305 });
    await fireEvent.pointerUp(daycols, { pointerId: 1, clientX: 450, clientY: 305 });

    // The browser's follow-up click still lands on the chunk's own (unmoved)
    // element — pointer capture on daycols does not retarget click — so it must
    // not reopen the task after a committed move.
    const block = container.querySelector('.chunk-block') as HTMLElement;
    await fireEvent.click(block);

    expect(onchunkmove).toHaveBeenCalledTimes(1);
    expect(onchunkopen).not.toHaveBeenCalled();
    const [chunkId] = onchunkmove.mock.calls[0] as [string, string, string];
    expect(chunkId).toBe('c1');
  });

  it('treats a drag that never moves as a click (opens the task)', async () => {
    const { container, daycols, onchunkmove, onchunkopen } = await startMoveDrag();

    // Release without any pointermove → no movement. The container's pointerup
    // commits nothing; the browser's own follow-up click (not retargeted by
    // pointer capture) lands on the chunk and opens it — exactly once.
    await fireEvent.pointerUp(daycols, { pointerId: 1, clientX: 120, clientY: 5 });
    const block = container.querySelector('.chunk-block') as HTMLElement;
    await fireEvent.click(block);

    expect(onchunkopen).toHaveBeenCalledTimes(1);
    expect(onchunkopen).toHaveBeenCalledWith('task-1');
    expect(onchunkmove).not.toHaveBeenCalled();
  });

  it('does not open or move when a drag wanders and returns to the same slot', async () => {
    const { container, daycols, onchunkmove, onchunkopen } = await startMoveDrag();

    // Drag well away from the press point so the gesture is flagged as a real
    // drag, then bring the snapped slot back to the original (clientY 725 → noon,
    // the chunk's start) before releasing. The release point still sits over the
    // chunk's own (unmoved) element — its "shadow" at the original slot — so the
    // browser's follow-up click lands there and must be ignored.
    await fireEvent.pointerMove(daycols, { pointerId: 1, clientX: 120, clientY: 305 });
    await fireEvent.pointerMove(daycols, { pointerId: 1, clientX: 120, clientY: 725 });
    await fireEvent.pointerUp(daycols, { pointerId: 1, clientX: 120, clientY: 725 });
    const block = container.querySelector('.chunk-block') as HTMLElement;
    await fireEvent.click(block);

    expect(onchunkopen).not.toHaveBeenCalled();
    expect(onchunkmove).not.toHaveBeenCalled();
  });

  it('shows the armed edge highlight while dwelling at an edge', async () => {
    const { container, daycols } = await startWeekDrag({ onweekflip: vi.fn() });

    await fireEvent.pointerMove(daycols, { pointerId: 1, clientX: 790, clientY: 100 });
    await tick();

    const armed = container.querySelector('.edge-zone--right.edge-zone--armed');
    expect(armed).toBeTruthy();
    const leftArmed = container.querySelector('.edge-zone--left.edge-zone--armed');
    expect(leftArmed).toBeNull();
  });

  it('Escape cancels an in-flight drag', async () => {
    const onchunkmove = vi.fn();
    await startWeekDrag({ onchunkmove });
    expect(dragState.active).not.toBeNull();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await tick();

    expect(dragState.active).toBeNull();
  });
});

describe('WeekView — external events', () => {
  it.each([
    { label: 'on displayed week', externalEvent: satExternal(), expectedCount: 1 },
    {
      label: 'outside displayed week',
      externalEvent: externalEventFixture({
        start_time: new Date(2026, 2, 30, 12, 0, 0).toISOString(),
        end_time: new Date(2026, 2, 30, 13, 0, 0).toISOString(),
      }),
      expectedCount: 0,
    },
  ])(
    'external event $label — $expectedCount rendered',
    async ({ externalEvent, expectedCount }) => {
      const { container } = await renderWeek({ externalEvents: [externalEvent] });
      expect(container.querySelectorAll('.external-event')).toHaveLength(expectedCount);
    },
  );

  it('declined external event has the --declined modifier class', async () => {
    const { container } = await renderWeek({ externalEvents: [satExternal({ declined: true })] });

    const block = container.querySelector('.external-event');
    expect(block).toBeTruthy();
    expect(block!.classList.contains('external-event--declined')).toBe(true);
  });

  it('empty-state indicator is not affected by external events alone', async () => {
    const { container } = await renderWeek({ externalEvents: [satExternal()] });

    // Saturday column (index 5) has an external event but no chunks → still shows "—"
    const columns = container.querySelectorAll('.day-column');
    const satColumn = columns[5] as HTMLElement;
    expect(satColumn.querySelector('.empty-state')).toBeTruthy();
  });
});

function allDayExternal(overrides: Partial<ExternalEvent> = {}): ExternalEvent {
  // March 28, 2026 is Saturday (index 5 in WEEK_DAYS).
  return externalEventFixture({
    all_day: true,
    title: 'Vacation',
    start_time: new Date(2026, 2, 28, 0, 0, 0).toISOString(),
    end_time: new Date(2026, 2, 29, 0, 0, 0).toISOString(),
    ...overrides,
  });
}

describe('WeekView — all-day lane', () => {
  it('renders an all-day event in the all-day lane, not a day column', async () => {
    const { container } = await renderWeek({ externalEvents: [allDayExternal()] });

    const lane = container.querySelector('.week-all-day');
    expect(lane).toBeTruthy();
    expect(lane!.querySelectorAll('.external-event--allday')).toHaveLength(1);
    // The timed grid columns must not carry the all-day block.
    const cols = container.querySelector('.day-columns') as HTMLElement;
    expect(cols.querySelectorAll('.external-event')).toHaveLength(0);
  });

  it('places the all-day event in its own day cell', async () => {
    const { container } = await renderWeek({ externalEvents: [allDayExternal()] });

    const cells = container.querySelectorAll('.all-day-cell');
    expect(cells).toHaveLength(7);
    expect(cells[5].querySelectorAll('.external-event')).toHaveLength(1); // Saturday
    expect(cells[0].querySelectorAll('.external-event')).toHaveLength(0);
  });

  it('does not render the all-day lane when there are no all-day events', async () => {
    const { container } = await renderWeek({ externalEvents: [satExternal()] });
    expect(container.querySelector('.week-all-day')).toBeNull();
  });
});

describe('WeekView — external event editing', () => {
  it('makes a primary-calendar external clickable and forwards to oneventopen', async () => {
    const oneventopen = vi.fn();
    const { container } = await renderWeek({
      externalEvents: [satExternal({ calendar_id: 'primary' })],
      oneventopen,
      editableCalendarId: 'primary',
    });

    const clickable = await isExternalEventClickableAndOpens(container, oneventopen, 'primary');
    expect(clickable).toBe(true);
  });

  it('leaves a non-primary external read-only', async () => {
    const { container } = await renderWeek({
      externalEvents: [satExternal({ calendar_id: 'other-cal' })],
      oneventopen: vi.fn(),
      editableCalendarId: 'primary',
    });
    expect(container.querySelector('.external-event')!.getAttribute('role')).toBe('img');
  });
});
