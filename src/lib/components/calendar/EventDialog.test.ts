// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom

import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import EventDialog from './EventDialog.svelte';
import type { EventDialogInitial } from './EventDialog.svelte';
import { isoToLocalDate } from '../shared/dateTimePickerShared';

const TIMED_START = '2026-07-20T09:00:00.000Z';
const TIMED_END = '2026-07-20T10:00:00.000Z';

function timedInitial(overrides: Partial<EventDialogInitial> = {}): EventDialogInitial {
  return {
    title: '',
    description: null,
    start: TIMED_START,
    end: TIMED_END,
    all_day: false,
    ...overrides,
  };
}

function setup(overrides: Record<string, unknown> = {}) {
  const onsubmit = vi.fn();
  const oncancel = vi.fn();
  const props = {
    open: true,
    mode: 'create' as const,
    initial: timedInitial(),
    busy: false,
    error: null,
    onsubmit,
    ondelete: null as (() => void) | null,
    oncancel,
    ...overrides,
  };
  const result = render(EventDialog, { props });
  return { ...result, onsubmit, oncancel };
}

/** Set a labelled text/date input's value through the input event Svelte binds to. */
async function setInput(input: HTMLElement, value: string): Promise<void> {
  await fireEvent.input(input, { target: { value } });
}

function submit(container: HTMLElement): Promise<boolean> {
  const form = container.querySelector('form');
  if (!form) throw new Error('form not found');
  return fireEvent.submit(form);
}

/** setup() in edit mode for an all-day event starting 2026-07-20, ending at `endDateTime` (UTC ISO). */
function setupEditAllDay(endDateTime: string) {
  const startIso = '2026-07-20T00:00:00.000Z';
  const endIso = endDateTime;
  return setup({
    mode: 'edit',
    ondelete: vi.fn(),
    initial: { title: 'Retreat', description: null, start: startIso, end: endIso, all_day: true },
  });
}

describe('EventDialog — rendering & mode', () => {
  it('renders nothing when closed', () => {
    const { queryByRole } = setup({ open: false });
    expect(queryByRole('dialog')).toBeNull();
  });

  it('titles the dialog "New event" and hides Delete in create mode', () => {
    const { getByText, queryByRole } = setup({ mode: 'create' });
    expect(getByText('New event')).toBeTruthy();
    expect(queryByRole('button', { name: 'Delete' })).toBeNull();
  });

  it('titles the dialog "Edit event" and shows Delete in edit mode', () => {
    const { getByText, getByRole } = setup({
      mode: 'edit',
      initial: timedInitial({ title: 'Dentist' }),
      ondelete: vi.fn(),
    });
    expect(getByText('Edit event')).toBeTruthy();
    expect(getByRole('button', { name: 'Delete' })).toBeTruthy();
  });
});

describe('EventDialog — timed submit', () => {
  it('emits a timed payload passing the seeded instants through unchanged', async () => {
    const { getByLabelText, container, onsubmit } = setup();
    await setInput(getByLabelText(/Title/), 'Dentist');
    await submit(container);
    expect(onsubmit).toHaveBeenCalledTimes(1);
    expect(onsubmit).toHaveBeenCalledWith({
      title: 'Dentist',
      description: null,
      start: TIMED_START,
      end: TIMED_END,
      all_day: false,
    });
  });

  it('trims both the title and the description', async () => {
    const { getByLabelText, container, onsubmit } = setup();
    await setInput(getByLabelText(/Title/), '  Gym  ');
    await setInput(getByLabelText('Description'), '  leg day  ');
    await submit(container);
    expect(onsubmit).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Gym', description: 'leg day' }),
    );
  });
});

type SetupResult = ReturnType<typeof setup>;

describe('EventDialog — validation', () => {
  it.each([
    {
      label: 'empty title',
      setupOpts: {},
      beforeSubmit: async () => {},
      alertPattern: /title/i,
    },
    {
      label: 'timed end not after start',
      setupOpts: { initial: timedInitial({ title: 'x', end: TIMED_START }) },
      beforeSubmit: async (r: SetupResult) => {
        await setInput(r.getByLabelText(/Title/), 'Meeting');
      },
      alertPattern: /end/i,
    },
    {
      label: 'all-day end before start',
      setupOpts: {},
      beforeSubmit: async (r: SetupResult) => {
        await setInput(r.getByLabelText(/Title/), 'Trip');
        await fireEvent.click(r.getByLabelText('All day'));
        await setInput(r.getByLabelText('Start date'), '2026-07-20');
        await setInput(r.getByLabelText('End date'), '2026-07-19');
      },
      alertPattern: /end date/i,
    },
  ])('rejects $label and shows alert', async ({ setupOpts, beforeSubmit, alertPattern }) => {
    const result = setup(setupOpts);
    await beforeSubmit(result);
    await submit(result.container);
    expect(result.onsubmit).not.toHaveBeenCalled();
    expect(result.getByRole('alert').textContent).toMatch(alertPattern);
  });
});

describe('EventDialog — all-day submit', () => {
  it('emits an all-day payload with a local-midnight start and exclusive end', async () => {
    const { getByLabelText, container, onsubmit } = setup();
    await setInput(getByLabelText(/Title/), 'Conference');
    await fireEvent.click(getByLabelText('All day'));
    await setInput(getByLabelText('Start date'), '2026-07-20');
    await setInput(getByLabelText('End date'), '2026-07-22');
    await submit(container);
    expect(onsubmit).toHaveBeenCalledTimes(1);
    const payload = onsubmit.mock.calls[0][0];
    expect(payload.all_day).toBe(true);
    expect(payload.title).toBe('Conference');
    expect(isoToLocalDate(payload.start)).toBe('2026-07-20');
    // End is exclusive: the day after the inclusive last day (2026-07-22).
    expect(isoToLocalDate(payload.end)).toBe('2026-07-23');
  });

  it('prefills inclusive dates when editing an existing all-day event', async () => {
    // Mirror convention: end_time is Local midnight of the day AFTER the last day.
    const { getByLabelText, container, onsubmit } = setupEditAllDay('2026-07-23T00:00:00.000Z');
    // Inclusive last day is 2026-07-22 (one before the exclusive end).
    expect((getByLabelText('Start date') as HTMLInputElement).value).toBe('2026-07-20');
    expect((getByLabelText('End date') as HTMLInputElement).value).toBe('2026-07-22');
    await submit(container);
    const payload = onsubmit.mock.calls[0][0];
    expect(isoToLocalDate(payload.start)).toBe('2026-07-20');
    expect(isoToLocalDate(payload.end)).toBe('2026-07-23');
  });

  it('switches an edited all-day event back to a timed payload', async () => {
    const { getByLabelText, container, onsubmit } = setupEditAllDay('2026-07-21T00:00:00.000Z');
    await fireEvent.click(getByLabelText('All day'));
    await submit(container);
    expect(onsubmit.mock.calls[0][0].all_day).toBe(false);
  });
});

describe('EventDialog — edit, delete, cancel', () => {
  it('emits the edited title in edit mode', async () => {
    const { getByLabelText, container, onsubmit } = setup({
      mode: 'edit',
      ondelete: vi.fn(),
      initial: timedInitial({ title: 'Old' }),
    });
    await setInput(getByLabelText(/Title/), 'New');
    await submit(container);
    expect(onsubmit).toHaveBeenCalledWith(expect.objectContaining({ title: 'New' }));
  });

  it('requires a second click to confirm a delete', async () => {
    const ondelete = vi.fn();
    const { getByRole } = setup({
      mode: 'edit',
      ondelete,
      initial: timedInitial({ title: 'Old' }),
    });
    await fireEvent.click(getByRole('button', { name: 'Delete' }));
    expect(ondelete).not.toHaveBeenCalled();
    const confirmBtn = getByRole('button', { name: /confirm delete/i });
    await fireEvent.click(confirmBtn);
    expect(ondelete).toHaveBeenCalledTimes(1);
  });

  it('calls oncancel from the Cancel button', async () => {
    const { getByRole, oncancel } = setup();
    await fireEvent.click(getByRole('button', { name: 'Cancel' }));
    expect(oncancel).toHaveBeenCalledTimes(1);
  });
});

describe('EventDialog — busy & error', () => {
  it('disables the primary action and relabels it while busy', () => {
    const { getByRole } = setup({ busy: true });
    const save = getByRole('button', { name: /saving/i }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
  });

  it('surfaces a parent-supplied error', () => {
    const { getByRole } = setup({ error: 'Google rejected the change' });
    expect(getByRole('alert').textContent).toContain('Google rejected the change');
  });
});
