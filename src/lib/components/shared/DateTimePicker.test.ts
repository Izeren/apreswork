// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import DateTimePicker from './DateTimePicker.svelte';
import { WEEK_START_STORAGE_KEY } from '../../weekStartPref';
import { FALLBACK_TIME } from './dateTimePickerShared';
import { installLocalStorageStub } from '../../storageStubHooks';

installLocalStorageStub({ restoreMocks: true });

const DTP_TEST_NOW = new Date('2026-07-28T12:00:00Z');

async function openPicker(container: HTMLElement) {
  const trigger = container.querySelector('button[aria-haspopup="dialog"]') as HTMLButtonElement;
  await fireEvent.click(trigger);
  await tick();
}

describe('DateTimePicker — null value', () => {
  it.each([
    { label: 'shows Pick a date placeholder', expectedText: 'Pick a date' as string },
    { label: 'shows FALLBACK_TIME when no defaultTime', expectedText: FALLBACK_TIME },
  ])('$label', ({ expectedText }) => {
    const { container } = render(DateTimePicker, {
      value: null,
      onchange: vi.fn(),
      now: DTP_TEST_NOW,
    });
    const trigger = container.querySelector('button[aria-haspopup="dialog"]') as HTMLButtonElement;
    expect(trigger.textContent).toContain(expectedText);
  });
});

describe('DateTimePicker — Today quick date', () => {
  it('calls onchange with ISO string for today at default time', async () => {
    const onchange = vi.fn();
    const { container } = render(DateTimePicker, { value: null, onchange, now: DTP_TEST_NOW });
    await openPicker(container);

    const todayBtn = container.querySelector('[data-shortcut="today"]') as HTMLButtonElement;
    expect(todayBtn).toBeTruthy();
    await fireEvent.click(todayBtn);
    await tick();

    expect(onchange).toHaveBeenCalledOnce();
    const iso = onchange.mock.calls[0][0] as string;
    expect(typeof iso).toBe('string');
    const result = new Date(iso);
    const today = DTP_TEST_NOW;
    expect(result.getDate()).toBe(today.getDate());
    expect(result.getMonth()).toBe(today.getMonth());
    expect(result.getFullYear()).toBe(today.getFullYear());
    expect(result.getHours()).toBe(9);
    expect(result.getMinutes()).toBe(0);
  });
});

describe('DateTimePicker — week-start knob', () => {
  it('weekday header row starts with Mo by default', async () => {
    const { container } = render(DateTimePicker, {
      value: null,
      onchange: vi.fn(),
      now: DTP_TEST_NOW,
    });
    await openPicker(container);

    const row = container.querySelector('.weekday-row');
    expect(row).toBeTruthy();
    const firstLabel = row!.querySelector('span')?.textContent;
    expect(firstLabel).toBe('Mo');
  });

  it('clicking Sun changes first weekday label to Su and persists to localStorage', async () => {
    const { container } = render(DateTimePicker, {
      value: null,
      onchange: vi.fn(),
      now: DTP_TEST_NOW,
    });
    await openPicker(container);

    const sunBtn = Array.from(container.querySelectorAll('.week-start-btn')).find(
      (b) => b.textContent?.trim() === 'Sun',
    ) as HTMLButtonElement;
    expect(sunBtn).toBeTruthy();
    await fireEvent.click(sunBtn);
    await tick();

    const row = container.querySelector('.weekday-row');
    const firstLabel = row!.querySelector('span')?.textContent;
    expect(firstLabel).toBe('Su');
    expect(window.localStorage.getItem(WEEK_START_STORAGE_KEY)).toBe('sun');
  });
});

describe('DateTimePicker — Clear button', () => {
  it.each([
    { nullable: true, expectsButton: true },
    { nullable: false, expectsButton: false },
  ])(
    'nullable=$nullable: Clear button is present=$expectsButton',
    async ({ nullable, expectsButton }) => {
      const onchange = vi.fn();
      const { container } = render(DateTimePicker, {
        value: '2026-07-08T09:00:00.000Z',
        onchange,
        nullable,
        now: DTP_TEST_NOW,
      });

      const clearBtn = container.querySelector('.clear-btn') as HTMLButtonElement | null;
      if (expectsButton) {
        expect(clearBtn).toBeTruthy();
        await fireEvent.click(clearBtn!);
        await tick();
        expect(onchange).toHaveBeenCalledOnce();
        expect(onchange.mock.calls[0][0]).toBeNull();
      } else {
        expect(clearBtn).toBeNull();
      }
    },
  );
});

describe('DateTimePicker — prop change updates trigger', () => {
  it('re-renders trigger date label when value prop changes', async () => {
    const onchange = vi.fn();
    const { container, rerender } = render(DateTimePicker, {
      value: null,
      onchange,
      now: DTP_TEST_NOW,
    });

    const trigger = container.querySelector('button[aria-haspopup="dialog"]') as HTMLButtonElement;
    expect(trigger.textContent).toContain('Pick a date');

    await rerender({ value: '2026-07-08T09:00:00.000Z', onchange, now: DTP_TEST_NOW });
    await tick();

    expect(trigger.textContent).not.toContain('Pick a date');
    expect(trigger.textContent).toContain('08/07/2026');
  });
});

describe('DateTimePicker — popover positioning in jsdom', () => {
  it('opens popover without throwing despite zero-rect geometry', async () => {
    const { container } = render(DateTimePicker, {
      value: null,
      onchange: vi.fn(),
      now: DTP_TEST_NOW,
    });
    await expect(openPicker(container)).resolves.toBeUndefined();
    expect(container.querySelector('.picker-popover')).toBeTruthy();
  });

  it('repositions popover on window resize', async () => {
    const { container } = render(DateTimePicker, {
      value: null,
      onchange: vi.fn(),
      now: DTP_TEST_NOW,
    });
    await openPicker(container);
    const popover = container.querySelector('.picker-popover') as HTMLElement;
    const originalWidth = window.innerWidth;

    try {
      Object.defineProperty(window, 'innerWidth', {
        writable: true,
        configurable: true,
        value: 400,
      });
      window.dispatchEvent(new Event('resize'));
      await tick();

      expect(popover.getAttribute('style')).toContain('width: 376px');
    } finally {
      Object.defineProperty(window, 'innerWidth', {
        writable: true,
        configurable: true,
        value: originalWidth,
      });
    }
  });
});
