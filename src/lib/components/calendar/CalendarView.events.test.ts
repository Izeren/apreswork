// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import {
  CALENDAR_TEST_NOW,
  installCalendarHooks,
  switchToDayMode,
  externalEventFixture,
  dragEmptySlot,
  calendarApiFake,
} from './testFixtures';
import { toastState } from '../../stores/toast.svelte';
import CalendarView from './CalendarView.svelte';

installCalendarHooks();

let fake: ReturnType<typeof calendarApiFake>;

const PRIMARY = { id: 'cal-primary', title: 'you@example.com', primary: true };

beforeEach(() => {
  fake = calendarApiFake();
  toastState.items = [];
});

/** Flush the two-hop mount chain (auth status → list calendars) plus renders. */
async function flush(): Promise<void> {
  await tick();
  await Promise.resolve();
  await Promise.resolve();
  await tick();
}

/** Point the mocks at a connected account whose primary calendar is `cal-primary`. */
function connectPrimary(): void {
  fake.googleAuthStatus.mockResolvedValue({ type: 'connected', email: 'a@b.c' });
  fake.googleListCalendars.mockResolvedValue([PRIMARY]);
}

/** Render CalendarView with the per-test fake and flush the mount chain. */
async function renderCalendar() {
  const utils = render(CalendarView, { apiClient: fake, getNow: () => CALENDAR_TEST_NOW });
  await flush();
  return utils;
}

function mockPrimaryEvent(overrides: Parameters<typeof externalEventFixture>[0] = {}) {
  connectPrimary();
  fake.listExternalEvents.mockResolvedValue([
    externalEventFixture({ calendar_id: 'cal-primary', ...overrides }),
  ]);
}

async function openEventEditor(container: HTMLElement) {
  await fireEvent.click(container.querySelector('.external-event--interactive') as HTMLElement);
  await tick();
}

async function openCreateChooser(
  container: HTMLElement,
  getByRole: Awaited<ReturnType<typeof renderCalendar>>['getByRole'],
) {
  await switchToDayMode(getByRole);
  await dragEmptySlot(container);
}

describe('CalendarView — external event editing', () => {
  it('opens the edit dialog prefilled when a primary-calendar event is clicked', async () => {
    mockPrimaryEvent({ title: 'Team meeting' });

    const { container, getByText, getByLabelText } = await renderCalendar();

    const block = container.querySelector('.external-event--interactive') as HTMLElement | null;
    expect(block).toBeTruthy();
    await fireEvent.click(block!);
    await tick();

    expect(getByText('Edit event')).toBeTruthy();
    expect((getByLabelText(/Title/) as HTMLInputElement).value).toBe('Team meeting');
  });

  it('leaves events on non-primary calendars read-only', async () => {
    connectPrimary();
    fake.listExternalEvents.mockResolvedValue([
      externalEventFixture({ calendar_id: 'someone-elses-cal' }),
    ]);

    const { container } = await renderCalendar();

    expect(container.querySelector('.external-event')).toBeTruthy();
    expect(container.querySelector('.external-event--interactive')).toBeNull();
  });

  it('submits updateUserEvent and refetches both datasets on save', async () => {
    mockPrimaryEvent({ title: 'Team meeting' });

    const { container, getByLabelText } = await renderCalendar();

    await openEventEditor(container);
    await fireEvent.input(getByLabelText(/Title/), { target: { value: 'Team sync' } });

    const agendaBefore = fake.getAgenda.mock.calls.length;
    const externalsBefore = fake.listExternalEvents.mock.calls.length;

    await fireEvent.submit(container.querySelector('.event-form')!);
    await flush();

    expect(fake.updateUserEvent).toHaveBeenCalledWith(
      'cal-primary',
      'provider-event-1',
      expect.objectContaining({ title: 'Team sync', all_day: false }),
    );
    expect(fake.getAgenda.mock.calls).toHaveLength(agendaBefore + 1);
    expect(fake.listExternalEvents.mock.calls).toHaveLength(externalsBefore + 1);
    expect(toastState.items).toEqual([
      expect.objectContaining({ level: 'success', text: 'Event updated' }),
    ]);
  });

  it('confirms then deletes via deleteUserEvent and refetches', async () => {
    mockPrimaryEvent();

    const { container, getByRole } = await renderCalendar();

    await openEventEditor(container);

    await fireEvent.click(getByRole('button', { name: 'Delete' }));
    await tick();
    expect(fake.deleteUserEvent).not.toHaveBeenCalled();

    await fireEvent.click(getByRole('button', { name: /confirm delete/i }));
    await flush();

    expect(fake.deleteUserEvent).toHaveBeenCalledWith('cal-primary', 'provider-event-1');
    expect(toastState.items).toEqual([
      expect.objectContaining({ level: 'success', text: 'Event deleted' }),
    ]);
  });

  async function openEditorForErrorTest() {
    mockPrimaryEvent();
    const utils = await renderCalendar();
    await openEventEditor(utils.container);
    return utils;
  }

  type TestUtils = Awaited<ReturnType<typeof renderCalendar>>;

  it.each([
    {
      label: 'write (HTTP 503)',
      reject: (): void => {
        fake.updateUserEvent.mockRejectedValue({
          error: 'calendar_sync',
          message: 'Calendar sync error: HTTP 503',
        });
      },
      code: 'HTTP 503',
      act: async (utils: TestUtils) => {
        await fireEvent.submit(utils.container.querySelector('.event-form')!);
      },
    },
    {
      label: 'delete (HTTP 500)',
      reject: (): void => {
        fake.deleteUserEvent.mockRejectedValue({
          error: 'calendar_sync',
          message: 'Calendar sync error: HTTP 500',
        });
      },
      code: 'HTTP 500',
      act: async (utils: TestUtils) => {
        await fireEvent.click(utils.getByRole('button', { name: 'Delete' }));
        await tick();
        await fireEvent.click(utils.getByRole('button', { name: /confirm delete/i }));
      },
    },
  ])(
    'surfaces a sanitized $label error and keeps the dialog open',
    async ({ reject, code, act }) => {
      const utils = await openEditorForErrorTest();
      reject();
      await act(utils);
      await flush();
      expect(utils.getByRole('alert').textContent).toContain(code);
      expect(utils.getByText('Edit event')).toBeTruthy();
    },
  );
});

describe('CalendarView — empty-slot create chooser', () => {
  it('offers the chooser when connected with a primary calendar', async () => {
    connectPrimary();

    const { container, getByRole, getByText } = await renderCalendar();
    await openCreateChooser(container, getByRole);

    expect(getByText('Add to calendar')).toBeTruthy();
  });

  it('choosing Event opens the create dialog and createUserEvent runs on submit', async () => {
    connectPrimary();

    const { container, getByRole, getByText, getByLabelText } = await renderCalendar();
    await openCreateChooser(container, getByRole);

    await fireEvent.click(getByRole('button', { name: 'Event' }));
    await tick();
    expect(getByText('New event')).toBeTruthy();

    await fireEvent.input(getByLabelText(/Title/), { target: { value: 'Dinner' } });
    await fireEvent.submit(container.querySelector('.event-form')!);
    await flush();

    expect(fake.createUserEvent).toHaveBeenCalledTimes(1);
    const [calId, payload] = fake.createUserEvent.mock.calls[0];
    expect(calId).toBe('cal-primary');
    expect(payload).toEqual(expect.objectContaining({ title: 'Dinner', all_day: false }));
    expect(toastState.items).toEqual([
      expect.objectContaining({ level: 'success', text: 'Event created' }),
    ]);
  });

  it('surfaces a create failure and keeps the create dialog open', async () => {
    connectPrimary();
    fake.createUserEvent.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: HTTP 500',
    });

    const { container, getByRole, getByText, getByLabelText } = await renderCalendar();
    await openCreateChooser(container, getByRole);

    await fireEvent.click(getByRole('button', { name: 'Event' }));
    await tick();
    await fireEvent.input(getByLabelText(/Title/), { target: { value: 'Dinner' } });
    await fireEvent.submit(container.querySelector('.event-form')!);
    await flush();

    expect(getByRole('alert').textContent).toContain('HTTP 500');
    expect(getByText('New event')).toBeTruthy();
  });

  it('choosing Task opens the task form without creating an event', async () => {
    connectPrimary();

    const { container, getByRole, getByText } = await renderCalendar();
    await openCreateChooser(container, getByRole);

    await fireEvent.click(getByRole('button', { name: 'Task' }));
    await tick();

    expect(getByText('Create Task')).toBeTruthy();
    expect(fake.createUserEvent).not.toHaveBeenCalled();
  });

  it('goes straight to the task form when connected without a primary calendar', async () => {
    fake.googleAuthStatus.mockResolvedValue({ type: 'connected', email: 'a@b.c' });
    fake.googleListCalendars.mockResolvedValue([{ id: 'shared', title: 'Shared', primary: false }]);

    const { container, getByRole, getByPlaceholderText, queryByText } = await renderCalendar();
    await openCreateChooser(container, getByRole);

    expect(queryByText('Add to calendar')).toBeNull();
    expect(getByPlaceholderText('Task title')).toBeTruthy();
  });
});
