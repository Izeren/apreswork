// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared fakes, fixtures, and lifecycle hooks for the CalendarView test files.
// Not collected by vitest (no .test suffix); imported by the CalendarView*.test.ts files.

import { vi, afterEach, beforeEach, type Mock, type Mocked } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { ScheduleState } from '../../stores/schedules.svelte';
import { tick } from 'svelte';
import { warningState } from '../../stores/warnings.svelte';
import { chunkFixture, configFixture } from '../../testFixtures';
import { syncErrorMessage, apiErrorMessage } from '../../api';
import type {
  AgendaItem,
  Chunk,
  ChunkStatus,
  ExternalEvent,
  Schedule,
  Task,
  Weekday,
} from '../../types';

import type { CalendarApi } from './calendarViewShared';

export const CALENDAR_TEST_NOW = new Date(2026, 2, 28, 12, 0, 0);

export function taskFixture(overrides: Partial<Task> = {}): Task {
  return {
    id: 'task-1',
    title: 'Task from calendar',
    description: null,
    duration_minutes: 60,
    time_logged_minutes: 0,
    priority: 'Medium',
    status: 'scheduled',
    start_date: null,
    deadline: '2026-03-29T10:00:00.000Z',
    schedule_id: 'sched-1',
    min_chunk_minutes: 15,
    no_split: false,
    recurring_template_id: null,
    labels: [],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function completedChunkFixture(): Chunk {
  return chunkFixture({
    status: 'completed',
    logged_minutes: 60,
    completed_at: '2026-03-28T13:00:00.000Z',
  });
}

/** Base external event fixture for 2026-03-28 12:00–13:00 UTC (matches other fixture dates). */
export function externalEventFixture(overrides: Partial<ExternalEvent> = {}): ExternalEvent {
  return {
    id: 'ext-1',
    calendar_id: 'cal-primary',
    event_id: 'provider-event-1',
    title: 'Team meeting',
    description: null,
    start_time: '2026-03-28T12:00:00.000Z',
    end_time: '2026-03-28T13:00:00.000Z',
    busy: true,
    declined: false,
    all_day: false,
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

export function scheduledAgendaItem(templateId: string | null = null): AgendaItem {
  return {
    chunk: chunkFixture(),
    task_title: 'Task from calendar',
    task_priority: 'Medium',
    task_labels: [],
    task_recurring_template_id: templateId,
    task_deadline: null,
  };
}

export function completedAgendaItem(): AgendaItem {
  return { ...scheduledAgendaItem(), chunk: completedChunkFixture() };
}

export function fixedAgendaItem(): AgendaItem {
  return { ...scheduledAgendaItem(), chunk: chunkFixture({ is_fixed: true }) };
}

/** One schedule ('sched-1', default) with a single window. */
export function scheduleWithWindow(
  day_of_week: Weekday,
  start_time: string,
  end_time: string,
): Schedule[] {
  return [
    {
      id: 'sched-1',
      name: 'Default',
      is_default: true,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      windows: [{ id: 'w1', schedule_id: 'sched-1', day_of_week, start_time, end_time }],
    },
  ];
}

function newTaskFixture(): Task {
  return taskFixture({ id: 'task-new', title: 'New task', status: 'pending' });
}

function newFixedChunkResult(): [Chunk, Task] {
  return [
    chunkFixture({ id: 'chunk-new', task_id: 'task-new', is_fixed: true }),
    taskFixture({ id: 'task-new', title: 'New task' }),
  ];
}

export function calendarApiFake(): Mocked<CalendarApi> {
  const fake: { [K in keyof CalendarApi]: Mock } = {
    getAgenda: vi.fn().mockResolvedValue([]),
    listExternalEvents: vi.fn().mockResolvedValue([]),
    googleListCalendars: vi.fn().mockResolvedValue([]),
    getConfig: vi.fn().mockResolvedValue(configFixture()),
    triggerReschedule: vi.fn().mockResolvedValue({ placed_chunks: [], warnings: [] }),
    apiErrorMessage: vi.fn().mockImplementation(apiErrorMessage),
    syncNow: vi.fn().mockResolvedValue({
      schedule: { placed_chunks: [], warnings: [] },
      pushed: { created: 0, updated: 0, deleted: 0 },
    }),
    syncErrorMessage: vi.fn().mockImplementation(syncErrorMessage),
    moveChunk: vi.fn().mockResolvedValue(chunkFixture()),
    resizeChunk: vi.fn().mockResolvedValue([chunkFixture(), taskFixture()]),
    getTask: vi.fn().mockResolvedValue(taskFixture()),
    createUserEvent: vi.fn().mockResolvedValue(externalEventFixture()),
    updateUserEvent: vi.fn().mockResolvedValue(externalEventFixture()),
    deleteUserEvent: vi.fn().mockResolvedValue(undefined),
    updateTask: vi.fn().mockResolvedValue(taskFixture()),
    createTask: vi.fn().mockResolvedValue(newTaskFixture()),
    createFixedChunk: vi.fn().mockResolvedValue(newFixedChunkResult()),
    updateConfig: vi.fn().mockResolvedValue(configFixture()),
    googleAuthStatus: vi.fn().mockResolvedValue({ type: 'not_connected' }),
    completeChunk: vi.fn().mockResolvedValue([chunkFixture(), taskFixture()]),
    completeTask: vi
      .fn()
      .mockResolvedValue(taskFixture({ time_logged_minutes: 60, status: 'completed' })),
    reopenChunk: vi.fn().mockResolvedValue([chunkFixture(), taskFixture()]),
    listChunksForTask: vi.fn().mockResolvedValue([]),
    cancelTask: vi.fn().mockResolvedValue(taskFixture({ status: 'cancelled' })),
    lockChunk: vi.fn().mockResolvedValue(chunkFixture({ is_fixed: true })),
    unlockChunk: vi.fn().mockResolvedValue(chunkFixture()),
    deleteFixedChunk: vi.fn().mockResolvedValue(chunkFixture()),
    deleteTask: vi.fn().mockResolvedValue(undefined),
  };
  return fake as Mocked<CalendarApi>;
}

function sharedCleanup(): void {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
}

export function installCalendarHooks() {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(CALENDAR_TEST_NOW);
  });

  afterEach(() => {
    sharedCleanup();
    warningState.items = [];
  });
}

/** Build a Date in local time (avoids UTC conversion surprises for y/m/d). */
export function localDate(year: number, month: number, day: number): Date {
  return new Date(year, month - 1, day);
}

export function installViewTestHooks() {
  afterEach(() => {
    sharedCleanup();
  });
}

export const chunkStatusCases: Array<{ status: ChunkStatus; expectedClass: string }> = [
  { status: 'scheduled', expectedClass: 'chunk-block--scheduled' },
  { status: 'completed', expectedClass: 'chunk-block--completed' },
];

export function soleBlockHasClass(container: HTMLElement, className: string): boolean {
  const block = container.querySelector('.chunk-block');
  return block !== null && block.classList.contains(className);
}

export function hasTwoColumnOverlapLayout(blocks: HTMLElement[]): boolean {
  return (
    blocks.length === 2 &&
    blocks[0]?.dataset.overlapCount === '2' &&
    (blocks[0]?.style.width ?? '').includes('calc(') &&
    (blocks[1]?.style.left ?? '').includes('calc(')
  );
}

export async function isOpenAfterClick(
  renderView: () => Promise<{ container: HTMLElement }>,
  onchunkopen: ReturnType<typeof vi.fn>,
): Promise<boolean> {
  const { container } = await renderView();
  const block = container.querySelector('.chunk-block');
  if (!block) return false;
  await fireEvent.click(block);
  return onchunkopen.mock.calls.length === 1 && onchunkopen.mock.calls[0][0] === 'task-1';
}

export async function isExternalEventClickableAndOpens(
  container: HTMLElement,
  oneventopen: ReturnType<typeof vi.fn>,
  calendarId: string,
): Promise<boolean> {
  const block = container.querySelector('.external-event') as HTMLElement;
  const isButton = block.getAttribute('role') === 'button';
  await fireEvent.click(block);
  const openedCorrectly =
    oneventopen.mock.calls.length === 1 && oneventopen.mock.calls[0][0].calendar_id === calendarId;
  return isButton && openedCorrectly;
}

export async function importCalendarView() {
  const mod = await import('./CalendarView.svelte');
  return mod.default;
}

export async function renderCalendarView(
  apiClient: Mocked<CalendarApi>,
  getNow: () => Date = () => CALENDAR_TEST_NOW,
  schedulesStore?: ScheduleState,
) {
  const CalendarView = await importCalendarView();
  const utils = render(CalendarView, { props: { getNow, apiClient, schedulesStore } });
  await tick();
  return utils;
}

type GetByRole = Awaited<ReturnType<typeof renderCalendarView>>['getByRole'];

export async function settle(): Promise<void> {
  await Promise.resolve();
  await tick();
}

export function modeButton(getByRole: GetByRole, label: 'Day' | 'Week'): HTMLButtonElement {
  const group = getByRole('group', { name: /view mode/i });
  return Array.from(group.querySelectorAll('button')).find(
    (button) => button.textContent?.trim() === label,
  )!;
}

export async function switchToDayMode(getByRole: GetByRole): Promise<HTMLButtonElement> {
  const dayBtn = modeButton(getByRole, 'Day');
  await fireEvent.click(dayBtn);
  await tick();
  return dayBtn;
}

export async function clickNav(getByRole: GetByRole, name: RegExp): Promise<void> {
  await fireEvent.click(getByRole('button', { name }));
  await tick();
}

export function dateHeader(container: HTMLElement): string {
  return container.querySelector('.date-header')!.textContent ?? '';
}

/** The clicked chunk (`chunk-1`) plus a sibling `chunk-2`, both scheduled — the
 *  multi-chunk `listChunksForTask` setup the completion-dialog tests share. */
export function twoScheduledChunks(): Chunk[] {
  return [
    chunkFixture(),
    chunkFixture({
      id: 'chunk-2',
      start_time: '2026-03-28T14:00:00.000Z',
      end_time: '2026-03-28T15:00:00.000Z',
    }),
  ];
}

export async function openCompletionDialog(
  apiClient: Mocked<CalendarApi>,
): Promise<Awaited<ReturnType<typeof renderCalendarView>>> {
  const utils = await renderCalendarView(apiClient);
  await settle();
  const toggle = utils.container.querySelector('.complete-toggle') as HTMLElement | null;
  if (!toggle) throw new Error('.complete-toggle not found');
  await fireEvent.click(toggle);
  await tick();
  return utils;
}

export function parseTimeRange(
  start: string,
  end: string,
): { hours: number; minutes: number; durationMs: number } {
  const startDate = new Date(start);
  return {
    hours: startDate.getHours(),
    minutes: startDate.getMinutes(),
    durationMs: new Date(end).getTime() - startDate.getTime(),
  };
}

export function makeInjectedSchedule(): {
  fakeListSchedules: Mock<() => Promise<Schedule[]>>;
  injectedSchedule: ScheduleState;
} {
  const fakeListSchedules = vi.fn<() => Promise<Schedule[]>>().mockResolvedValue([]);
  const injectedSchedule = new ScheduleState({
    listSchedules: fakeListSchedules,
    createSchedule: vi.fn(),
    updateSchedule: vi.fn(),
    deleteSchedule: vi.fn(),
  });
  return { fakeListSchedules, injectedSchedule };
}

export async function dragEmptySlot(container: HTMLElement): Promise<void> {
  const hitArea = container.querySelector('.create-hit-area') as HTMLElement | null;
  if (!hitArea) throw new Error('.create-hit-area not found in container');
  Object.assign(hitArea, {
    setPointerCapture: vi.fn(),
    releasePointerCapture: vi.fn(),
  });
  await fireEvent.pointerDown(hitArea!, { button: 0, clientY: 540, pointerId: 1 });
  await fireEvent.pointerUp(hitArea!, { button: 0, clientY: 540, pointerId: 1 });
  await tick();
}
