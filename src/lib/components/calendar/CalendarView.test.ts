// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Schedule } from '../../types';
import type { ScheduleState } from '../../stores/schedules.svelte';
import { warningState } from '../../stores/warnings.svelte';
import { weekdayName } from './calendarViewShared';
import {
  CALENDAR_TEST_NOW,
  installCalendarHooks,
  externalEventFixture,
  importCalendarView,
  renderCalendarView,
  settle,
  modeButton,
  switchToDayMode,
  clickNav,
  dateHeader,
  scheduleWithWindow,
  calendarApiFake,
  makeInjectedSchedule,
} from './testFixtures';

const { calendarFocusState } = await import('../../stores/calendarFocus.svelte');
const { formatWeekHeader } = await import('../../utils');
const { toastState } = await import('../../stores/toast.svelte');

let fake: ReturnType<typeof calendarApiFake>;

installCalendarHooks();

beforeEach(() => {
  fake = calendarApiFake();
  toastState.items = [];
});

afterEach(() => {
  toastState.items = [];
});

const CONNECTED_AUTH = { type: 'connected' as const, email: 'a@b.c' };
const EXT_FIXTURE_ARGS = {
  start_time: '2026-03-28T12:00:00.000Z',
  end_time: '2026-03-28T13:00:00.000Z',
} as const;

function isIsoString(s: string): boolean {
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/.test(s);
}

describe('CalendarView — mode toggle', () => {
  it('renders Day and Week mode buttons', async () => {
    const { getByRole } = await renderCalendarView(fake);
    const group = getByRole('group', { name: /view mode/i });
    expect(group.querySelector('button[aria-pressed="true"]')).toBeTruthy();
  });

  type GetByRole = Awaited<ReturnType<typeof renderCalendarView>>['getByRole'];
  type ModeLabel = 'Day' | 'Week';

  it.each<{
    label: string;
    act: (g: GetByRole) => Promise<void>;
    active: ModeLabel;
    inactive: ModeLabel;
  }>([
    { label: 'Week active by default', act: async () => {}, active: 'Week', inactive: 'Day' },
    {
      label: 'Day becomes active after click',
      act: async (g) => {
        await switchToDayMode(g);
      },
      active: 'Day',
      inactive: 'Week',
    },
    {
      label: 'Week reactivates after Day then Week click',
      act: async (g) => {
        await switchToDayMode(g);
        await fireEvent.click(modeButton(g, 'Week'));
        await tick();
      },
      active: 'Week',
      inactive: 'Day',
    },
  ])('mode toggle: $label', async ({ act, active, inactive }) => {
    const { getByRole } = await renderCalendarView(fake);
    await act(getByRole);
    expect(modeButton(getByRole, active).getAttribute('aria-pressed')).toBe('true');
    expect(modeButton(getByRole, inactive).getAttribute('aria-pressed')).toBe('false');
  });
});

describe('CalendarView — navigation', () => {
  it.each([
    { label: 'day mode / next', dayMode: true, nav: /next/i },
    { label: 'day mode / prev', dayMode: true, nav: /previous/i },
    { label: 'week mode / next', dayMode: false, nav: /next/i },
    { label: 'week mode / prev', dayMode: false, nav: /previous/i },
  ])('$label changes the date header', async ({ dayMode, nav }) => {
    const { getByRole, container } = await renderCalendarView(fake);
    if (dayMode) await switchToDayMode(getByRole);
    const headerBefore = dateHeader(container);
    await clickNav(getByRole, nav);
    expect(dateHeader(container)).not.toBe(headerBefore);
  });
});

describe('CalendarView — navigation roundtrip', () => {
  type CalViewUtils = Awaited<ReturnType<typeof renderCalendarView>>;

  it.each([
    {
      label: 'day mode',
      prepare: async (utils: CalViewUtils) => switchToDayMode(utils.getByRole),
    },
    {
      label: 'week mode',
      prepare: async () => {},
    },
  ])('next then prev returns to original $label', async ({ prepare }) => {
    const utils = await renderCalendarView(fake);
    await prepare(utils);
    const headerOriginal = dateHeader(utils.container);
    await clickNav(utils.getByRole, /next/i);
    await clickNav(utils.getByRole, /previous/i);
    expect(dateHeader(utils.container)).toBe(headerOriginal);
  });
});

describe('CalendarView — Today button', () => {
  it('renders a Today button', async () => {
    const { getByText } = await renderCalendarView(fake);
    expect(getByText('Today')).toBeTruthy();
  });

  it('clicking Today after advancing resets the header to the original', async () => {
    const { getByRole, getByText, container } = await renderCalendarView(fake);
    const headerToday = dateHeader(container);

    await clickNav(getByRole, /next/i);
    expect(dateHeader(container)).not.toBe(headerToday);

    await fireEvent.click(getByText('Today'));
    await tick();

    expect(dateHeader(container)).toBe(headerToday);
  });
});

describe('CalendarView — date header format', () => {
  it('week mode shows a range header with en-dash', async () => {
    const { container } = await renderCalendarView(fake);
    expect(dateHeader(container)).toContain('–');
  });

  it('day mode shows a long-form date (includes year)', async () => {
    const { getByRole, container } = await renderCalendarView(fake);
    await switchToDayMode(getByRole);

    const header = dateHeader(container);
    expect(header).toMatch(/\d{4}/);
    expect(header).not.toContain('–');
  });

  it('injected now prop seeds the initial week (independent of system clock)', async () => {
    const CalendarView = await importCalendarView();
    const { container } = render(CalendarView, {
      props: { apiClient: fake, getNow: () => new Date(2026, 0, 15) },
    });
    await tick();
    // Jan 15 (Thu) is in Mon Jan 12 – Sun Jan 18 (system clock is 2026-03-28 — different week)
    expect(dateHeader(container)).toBe('Jan 12 – 18, 2026');
  });
});

describe('CalendarView — agenda loading', () => {
  type GetByRole = Awaited<ReturnType<typeof renderCalendarView>>['getByRole'];

  beforeEach(() => {
    fake.getAgenda.mockResolvedValue([]);
    fake.getAgenda.mockClear();
  });

  it('calls getAgenda on mount', async () => {
    await renderCalendarView(fake);
    await settle();
    expect(fake.getAgenda).toHaveBeenCalledTimes(1);
  });

  it('calls getAgenda with ISO strings', async () => {
    await renderCalendarView(fake);
    await settle();
    const [start, end] = fake.getAgenda.mock.calls[0];
    expect(isIsoString(start)).toBe(true);
    expect(isIsoString(end)).toBe(true);
  });

  it.each([
    {
      label: 'navigating forward',
      act: async (getByRole: GetByRole) => clickNav(getByRole, /next/i),
    },
    {
      label: 'switching from week to day mode',
      act: async (getByRole: GetByRole) => switchToDayMode(getByRole),
    },
  ])('calls getAgenda again when $label', async ({ act }) => {
    const { getByRole } = await renderCalendarView(fake);
    await settle();
    const callsBefore = fake.getAgenda.mock.calls.length;

    await act(getByRole);
    await settle();

    expect(fake.getAgenda.mock.calls.length).toBeGreaterThan(callsBefore);
  });

  it('shows loading indicator while fetching', async () => {
    let resolveAgenda!: (v: []) => void;
    fake.getAgenda.mockReturnValue(
      new Promise((res) => {
        resolveAgenda = res;
      }),
    );

    const { container } = await renderCalendarView(fake);
    expect(container.querySelector('.loading-bar')).toBeTruthy();

    resolveAgenda([]);
    await tick();
    await settle();
    expect(container.querySelector('.loading-bar')).toBeNull();
  });

  it('on getAgenda failure: clears loading, resets items, shows error toast', async () => {
    fake.getAgenda.mockRejectedValue(new Error('network error'));

    const { container } = await renderCalendarView(fake);
    await settle();

    expect(container.querySelector('.loading-bar')).toBeNull();
    expect(toastState.items.find((t) => t.level === 'error')?.text).toBe('Failed to load agenda');
  });
});

describe('CalendarView — week layout', () => {
  it.each([
    {
      label: 'week view',
      dayMode: false,
      weekViewExists: true,
      dayViewCount: 0,
      dayColumnCount: 7,
    },
    { label: 'day view', dayMode: true, weekViewExists: false, dayViewCount: 1, dayColumnCount: 0 },
  ])(
    'renders correct layout in $label',
    async ({ dayMode, weekViewExists, dayViewCount, dayColumnCount }) => {
      const { getByRole, container } = await renderCalendarView(fake);
      if (dayMode) await switchToDayMode(getByRole);
      expect(container.querySelector('.week-view') !== null).toBe(weekViewExists);
      expect(container.querySelectorAll('.day-view')).toHaveLength(dayViewCount);
      expect(container.querySelectorAll('.day-column')).toHaveLength(dayColumnCount);
    },
  );
});

describe('CalendarView — schedule window loading', () => {
  let fakeListSchedules: Mock<() => Promise<Schedule[]>>;
  let injectedSchedule: ScheduleState;

  beforeEach(() => {
    ({ fakeListSchedules, injectedSchedule } = makeInjectedSchedule());
  });

  it('calls scheduleState.load on mount', async () => {
    await renderCalendarView(fake, () => CALENDAR_TEST_NOW, injectedSchedule);
    await settle();
    expect(fakeListSchedules).toHaveBeenCalledTimes(1);
  });

  it('renders schedule-window-band elements when scheduleState has windows', async () => {
    fakeListSchedules.mockResolvedValue(
      scheduleWithWindow(weekdayName(CALENDAR_TEST_NOW), '09:00:00', '11:00:00'),
    );
    const { container } = await renderCalendarView(fake, () => CALENDAR_TEST_NOW, injectedSchedule);
    await settle();
    expect(container.querySelectorAll('.schedule-window-band').length).toBeGreaterThanOrEqual(1);
  });

  it('does not crash when scheduleState.items is empty', async () => {
    const { container } = await renderCalendarView(fake, () => CALENDAR_TEST_NOW, injectedSchedule);
    await settle();
    expect(container.querySelector('.calendar-view')).toBeTruthy();
    expect(container.querySelectorAll('.schedule-window-band')).toHaveLength(0);
  });
});

describe('CalendarView — reschedule warnings', () => {
  it('stores warnings returned from reschedule', async () => {
    const warning = {
      task_id: 'task-2',
      task_title: 'Calendar task',
      kind: {
        Unschedulable: {
          reason: 'No schedule windows are available this week.',
        },
      },
    };
    fake.triggerReschedule.mockResolvedValueOnce({
      placed_chunks: [],
      warnings: [warning],
    });

    const { getByRole } = await renderCalendarView(fake);
    await settle();

    await fireEvent.click(getByRole('button', { name: /reschedule tasks/i }));
    await settle();

    expect(warningState.items).toEqual([warning]);
  });
});

describe('CalendarView — scroll positioning', () => {
  let fakeListSchedules: Mock<() => Promise<Schedule[]>>;
  let injectedSchedule: ScheduleState;

  beforeEach(() => {
    ({ fakeListSchedules, injectedSchedule } = makeInjectedSchedule());
  });

  it.each([
    {
      label: 'week view',
      dayMode: false,
      selector: '.week-body',
      expectedScrollTop: 480,
      getSchedule: () => scheduleWithWindow('Mon', '09:00:00', '17:00:00'),
    },
    {
      label: 'day view',
      dayMode: true,
      selector: '.day-body',
      expectedScrollTop: 540,
      getSchedule: () => scheduleWithWindow(weekdayName(CALENDAR_TEST_NOW), '10:00:00', '17:00:00'),
    },
  ])(
    '$label scrolls to one hour before the earliest schedule start',
    async ({ dayMode, selector, expectedScrollTop, getSchedule }) => {
      fakeListSchedules.mockResolvedValue(getSchedule());

      const { container, getByRole } = await renderCalendarView(
        fake,
        () => CALENDAR_TEST_NOW,
        injectedSchedule,
      );
      await settle();
      if (dayMode) await switchToDayMode(getByRole);

      const body = container.querySelector(selector) as HTMLDivElement | null;
      expect(body).toBeTruthy();
      expect(body?.scrollTop).toBe(expectedScrollTop);
    },
  );
});

describe('CalendarView — cross-view focus requests', () => {
  afterEach(() => {
    calendarFocusState.clear();
  });

  it('jumps the visible range to a focus request made before mount', async () => {
    calendarFocusState.request('chunk-1', '2026-05-12T09:00:00.000Z');

    const { container } = await renderCalendarView(fake);

    expect(dateHeader(container)).toBe(formatWeekHeader(new Date('2026-05-12T09:00:00.000Z')));
  });

  it('jumps on a request while mounted and clears it after the flash window', async () => {
    const { container } = await renderCalendarView(fake);

    calendarFocusState.request('chunk-1', '2026-05-12T09:00:00.000Z');
    await tick();

    expect(dateHeader(container)).toBe(formatWeekHeader(new Date('2026-05-12T09:00:00.000Z')));
    expect(calendarFocusState.chunkId).toBe('chunk-1');

    vi.advanceTimersByTime(3000);
    expect(calendarFocusState.chunkId).toBeNull();
  });

  it('ignores an already-cleared carrier on mount', async () => {
    calendarFocusState.request('chunk-1', '2026-05-12T09:00:00.000Z');
    calendarFocusState.clear();

    const { container } = await renderCalendarView(fake);

    // System time is 2026-03-28 — the header must show the current week, not May.
    expect(dateHeader(container)).toBe(formatWeekHeader(CALENDAR_TEST_NOW));
  });
});

describe('CalendarView — external events loading', () => {
  beforeEach(() => {
    fake.getAgenda.mockResolvedValue([]);
    fake.getAgenda.mockClear();
    fake.listExternalEvents.mockClear();
  });

  it('calls listExternalEvents on mount with the visible range', async () => {
    fake.listExternalEvents.mockResolvedValue([]);

    await renderCalendarView(fake);
    await settle();

    expect(fake.listExternalEvents).toHaveBeenCalledTimes(1);
    const [start, end] = fake.listExternalEvents.mock.calls[0] as [string, string];
    expect(isIsoString(start)).toBe(true);
    expect(isIsoString(end)).toBe(true);
  });

  it('renders an external event block once the promise resolves', async () => {
    const ext = externalEventFixture(EXT_FIXTURE_ARGS);
    fake.listExternalEvents.mockResolvedValue([ext]);

    const { container } = await renderCalendarView(fake);
    await settle();

    expect(container.querySelectorAll('.external-event').length).toBeGreaterThanOrEqual(1);
  });

  it('on listExternalEvents failure: shows error toast and renders no block', async () => {
    fake.listExternalEvents.mockRejectedValue({ error: 'internal', message: 'boom' });

    const { container } = await renderCalendarView(fake);
    await settle();

    expect(toastState.items.find((t) => t.level === 'error')?.text).toBe(
      'Failed to load calendar events',
    );
    expect(container.querySelectorAll('.external-event')).toHaveLength(0);
  });
});

describe('CalendarView — Sync button', () => {
  const SYNC_NAME = /sync with google calendar/i;

  beforeEach(() => {
    fake.getAgenda.mockResolvedValue([]);
    fake.listExternalEvents.mockResolvedValue([]);
  });

  it.each([
    { label: 'factory default (not connected)', setup: async () => {} },
    {
      label: 'auth-status check fails',
      setup: async () => {
        fake.googleAuthStatus.mockRejectedValue(new Error('status unavailable'));
      },
    },
  ])('sync button hidden and silent: $label', async ({ setup }) => {
    await setup();
    const { queryByRole } = await renderCalendarView(fake);
    await settle();
    expect(queryByRole('button', { name: SYNC_NAME })).toBeNull();
    expect(toastState.items.some((t) => t.level === 'error')).toBe(false);
  });

  it('connected: click syncs, refetches agenda + externals, sets warnings, toasts', async () => {
    fake.googleAuthStatus.mockResolvedValue(CONNECTED_AUTH);
    fake.syncNow.mockResolvedValue({
      schedule: {
        placed_chunks: [],
        warnings: [
          {
            task_id: 'task-warn',
            task_title: 'Overdue Task',
            kind: { Unschedulable: { reason: 'no windows' } },
          },
        ],
      },
      pushed: { created: 1, updated: 1, deleted: 0 },
    });

    const { getByRole } = await renderCalendarView(fake);
    await settle();

    const syncBtn = getByRole('button', { name: SYNC_NAME });
    const agendaCallsBefore = fake.getAgenda.mock.calls.length;
    const externalCallsBefore = fake.listExternalEvents.mock.calls.length;

    await fireEvent.click(syncBtn);
    await tick();
    await settle();

    expect(fake.syncNow).toHaveBeenCalledOnce();
    expect(fake.getAgenda.mock.calls).toHaveLength(agendaCallsBefore + 1);
    expect(fake.listExternalEvents.mock.calls).toHaveLength(externalCallsBefore + 1);
    expect(warningState.items).toHaveLength(1);
    expect(warningState.items[0].task_id).toBe('task-warn');
    expect(toastState.items.find((t) => t.level === 'success')?.text).toBe(
      'Synced — 0 chunks scheduled, 2 Google events updated.',
    );
  });

  it('failure: error toast with sanitized message; button re-enabled', async () => {
    fake.googleAuthStatus.mockResolvedValue(CONNECTED_AUTH);
    fake.syncNow.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: HTTP 503',
    });

    const { getByRole } = await renderCalendarView(fake);
    await settle();

    await fireEvent.click(getByRole('button', { name: SYNC_NAME }));
    await tick();
    await settle();

    expect(toastState.items.find((t) => t.level === 'error')?.text).toBe(
      'Calendar sync error: HTTP 503',
    );
    const syncBtn = getByRole('button', { name: SYNC_NAME }) as HTMLButtonElement;
    expect(syncBtn.disabled).toBe(false);
  });
});

describe('CalendarView — reconnect hint', () => {
  // Default fake has googleAuthStatus → not_connected (see calendarApiFake).
  // Most tests in this suite need an external event so the hint is visible.
  const ext = externalEventFixture(EXT_FIXTURE_ARGS);

  beforeEach(() => {
    fake.getAgenda.mockResolvedValue([]);
    fake.listExternalEvents.mockResolvedValue([ext]);
  });

  it.each([
    { connected: false, hasEvents: true, shouldShow: true },
    { connected: false, hasEvents: false, shouldShow: false },
    { connected: true, hasEvents: true, shouldShow: false },
  ])(
    'hint visibility: connected=$connected, events=$hasEvents → visible=$shouldShow',
    async ({ connected, hasEvents, shouldShow }) => {
      if (connected) fake.googleAuthStatus.mockResolvedValue(CONNECTED_AUTH);
      if (!hasEvents) fake.listExternalEvents.mockResolvedValue([]);
      const { container } = await renderCalendarView(fake);
      await settle();
      expect(container.querySelector('.reconnect-hint') !== null).toBe(shouldShow);
    },
  );

  it('external event blocks carry the --disconnected class when not_connected', async () => {
    const { container } = await renderCalendarView(fake);
    await settle();
    const block = container.querySelector('.external-event') as HTMLElement | null;
    expect(block).not.toBeNull();
    expect(block!.classList.contains('external-event--disconnected')).toBe(true);
  });

  it('clicking "Open Settings" navigates to settings', async () => {
    window.location.hash = '#/calendar';
    const { container } = await renderCalendarView(fake);
    await settle();
    const btn = container.querySelector('.reconnect-hint__btn') as HTMLElement | null;
    expect(btn).not.toBeNull();
    await fireEvent.click(btn!);
    expect(window.location.hash).toBe('#/settings');
  });
});
