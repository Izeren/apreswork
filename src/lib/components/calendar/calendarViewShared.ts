// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared prop contract, schedule-window helpers, and injectable CalendarApi for CalendarView.

import type {
  AgendaItem,
  AppConfig,
  AuthStatus,
  Chunk,
  CreateTaskInput,
  ExternalCalendar,
  ExternalEvent,
  ScheduleResult,
  ScheduleWindow,
  SyncOutcome,
  Task,
  UpdateConfigInput,
  UpdateTaskInput,
  UserEventPayload,
} from '../../types';
import * as api from '../../api';
import { HOUR_HEIGHT_PX } from './calendarLayout';
import { resolveEventOpenHandler } from './externalEventInteractivity';
import { isSameLocalDate, parseTimeToHours } from '../../utils';

/** Props common to the day and week calendar views (data + interaction callbacks). */
export interface CalendarViewCommonProps {
  items: AgendaItem[];
  /** Current wall-clock time injected by the parent — drives past-wash and the time indicator. */
  now: Date;
  windows?: ScheduleWindow[];
  externalEvents?: ExternalEvent[];
  oneventopen?: ((event: ExternalEvent) => void) | null;
  /** Calendar id whose events are editable (the primary calendar), or null. */
  editableCalendarId?: string | null;
  onchunkopen?: ((taskId: string) => void) | null;
  onchunkcomplete?: ((item: AgendaItem) => void) | null;
  onchunkmove?: ((chunkId: string, newStart: string, newEnd: string) => void) | null;
  onchunkresize?: ((chunkId: string, newEnd: string) => void) | null;
  onchunkmenu?: ((item: AgendaItem, x: number, y: number) => void) | null;
  onchunklock?: ((item: AgendaItem) => void) | null;
  oncreatechunk?: ((start: string, end: string) => void) | null;
  /** When true, external event blocks render with a disconnected visual (stale data). */
  disconnected?: boolean;
}

// Indexed by Date.getDay() (0 = Sun … 6 = Sat).
const WEEKDAY_NAMES: ScheduleWindow['day_of_week'][] = [
  'Sun',
  'Mon',
  'Tue',
  'Wed',
  'Thu',
  'Fri',
  'Sat',
];

/** Filter `arr` to items whose `getStartTime` falls on the same local calendar date as `day`. */
export function filterByDay<T>(arr: T[], getStartTime: (item: T) => string, day: Date): T[] {
  return arr.filter((item) => isSameLocalDate(new Date(getStartTime(item)), day));
}

export function weekdayName(day: Date): ScheduleWindow['day_of_week'] {
  return WEEKDAY_NAMES[day.getDay()];
}

/** Bind the shared primary-calendar editability policy to a view's open-event props. */
export function makeEventOpenHandler(
  oneventopen: ((event: ExternalEvent) => void) | null,
  editableCalendarId: string | null,
): (ext: ExternalEvent) => ((event: ExternalEvent) => void) | null {
  return (ext) => resolveEventOpenHandler(oneventopen, editableCalendarId, ext);
}

/**
 * Initial grid scroll offset that puts the earliest of `windows` one hour
 * below the top edge; 0 when there are no windows.
 */
export function earliestWindowScrollTop(windows: ScheduleWindow[]): number {
  if (windows.length === 0) return 0;
  const earliestHour = Math.min(...windows.map((window) => parseTimeToHours(window.start_time)));
  return Math.max(0, (earliestHour - 1) * HOUR_HEIGHT_PX);
}

/** Injectable subset of api functions used by CalendarView. */
export interface CalendarApi {
  getAgenda: (start: string, end: string, label?: string | null) => Promise<AgendaItem[]>;
  listExternalEvents: (start: string, end: string) => Promise<ExternalEvent[]>;
  googleListCalendars: () => Promise<ExternalCalendar[]>;
  getConfig: () => Promise<AppConfig>;
  moveChunk: (chunkId: string, newStart: string, newEnd: string) => Promise<Chunk>;
  resizeChunk: (chunkId: string, newEnd: string) => Promise<[Chunk, Task]>;
  getTask: (id: string) => Promise<Task>;
  createUserEvent: (calendarId: string, payload: UserEventPayload) => Promise<ExternalEvent>;
  updateUserEvent: (
    calendarId: string,
    eventId: string,
    payload: UserEventPayload,
  ) => Promise<ExternalEvent>;
  deleteUserEvent: (calendarId: string, eventId: string) => Promise<void>;
  updateTask: (id: string, input: UpdateTaskInput) => Promise<Task>;
  createTask: (input: CreateTaskInput) => Promise<Task>;
  createFixedChunk: (taskId: string, startTime: string, endTime: string) => Promise<[Chunk, Task]>;
  updateConfig: (input: UpdateConfigInput) => Promise<AppConfig>;
  googleAuthStatus: () => Promise<AuthStatus>;
  /** Satisfies RescheduleApiSubset so apiClient can be passed to runReschedule. */
  triggerReschedule: () => Promise<ScheduleResult>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
  /** Satisfies SyncApiSubset so apiClient can be passed to runSync. */
  syncNow: () => Promise<SyncOutcome>;
  syncErrorMessage: (e: unknown, fallback: string) => string;
  /** Chunk completion/reopen — satisfies CompletionFlowApi so apiClient can be passed to CompletionFlow. */
  completeChunk: (chunkId: string) => Promise<[Chunk, Task]>;
  completeTask: (taskId: string) => Promise<Task>;
  reopenChunk: (chunkId: string) => Promise<[Chunk, Task]>;
  listChunksForTask: (taskId: string) => Promise<Chunk[]>;
  /** Task/chunk mutations — satisfies TaskActionsApiSubset so apiClient can be passed to TaskActions. */
  cancelTask: (taskId: string) => Promise<Task>;
  lockChunk: (chunkId: string) => Promise<Chunk>;
  unlockChunk: (chunkId: string) => Promise<Chunk>;
  deleteFixedChunk: (chunkId: string) => Promise<Chunk>;
  deleteTask: (taskId: string) => Promise<void>;
}

/** Production default for the `apiClient` prop; the full `api` module satisfies the subset interface. */
export const defaultCalendarApi: CalendarApi = api;
