// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import MiniCalendar from './MiniCalendar.svelte';
import { toLocalDateString } from './dateTimePickerShared';
import { saveWeekStart } from '../../weekStartPref';
import { installLocalStorageStub } from '../../storageStubHooks';

installLocalStorageStub();

const FIXED_TODAY = new Date('2026-07-15T12:00:00Z');

describe('MiniCalendar', () => {
  it('opens on the selected month and marks the selection', () => {
    const { container, getByText } = render(MiniCalendar, {
      selected: '2026-07-01',
      onpick: vi.fn(),
      today: FIXED_TODAY,
    });

    expect(getByText('July 2026')).toBeTruthy();
    expect(container.querySelector('.calendar-day-btn--selected')?.getAttribute('data-date')).toBe(
      '2026-07-01',
    );
  });

  it('falls back to the current month, marking today, without a selection', () => {
    const FROZEN = new Date('2026-07-28T12:00:00Z');

    const { container, getByText } = render(MiniCalendar, {
      selected: null,
      onpick: vi.fn(),
      today: FROZEN,
    });

    const expectedMonthLabel = FROZEN.toLocaleDateString(undefined, {
      month: 'long',
      year: 'numeric',
    });
    expect(getByText(expectedMonthLabel)).toBeTruthy();
    expect(container.querySelector('.calendar-day-btn--selected')).toBeNull();
    expect(container.querySelector('.calendar-day-btn--today')?.getAttribute('data-date')).toBe(
      toLocalDateString(FROZEN),
    );
  });

  it('month navigation shifts the visible grid both ways', async () => {
    const { getByText, getByRole } = render(MiniCalendar, {
      selected: '2026-07-01',
      onpick: vi.fn(),
      today: FIXED_TODAY,
    });

    await fireEvent.click(getByRole('button', { name: 'Next month' }));
    expect(getByText('August 2026')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: 'Previous month' }));
    await fireEvent.click(getByRole('button', { name: 'Previous month' }));
    expect(getByText('June 2026')).toBeTruthy();
  });

  it('clicking a day reports its local date', async () => {
    const onpick = vi.fn();
    const { container } = render(MiniCalendar, {
      selected: '2026-07-01',
      onpick,
      today: FIXED_TODAY,
    });

    await fireEvent.click(container.querySelector('[data-date="2026-07-15"]')!);

    expect(onpick).toHaveBeenCalledWith('2026-07-15');
  });

  type WeekStartCase = {
    label: string;
    setup: () => void;
    weekStart: 'sun' | 'mon' | undefined;
    expected: string;
  };
  it.each<WeekStartCase>([
    {
      label: 'localStorage sun, no prop',
      setup: () => saveWeekStart('sun', window.localStorage),
      weekStart: undefined,
      expected: 'Su',
    },
    { label: 'prop sun, clean storage', setup: () => {}, weekStart: 'sun', expected: 'Su' },
    {
      label: 'prop mon overrides localStorage sun',
      setup: () => saveWeekStart('sun', window.localStorage),
      weekStart: 'mon',
      expected: 'Mo',
    },
  ])('$label -- weekday row starts with correct day', ({ setup, weekStart, expected }) => {
    setup();
    const { container } = render(MiniCalendar, {
      selected: null,
      onpick: vi.fn(),
      weekStart,
      today: FIXED_TODAY,
    });
    expect(container.querySelector('.weekday-row span')?.textContent).toBe(expected);
  });

  it.each([
    { label: 'no size prop (default sm)', size: undefined as 'md' | undefined, expectMd: false },
    { label: "size='md'", size: 'md' as 'md' | undefined, expectMd: true },
  ])('$label -> mini-calendar--md class: $expectMd', ({ size, expectMd }) => {
    const { container } = render(MiniCalendar, {
      selected: null,
      onpick: vi.fn(),
      size,
      today: FIXED_TODAY,
    });
    expect(container.querySelector('.mini-calendar')?.classList.contains('mini-calendar--md')).toBe(
      expectMd,
    );
  });
});
