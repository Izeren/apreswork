// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, cleanup, fireEvent, type RenderResult } from '@testing-library/svelte';
import { tick } from 'svelte';
import TimeMenu from './TimeMenu.svelte';
import { STICKY_TIME_OPTIONS } from './dateTimePickerShared';
import { makeTimeMenuGetBCR } from '../../testFixtures';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

async function renderAndOpenMenu(onselect = vi.fn()) {
  const result = render(TimeMenu, { value: '10:30', onselect });
  await fireEvent.click(result.getByRole('button', { name: 'Time' }));
  await tick();
  return result;
}

describe('TimeMenu — closed by default', () => {
  it.each([
    {
      label: 'shows the current value in the trigger',
      check: (r: RenderResult<typeof TimeMenu>) => {
        expect(r.getByRole('button', { name: 'Time' }).textContent).toContain('10:30');
      },
    },
    {
      label: 'trigger has aria-expanded=false',
      check: (r: RenderResult<typeof TimeMenu>) => {
        expect(r.getByRole('button', { name: 'Time' }).getAttribute('aria-expanded')).toBe('false');
      },
    },
    {
      label: 'no listbox is rendered while closed',
      check: (r: RenderResult<typeof TimeMenu>) => {
        expect(r.queryByRole('listbox')).toBeNull();
      },
    },
  ])('$label', ({ check }) => {
    check(render(TimeMenu, { value: '10:30', onselect: vi.fn() }));
  });
});

describe('TimeMenu — after clicking trigger', () => {
  it('renders the listbox', async () => {
    const { getByRole } = await renderAndOpenMenu();
    expect(getByRole('listbox')).toBeTruthy();
  });

  it('shows 4 sticky quick-time options', async () => {
    const { getAllByRole } = await renderAndOpenMenu();
    const buttons = getAllByRole('button');
    const stickyLabels = STICKY_TIME_OPTIONS.map((o) => o.label);
    const stickyButtons = buttons.filter((b) =>
      stickyLabels.some((l) => b.textContent?.includes(l)),
    );
    expect(stickyButtons).toHaveLength(4);
  });

  it('the 10:30 option carries option-btn--active', async () => {
    const { container } = await renderAndOpenMenu();
    const active = container.querySelector('.option-btn--active');
    expect(active).toBeTruthy();
    expect(active?.getAttribute('data-time')).toBe('10:30');
  });

  it('trigger aria-expanded becomes true', async () => {
    const { getByRole } = await renderAndOpenMenu();
    expect(getByRole('button', { name: 'Time' }).getAttribute('aria-expanded')).toBe('true');
  });

  describe('repositions the menu on window resize', () => {
    let savedInnerHeight: number;
    beforeEach(() => {
      savedInnerHeight = window.innerHeight;
    });
    afterEach(() => {
      Object.defineProperty(window, 'innerHeight', {
        writable: true,
        configurable: true,
        value: savedInnerHeight,
      });
    });
    it('shrinks max-height when viewport gets shorter', async () => {
      const { container } = await renderAndOpenMenu();
      const menu = container.querySelector('.time-menu') as HTMLElement;

      Object.defineProperty(window, 'innerHeight', {
        writable: true,
        configurable: true,
        value: 300,
      });
      window.dispatchEvent(new Event('resize'));
      await tick();

      expect(menu.getAttribute('style')).toContain('max-height: 280px');
    });
  });
});

describe('TimeMenu — option selection', () => {
  it('calls onselect with the chosen time and closes the menu', async () => {
    const onselect = vi.fn();
    const { container, getByRole } = await renderAndOpenMenu(onselect);

    const btn = container.querySelector('[data-time="11:00"]') as HTMLButtonElement;
    expect(btn).toBeTruthy();
    await fireEvent.click(btn);
    await tick();

    expect(onselect).toHaveBeenCalledOnce();
    expect(onselect).toHaveBeenCalledWith('11:00');
    expect(getByRole('button', { name: 'Time' }).getAttribute('aria-expanded')).toBe('false');
  });
});

describe('TimeMenu — scrollSelectedTimeIntoView', () => {
  const originalGetBCR = HTMLElement.prototype.getBoundingClientRect;
  let originalClientHeightDescriptor: PropertyDescriptor | undefined;
  const mockGetBCR = makeTimeMenuGetBCR({
    listHeight: 300,
    listBottom: 350,
    activeTop: 250,
    activeBottom: 282,
    activeY: 250,
  });

  beforeEach(() => {
    HTMLElement.prototype.getBoundingClientRect = mockGetBCR;

    // clientHeight drives the centering denominator — stub to 300.
    originalClientHeightDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      'clientHeight',
    );
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get: () => 300,
    });
  });

  afterEach(() => {
    HTMLElement.prototype.getBoundingClientRect = originalGetBCR;
    if (originalClientHeightDescriptor) {
      Object.defineProperty(HTMLElement.prototype, 'clientHeight', originalClientHeightDescriptor);
    } else {
      Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight');
    }
  });

  it.each([
    {
      label: 'sets scrollTop to center the active option',
      value: '10:30',
      // '10:30' is not sticky → scrollSelectedTimeIntoView runs fully.
      // relativeTop = optionTop(250) − listTop(50) + scrollTop(0) = 200
      // targetScrollTop = max(0, 200 − (clientHeight(300) − optionHeight(32)) / 2) = 66
      expectedScrollTop: 66,
    },
    {
      label: 'does not touch scrollTop for a sticky value (09:00)',
      value: '09:00',
      // '09:00' is sticky → scrollSelectedTimeIntoView returns early.
      expectedScrollTop: 0,
    },
  ])('$label', async ({ value, expectedScrollTop }) => {
    const { container, getByRole } = render(TimeMenu, { value, onselect: vi.fn() });
    await fireEvent.click(getByRole('button', { name: 'Time' }));
    await tick();

    const list = container.querySelector('.time-menu-list') as HTMLElement;
    expect(list?.scrollTop ?? 0).toBe(expectedScrollTop);
  });
});
