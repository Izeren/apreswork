// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { installViewTestHooks, localDate } from './testFixtures';

installViewTestHooks();

async function importTimeGrid() {
  const mod = await import('./TimeGrid.svelte');
  return mod.default;
}

async function renderTimeGrid(date: Date = localDate(2026, 3, 28)) {
  const TimeGrid = await importTimeGrid();
  const result = render(TimeGrid, { date });
  await tick();
  return result;
}

function hourLabelTexts(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll('.time-label'))
    .map((el) => el.textContent?.trim() ?? '')
    .filter((text) => text.length > 0);
}

describe('TimeGrid — hour labels', () => {
  it.each(Array.from({ length: 24 }, (_, i) => ({ index: i, expected: `${i}:00` })))(
    'label at index $index is "$expected"',
    async ({ index, expected }) => {
      const { container } = await renderTimeGrid();
      const texts = hourLabelTexts(container);
      expect(texts).toHaveLength(24);
      expect(texts[index]).toBe(expected);
    },
  );
});

describe('TimeGrid — hour lines', () => {
  it('renders 24 hour rows', async () => {
    const { container } = await renderTimeGrid();

    expect(container.querySelectorAll('.hour-row')).toHaveLength(24);
  });

  it('each hour row contains an hour-line element', async () => {
    const { container } = await renderTimeGrid();

    container.querySelectorAll('.hour-row').forEach((row) => {
      expect(row.querySelector('.hour-line')).toBeTruthy();
    });
  });
});

describe('TimeGrid — half-hour markers', () => {
  it('renders 24 half-hour markers (one per hour row)', async () => {
    const { container } = await renderTimeGrid();

    expect(container.querySelectorAll('.half-hour-marker')).toHaveLength(24);
  });

  it('each hour row contains exactly one half-hour-marker', async () => {
    const { container } = await renderTimeGrid();

    container.querySelectorAll('.hour-row').forEach((row) => {
      expect(row.querySelectorAll('.half-hour-marker')).toHaveLength(1);
    });
  });
});

describe('TimeGrid — current time indicator', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('shows the time indicator when date is today', async () => {
    const frozenToday = new Date(2026, 2, 28, 9, 30, 0);
    vi.setSystemTime(frozenToday);
    const todayDate = localDate(
      frozenToday.getFullYear(),
      frozenToday.getMonth() + 1,
      frozenToday.getDate(),
    );
    const { container } = await renderTimeGrid(todayDate);
    expect(container.querySelector('.time-indicator')).toBeTruthy();
  });

  it.each([
    { label: 'past', date: localDate(2000, 1, 1) },
    { label: 'future', date: localDate(2099, 12, 31) },
  ])('hides time indicator for $label dates', async ({ date }) => {
    const { container } = await renderTimeGrid(date);
    expect(container.querySelector('.time-indicator')).toBeNull();
  });

  it('time indicator top position reflects current hour and minute', async () => {
    const fixedNow = new Date(2026, 2, 28, 9, 30, 0);
    vi.setSystemTime(fixedNow);

    const { container } = await renderTimeGrid(localDate(2026, 3, 28));

    const indicator = container.querySelector('.time-indicator') as HTMLElement | null;
    expect(indicator).toBeTruthy();
    // Expected top: (9 + 30/60) * 60 = 9.5 * 60 = 570px
    expect(indicator!.style.top).toBe('570px');
  });

  it('interval is set up and cleared on unmount (no timer leak)', async () => {
    const setIntervalSpy = vi.spyOn(globalThis, 'setInterval');
    const clearIntervalSpy = vi.spyOn(globalThis, 'clearInterval');

    const { unmount } = await renderTimeGrid();

    expect(setIntervalSpy).toHaveBeenCalledWith(expect.any(Function), 60_000);

    unmount();
    await tick();

    expect(clearIntervalSpy).toHaveBeenCalled();
  });
});

describe('TimeGrid — layout', () => {
  it.each([
    { selector: '.time-grid', label: 'time-grid container' },
    { selector: '.grid-content', label: 'grid-content overlay' },
  ])('$label is present', async ({ selector }) => {
    const { container } = await renderTimeGrid();

    expect(container.querySelector(selector)).toBeTruthy();
  });
});

describe('TimeGrid — edge cases', () => {
  it.each([
    { label: 'the epoch (Jan 1, 1970)', date: new Date(0) },
    { label: 'a far-future date (Dec 31, 2099)', date: localDate(2099, 12, 31) },
  ])('renders correctly for $label', async ({ date }) => {
    const { container } = await renderTimeGrid(date);

    expect(container.querySelector('.time-grid')).toBeTruthy();
    expect(container.querySelectorAll('.hour-row')).toHaveLength(24);
    expect(container.querySelector('.time-indicator')).toBeNull();
  });
});
