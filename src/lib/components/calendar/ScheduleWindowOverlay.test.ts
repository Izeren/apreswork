// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/svelte';
import type { ScheduleWindow, Weekday } from '../../types';

afterEach(() => {
  cleanup();
});

function makeWindow(
  id: string,
  day_of_week: Weekday,
  start_time: string,
  end_time: string,
): ScheduleWindow {
  return { id, schedule_id: 'sched-1', day_of_week, start_time, end_time };
}

/** Build a local-time Date for a given weekday in 2026-03-23..29 (Mon=23 … Sun=29). */
function weekdayDate(weekday: Weekday): Date {
  const offsets: Record<Weekday, number> = {
    Mon: 0,
    Tue: 1,
    Wed: 2,
    Thu: 3,
    Fri: 4,
    Sat: 5,
    Sun: 6,
  };
  // 2026-03-23 is a Monday
  return new Date(2026, 2, 23 + offsets[weekday]);
}

async function importOverlay() {
  const mod = await import('./ScheduleWindowOverlay.svelte');
  return mod.default;
}

describe('ScheduleWindowOverlay — band rendering', () => {
  it.each([
    {
      label: 'matching day',
      windows: [makeWindow('w1', 'Mon', '09:00:00', '11:00:00')],
      date: weekdayDate('Mon'),
      expected: 1,
    },
    {
      label: 'non-matching day',
      windows: [makeWindow('w1', 'Tue', '09:00:00', '11:00:00')],
      date: weekdayDate('Mon'),
      expected: 0,
    },
    {
      label: 'empty windows',
      windows: [] as ScheduleWindow[],
      date: weekdayDate('Mon'),
      expected: 0,
    },
    {
      label: 'multiple matching windows',
      windows: [
        makeWindow('w1', 'Fri', '07:00:00', '09:00:00'),
        makeWindow('w2', 'Fri', '18:00:00', '23:00:00'),
      ],
      date: weekdayDate('Fri'),
      expected: 2,
    },
  ])('$label — band count', async ({ windows, date, expected }) => {
    const Overlay = await importOverlay();
    const { container } = render(Overlay, { windows, date });
    expect(container.querySelectorAll('.schedule-window-band')).toHaveLength(expected);
  });
});

describe('ScheduleWindowOverlay — weekday matching', () => {
  const weekdays: Weekday[] = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  it.each(weekdays)('renders band for %s when date is that weekday', async (day) => {
    const Overlay = await importOverlay();
    const windows = [makeWindow(`w-${day}`, day, '10:00:00', '12:00:00')];
    const { container } = render(Overlay, { windows, date: weekdayDate(day) });
    expect(container.querySelectorAll('.schedule-window-band')).toHaveLength(1);
  });

  it.each(weekdays)(
    'renders no band for %s window when date is a different weekday',
    async (day) => {
      const Overlay = await importOverlay();
      const windows = [makeWindow(`w-${day}`, day, '10:00:00', '12:00:00')];
      const otherDay: Weekday = day === 'Mon' ? 'Tue' : 'Mon';
      const { container } = render(Overlay, { windows, date: weekdayDate(otherDay) });
      expect(container.querySelectorAll('.schedule-window-band')).toHaveLength(0);
    },
  );
});

describe('ScheduleWindowOverlay — geometry', () => {
  const HOUR_HEIGHT = 60;

  const geometryCases = [
    {
      label: '09:00-11:00 → top=540, height=120',
      start: '09:00:00',
      end: '11:00:00',
      expectedTop: 9 * HOUR_HEIGHT,
      expectedHeight: 2 * HOUR_HEIGHT,
    },
    {
      label: '18:00-23:00 → top=1080, height=300',
      start: '18:00:00',
      end: '23:00:00',
      expectedTop: 18 * HOUR_HEIGHT,
      expectedHeight: 5 * HOUR_HEIGHT,
    },
    {
      label: '07:30-09:00 → top=450, height=90',
      start: '07:30:00',
      end: '09:00:00',
      expectedTop: 7.5 * HOUR_HEIGHT,
      expectedHeight: 1.5 * HOUR_HEIGHT,
    },
    {
      label: '00:00-24:00 full day → top=0, height=1440',
      start: '00:00:00',
      end: '24:00:00',
      expectedTop: 0,
      expectedHeight: 24 * HOUR_HEIGHT,
    },
  ];

  it.each(geometryCases)('$label', async ({ start, end, expectedTop, expectedHeight }) => {
    const Overlay = await importOverlay();
    const windows = [makeWindow('w1', 'Mon', start, end)];
    const { container } = render(Overlay, { windows, date: weekdayDate('Mon') });
    const band = container.querySelector('.schedule-window-band') as HTMLElement | null;
    expect(band).not.toBeNull();
    expect(band!.style.top).toBe(`${expectedTop}px`);
    expect(band!.style.height).toBe(`${expectedHeight}px`);
  });

  it('only matching-day bands are rendered when mixed days are passed', async () => {
    const Overlay = await importOverlay();
    const windows = [
      makeWindow('w-mon', 'Mon', '08:00:00', '10:00:00'),
      makeWindow('w-tue', 'Tue', '08:00:00', '10:00:00'),
      makeWindow('w-wed', 'Wed', '08:00:00', '10:00:00'),
    ];
    const { container } = render(Overlay, { windows, date: weekdayDate('Mon') });
    const bands = container.querySelectorAll('.schedule-window-band');
    expect(bands).toHaveLength(1);
    expect((bands[0] as HTMLElement).style.top).toBe(`${8 * HOUR_HEIGHT}px`);
  });
});

describe('ScheduleWindowOverlay — accessibility', () => {
  it('bands are aria-hidden (decorative)', async () => {
    const Overlay = await importOverlay();
    const windows = [makeWindow('w1', 'Wed', '09:00:00', '11:00:00')];
    const { container } = render(Overlay, { windows, date: weekdayDate('Wed') });
    const band = container.querySelector('.schedule-window-band');
    expect(band).not.toBeNull();
    expect(band!.getAttribute('aria-hidden')).toBe('true');
  });
});
