// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Cadence, Weekday, Window } from '../../types';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const OFFSET: Record<Weekday, number> = {
  Mon: 0,
  Tue: 1,
  Wed: 2,
  Thu: 3,
  Fri: 4,
  Sat: 5,
  Sun: 6,
};

function singletons(days: Weekday[]): Window[] {
  return days.map((d) => ({ start: OFFSET[d], end: OFFSET[d] }));
}

function weekly(days: Weekday[], interval = 1): Cadence {
  return { period: 'Weekly', interval, windows: singletons(days) };
}

function weeklyWindows(windows: Window[], interval = 1): Cadence {
  return { period: 'Weekly', interval, windows };
}

function monthly(dayOfMonth: number, interval = 1): Cadence {
  return { period: 'Monthly', interval, windows: [{ start: dayOfMonth - 1, end: dayOfMonth - 1 }] };
}

function lastEmit(onchange: ReturnType<typeof vi.fn>): Cadence {
  return onchange.mock.calls[onchange.mock.calls.length - 1][0] as Cadence;
}

async function importComponent() {
  const mod = await import('./RecurringSection.svelte');
  return mod.default;
}

async function renderSection(cadence: Cadence, onchange: (c: Cadence) => void = vi.fn()) {
  const Comp = await importComponent();
  const utils = render(Comp, { cadence, onchange });
  await tick();
  return utils;
}

async function typeInto(
  getByLabelText: (matcher: RegExp) => HTMLElement,
  label: RegExp,
  value: string,
): Promise<HTMLInputElement> {
  const input = getByLabelText(label) as HTMLInputElement;
  input.value = value;
  await fireEvent.input(input);
  await tick();
  return input;
}

async function clickDay(
  getByRole: (role: string, opts: { name: string }) => HTMLElement,
  day: string,
): Promise<void> {
  await fireEvent.click(getByRole('button', { name: day }));
  await tick();
}

describe('RecurringSection — weekly initial render', () => {
  it('renders Weekly radio selected when cadence is Weekly', async () => {
    const { container } = await renderSection(weekly(['Mon']));
    const radios = container.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    const weeklyRadio = [...radios].find((r) => r.value === 'Weekly');
    expect(weeklyRadio?.checked).toBe(true);
  });

  it('renders day picker buttons for all 7 days', async () => {
    const { container } = await renderSection(weekly(['Mon']));
    expect(container.querySelectorAll('.day-btn')).toHaveLength(7);
  });

  it('highlights selected days from the initial windows', async () => {
    const { container } = await renderSection(weekly(['Mon', 'Wed', 'Fri']));
    const selected = container.querySelectorAll('.day-btn--selected');
    const labels = [...selected].map((b) => b.textContent?.trim());
    expect(labels).toEqual(expect.arrayContaining(['Mon', 'Wed', 'Fri']));
    expect(selected).toHaveLength(3);
  });

  it('highlights every day of a multi-day window span', async () => {
    const { container } = await renderSection(weeklyWindows([{ start: 5, end: 6 }]));
    const labels = [...container.querySelectorAll('.day-btn--selected')].map((b) =>
      b.textContent?.trim(),
    );
    expect(labels).toEqual(['Sat', 'Sun']);
  });

  it('does not show day-of-month input in weekly mode', async () => {
    const { queryByLabelText } = await renderSection(weekly(['Mon']));
    expect(queryByLabelText(/day of month/i)).toBeNull();
  });
});

describe('RecurringSection — monthly initial render', () => {
  it('renders Monthly radio selected when cadence is Monthly', async () => {
    const { container } = await renderSection(monthly(15));
    const radios = container.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    const monthlyRadio = [...radios].find((r) => r.value === 'Monthly');
    expect(monthlyRadio?.checked).toBe(true);
  });

  it('renders day-of-month input showing window start + 1', async () => {
    const { getByLabelText } = await renderSection(monthly(20));
    const input = getByLabelText(/day of month/i) as HTMLInputElement;
    expect(input.value).toBe('20');
  });

  it('caps the day-of-month input at 28', async () => {
    const { getByLabelText } = await renderSection(monthly(15));
    const input = getByLabelText(/day of month/i) as HTMLInputElement;
    expect(input.getAttribute('max')).toBe('28');
  });

  it('does not show day picker in monthly mode', async () => {
    const { container } = await renderSection(monthly(15));
    expect(container.querySelector('.day-picker')).toBeNull();
  });
});

async function switchTo(container: HTMLElement, value: 'Weekly' | 'Monthly') {
  const radios = container.querySelectorAll<HTMLInputElement>('input[type="radio"]');
  const radio = [...radios].find((r) => r.value === value)!;
  await fireEvent.change(radio);
  await tick();
}

describe('RecurringSection — switching cadence type', () => {
  it('switching Weekly -> Monthly emits a Monthly cadence on day 1', async () => {
    const onchange = vi.fn();
    const { container } = await renderSection(weekly(['Mon']), onchange);

    await switchTo(container, 'Monthly');

    const arg = lastEmit(onchange);
    expect(arg.period).toBe('Monthly');
    expect(arg.windows).toEqual([{ start: 0, end: 0 }]);
  });

  it('switching Monthly -> Weekly emits a Weekly cadence defaulting to Mon', async () => {
    const onchange = vi.fn();
    const { container } = await renderSection(monthly(15), onchange);

    await switchTo(container, 'Weekly');

    const arg = lastEmit(onchange);
    expect(arg.period).toBe('Weekly');
    expect(arg.windows).toEqual([{ start: 0, end: 0 }]);
  });

  it('shows day picker after switching to Weekly', async () => {
    const { container } = await renderSection(monthly(15));
    await switchTo(container, 'Weekly');
    expect(container.querySelector('.day-picker')).toBeTruthy();
  });

  it('shows day-of-month input after switching to Monthly', async () => {
    const { container, queryByLabelText } = await renderSection(weekly(['Mon']));
    await switchTo(container, 'Monthly');
    expect(queryByLabelText(/day of month/i)).toBeTruthy();
  });
});

describe('RecurringSection — day toggle', () => {
  it('clicking an unselected day adds a singleton window and calls onchange', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weekly(['Mon']), onchange);

    await clickDay(getByRole, 'Tue');

    expect(onchange).toHaveBeenCalled();
    expect(lastEmit(onchange).windows).toEqual([
      { start: 0, end: 0 },
      { start: 1, end: 1 },
    ]);
  });

  it('clicking a selected day removes its window', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weekly(['Mon', 'Wed']), onchange);

    await clickDay(getByRole, 'Mon');

    expect(lastEmit(onchange).windows).toEqual([{ start: 2, end: 2 }]);
  });

  it('clicking any covered day removes its whole multi-day window', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(
      weeklyWindows([
        { start: 0, end: 2 },
        { start: 5, end: 5 },
      ]),
      onchange,
    );

    // Tue (offset 1) is interior to the Mon–Wed window.
    await clickDay(getByRole, 'Tue');

    expect(lastEmit(onchange).windows).toEqual([{ start: 5, end: 5 }]);
  });

  it('emits windows in ascending order regardless of click order', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weekly(['Wed']), onchange);

    await clickDay(getByRole, 'Mon');

    expect(lastEmit(onchange).windows).toEqual([
      { start: 0, end: 0 },
      { start: 2, end: 2 },
    ]);
  });

  it('selected day button has aria-pressed=true', async () => {
    const { getByRole } = await renderSection(weekly(['Fri']));
    expect(getByRole('button', { name: 'Fri' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('unselected day button has aria-pressed=false', async () => {
    const { getByRole } = await renderSection(weekly(['Mon']));
    expect(getByRole('button', { name: 'Sat' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('deselecting the last day shows an error and does not call onchange', async () => {
    const onchange = vi.fn();
    const { getByRole, getByText } = await renderSection(weekly(['Mon']), onchange);

    onchange.mockClear();
    await clickDay(getByRole, 'Mon');

    expect(getByText(/select at least one day/i)).toBeTruthy();
    expect(onchange).not.toHaveBeenCalled();
  });
});

describe('RecurringSection — drag to select a window', () => {
  it('dragging across two days creates one spanning window', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weeklyWindows([]), onchange);

    const sat = getByRole('button', { name: 'Sat' });
    const sun = getByRole('button', { name: 'Sun' });
    await fireEvent.pointerDown(sat);
    await fireEvent.pointerEnter(sun);
    await fireEvent.pointerUp(sun);
    await tick();

    expect(lastEmit(onchange).windows).toEqual([{ start: 5, end: 6 }]);
  });

  it('dragging across three days creates one window spanning all of them', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weeklyWindows([]), onchange);

    await fireEvent.pointerDown(getByRole('button', { name: 'Wed' }));
    await fireEvent.pointerEnter(getByRole('button', { name: 'Thu' }));
    await fireEvent.pointerEnter(getByRole('button', { name: 'Fri' }));
    await fireEvent.pointerUp(getByRole('button', { name: 'Fri' }));
    await tick();

    expect(lastEmit(onchange).windows).toEqual([{ start: 2, end: 4 }]);
  });

  it('a drag absorbs windows it overlaps into a single union window', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(
      weeklyWindows([
        { start: 1, end: 1 },
        { start: 2, end: 2 },
      ]),
      onchange,
    );

    await fireEvent.pointerDown(getByRole('button', { name: 'Mon' }));
    await fireEvent.pointerEnter(getByRole('button', { name: 'Tue' }));
    await fireEvent.pointerEnter(getByRole('button', { name: 'Wed' }));
    await fireEvent.pointerUp(getByRole('button', { name: 'Wed' }));
    await tick();

    expect(lastEmit(onchange).windows).toEqual([{ start: 0, end: 2 }]);
  });

  it('a drag that returns to its start day toggles a single-day window via click', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weeklyWindows([]), onchange);

    const wed = getByRole('button', { name: 'Wed' });
    // pointer down + up on the same day with no movement → treated as a click.
    await fireEvent.pointerDown(wed);
    await fireEvent.pointerUp(wed);
    await fireEvent.click(wed);
    await tick();

    expect(lastEmit(onchange).windows).toEqual([{ start: 2, end: 2 }]);
  });
});

describe('RecurringSection — window markers and hint', () => {
  it('renders a single-day marker for a singleton window', async () => {
    const { container } = await renderSection(weekly(['Mon']));
    expect(container.querySelectorAll('.marker--single')).toHaveLength(1);
    expect(container.querySelectorAll('.marker--start')).toHaveLength(0);
  });

  it('renders start / mid / end markers across a multi-day window', async () => {
    const { container } = await renderSection(weeklyWindows([{ start: 0, end: 2 }]));
    expect(container.querySelectorAll('.marker--start')).toHaveLength(1);
    expect(container.querySelectorAll('.marker--mid')).toHaveLength(1);
    expect(container.querySelectorAll('.marker--end')).toHaveLength(1);
  });

  it.each([
    { days: ['Mon'] as Weekday[], hint: '1 instance per week' },
    { days: ['Mon', 'Wed'] as Weekday[], hint: '2 instances per week' },
  ])('shows "$hint" for $days.length window(s)', async ({ days, hint }) => {
    const { getByText } = await renderSection(weekly(days));
    expect(getByText(hint)).toBeTruthy();
  });
});

describe('RecurringSection — interval', () => {
  it('renders the current interval', async () => {
    const { getByLabelText } = await renderSection(weekly(['Mon'], 3));
    expect((getByLabelText(/repeat every/i) as HTMLInputElement).value).toBe('3');
  });

  it('changing the interval emits the new interval while keeping windows', async () => {
    const onchange = vi.fn();
    const { getByLabelText } = await renderSection(weekly(['Mon', 'Wed']), onchange);

    await typeInto(getByLabelText, /repeat every/i, '2');

    const arg = lastEmit(onchange);
    expect(arg.interval).toBe(2);
    expect(arg.windows).toEqual([
      { start: 0, end: 0 },
      { start: 2, end: 2 },
    ]);
  });

  it('changing the interval in monthly mode emits the new interval', async () => {
    const onchange = vi.fn();
    const { getByLabelText } = await renderSection(monthly(15), onchange);

    await typeInto(getByLabelText, /repeat every/i, '4');

    const arg = lastEmit(onchange);
    expect(arg.period).toBe('Monthly');
    expect(arg.interval).toBe(4);
    expect(arg.windows).toEqual([{ start: 14, end: 14 }]);
  });

  it.each([
    { value: '0', label: 'zero' },
    { value: '', label: 'empty' },
    { value: '1.5', label: 'fractional' },
  ])('an invalid interval ($label) shows an error and does not emit', async ({ value }) => {
    const onchange = vi.fn();
    const { getByLabelText, getByText } = await renderSection(weekly(['Mon']), onchange);

    onchange.mockClear();
    await typeInto(getByLabelText, /repeat every/i, value);

    expect(getByText(/whole number/i)).toBeTruthy();
    expect(onchange).not.toHaveBeenCalled();
  });

  it('preserves the weekly interval when toggling a day', async () => {
    const onchange = vi.fn();
    const { getByRole } = await renderSection(weekly(['Mon'], 2), onchange);

    await clickDay(getByRole, 'Tue');

    expect(lastEmit(onchange).interval).toBe(2);
  });

  it('preserves the monthly interval when changing the day', async () => {
    const onchange = vi.fn();
    const { getByLabelText } = await renderSection(monthly(15, 3), onchange);

    await typeInto(getByLabelText, /day of month/i, '10');

    expect(lastEmit(onchange).interval).toBe(3);
  });
});

describe('RecurringSection — day of month', () => {
  it('a valid day-of-month emits a singleton window at value - 1', async () => {
    const onchange = vi.fn();
    const { getByLabelText } = await renderSection(monthly(15), onchange);

    await typeInto(getByLabelText, /day of month/i, '28');

    expect(lastEmit(onchange).windows).toEqual([{ start: 27, end: 27 }]);
  });

  it.each([
    { value: '0', label: 'below 1' },
    { value: '29', label: 'above 28' },
  ])('day-of-month $label shows an error and does not call onchange', async ({ value }) => {
    const onchange = vi.fn();
    const { getByLabelText, getByText } = await renderSection(monthly(15), onchange);

    onchange.mockClear();
    await typeInto(getByLabelText, /day of month/i, value);

    expect(getByText(/must be between 1 and 28/i)).toBeTruthy();
    expect(onchange).not.toHaveBeenCalled();
  });

  it.each([
    { value: 1, expected: 0 },
    { value: 28, expected: 27 },
    { value: 15, expected: 14 },
  ])('day-of-month=$value is valid (window start $expected)', async ({ value, expected }) => {
    const onchange = vi.fn();
    const { getByLabelText } = await renderSection(monthly(15), onchange);

    await typeInto(getByLabelText, /day of month/i, String(value));

    expect(lastEmit(onchange).windows).toEqual([{ start: expected, end: expected }]);
  });

  it('empty day-of-month shows an error and does not call onchange', async () => {
    const onchange = vi.fn();
    const { getByLabelText, getByText } = await renderSection(monthly(15), onchange);

    onchange.mockClear();
    await typeInto(getByLabelText, /day of month/i, '');

    expect(getByText(/enter a number/i)).toBeTruthy();
    expect(onchange).not.toHaveBeenCalled();
  });

  it('an out-of-range day-of-month does not block a later interval change', async () => {
    const onchange = vi.fn();
    const { getByLabelText } = await renderSection(monthly(15), onchange);

    // Out-of-range day: error shown, no emit, last valid day (15) retained.
    await typeInto(getByLabelText, /day of month/i, '50');

    onchange.mockClear();
    // Changing the interval must still emit, using the retained valid day.
    await typeInto(getByLabelText, /repeat every/i, '3');

    expect(onchange).toHaveBeenCalledTimes(1);
    const arg = lastEmit(onchange);
    expect(arg.interval).toBe(3);
    expect(arg.windows).toEqual([{ start: 14, end: 14 }]); // day 15 retained
  });
});

describe('RecurringSection — all days toggleable', () => {
  const ALL_DAYS: Weekday[] = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  it.each(ALL_DAYS.map((d) => ({ day: d })))(
    'day $day can be toggled on from unselected',
    async ({ day }) => {
      const onchange = vi.fn();
      const initialDays: Weekday[] = day === 'Mon' ? ['Tue'] : ['Mon'];
      const { getByRole } = await renderSection(weekly(initialDays), onchange);

      await clickDay(getByRole, day);

      expect(lastEmit(onchange).windows).toContainEqual({ start: OFFSET[day], end: OFFSET[day] });
    },
  );
});

describe('RecurringSection — roundtrip: empty days → Monthly → Weekly', () => {
  it('switching back to Weekly after days were emptied emits a non-empty cadence', async () => {
    const onchange = vi.fn();
    const { container, getByRole } = await renderSection(weekly(['Mon']), onchange);

    // Deselect Mon → empty (error shown, onchange not called).
    await clickDay(getByRole, 'Mon');

    await switchTo(container, 'Monthly');

    onchange.mockClear();
    await switchTo(container, 'Weekly');

    expect(onchange).toHaveBeenCalledTimes(1);
    const arg = lastEmit(onchange);
    expect(arg.period).toBe('Weekly');
    expect(arg.windows).toEqual([{ start: 0, end: 0 }]); // recovers Mon
  });
});

describe('RecurringSection — accessibility', () => {
  it('day-of-month input has aria-invalid=true when an error is shown', async () => {
    const { getByLabelText } = await renderSection(monthly(15));

    const input = await typeInto(getByLabelText, /day of month/i, '50');

    expect(input.getAttribute('aria-invalid')).toBe('true');
  });

  it('day picker has role=group with an accessible label', async () => {
    const { container } = await renderSection(weekly(['Mon']));

    const group = container.querySelector('[role="group"]');
    expect(group).toBeTruthy();
    expect(group?.getAttribute('aria-label')).toBeTruthy();
  });
});
