// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import type { ExternalEvent } from '../../types';
import { externalEventFixture } from './testFixtures';

let Block: Awaited<ReturnType<typeof importBlock>>;
beforeEach(async () => {
  Block = await importBlock();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importBlock() {
  const mod = await import('./ExternalEventBlock.svelte');
  return mod.default;
}

interface TreatmentCase {
  label: string;
  event: ExternalEvent;
  expectedModifierClass: string | null;
  ariaContains: string | null;
}

const treatmentCases: TreatmentCase[] = [
  {
    label: 'busy (default) — no modifier class, no suffix in aria-label',
    event: externalEventFixture({ busy: true, declined: false }),
    expectedModifierClass: null,
    ariaContains: null,
  },
  {
    label: 'declined — --declined class, ", declined" in aria-label',
    event: externalEventFixture({ busy: true, declined: true }),
    expectedModifierClass: 'external-event--declined',
    ariaContains: ', declined',
  },
  {
    label: 'free — --free class, ", free" in aria-label',
    event: externalEventFixture({ busy: false, declined: false }),
    expectedModifierClass: 'external-event--free',
    ariaContains: ', free',
  },
];

describe('ExternalEventBlock — visual treatments', () => {
  it.each(treatmentCases)('$label', async ({ event, expectedModifierClass, ariaContains }) => {
    const { container } = render(Block, { event });

    const root = container.querySelector('.external-event');
    expect(root).toBeTruthy();

    if (expectedModifierClass) {
      expect(root!.classList.contains(expectedModifierClass)).toBe(true);
    } else {
      expect(root!.classList.contains('external-event--declined')).toBe(false);
      expect(root!.classList.contains('external-event--free')).toBe(false);
    }

    const label = root!.getAttribute('aria-label') ?? '';
    expect(label).toContain('External event:');
    if (ariaContains) {
      expect(label).toContain(ariaContains);
    } else {
      expect(label).not.toContain(', declined');
      expect(label).not.toContain(', free');
    }
  });
});

describe('ExternalEventBlock — title', () => {
  it('always renders the event title', async () => {
    const { container } = render(Block, {
      event: externalEventFixture({ title: 'My Calendar Event' }),
    });
    const title = container.querySelector('.title');
    expect(title).toBeTruthy();
    expect(title!.textContent).toContain('My Calendar Event');
  });
});

describe('ExternalEventBlock — time label', () => {
  it.each([
    {
      label: 'shows time for a 60-min event (height ~60px >> 42px)',
      end_time: '2026-03-28T13:00:00.000Z',
      shouldExist: true,
    },
    // 15px natural height → clamped to CHUNK_MIN_HEIGHT_PX=22px < 42px
    {
      label: 'hides time for a 15-min event (clamped to 22px < 42px)',
      end_time: '2026-03-28T12:15:00.000Z',
      shouldExist: false,
    },
  ])('$label', async ({ end_time, shouldExist }) => {
    const { container } = render(Block, {
      event: externalEventFixture({ start_time: '2026-03-28T12:00:00.000Z', end_time }),
    });
    expect(!!container.querySelector('.time')).toBe(shouldExist);
  });
});

describe('ExternalEventBlock — position style', () => {
  it('sets top based on local hours of start_time', async () => {
    // 2026-03-28T12:00:00.000Z → local hours depend on TZ; test verifies top: NNNpx present
    const event = externalEventFixture({
      start_time: '2026-03-28T12:00:00.000Z',
      end_time: '2026-03-28T13:00:00.000Z',
    });
    const { container } = render(Block, { event });
    const root = container.querySelector('.external-event') as HTMLElement;
    expect(root).toBeTruthy();
    expect(root.style.top).toMatch(/^\d+(\.\d+)?px$/);
  });

  it('sets height for a 60-min event to 60px', async () => {
    const event = externalEventFixture({
      start_time: '2026-03-28T00:00:00.000Z',
      end_time: '2026-03-28T01:00:00.000Z',
    });
    const { container } = render(Block, { event });
    const root = container.querySelector('.external-event') as HTMLElement;
    expect(root.style.height).toBe('60px');
  });
});

describe('ExternalEventBlock — overlap layout', () => {
  it('uses a narrower width when overlapCount=2 vs 1', async () => {
    const event = externalEventFixture();

    const { container: c1 } = render(Block, { event, overlapCount: 1 });
    const { container: c2 } = render(Block, { event, overlapCount: 2 });

    const w1 = (c1.querySelector('.external-event') as HTMLElement).style.width;
    const w2 = (c2.querySelector('.external-event') as HTMLElement).style.width;
    expect(w1).toBeTruthy();
    expect(w2).toBeTruthy();
    expect(w1).not.toBe(w2);
  });

  it('renders at z-index 1', async () => {
    const { container } = render(Block, { event: externalEventFixture() });
    const root = container.querySelector('.external-event') as HTMLElement;
    expect(root.style.zIndex).toBe('1');
  });
});

describe('ExternalEventBlock — interactivity', () => {
  it.each([
    {
      label: 'is a non-interactive img with no onopen (read-only)',
      onopen: undefined as (() => void) | undefined,
      role: 'img',
      tabindex: null as string | null,
      interactive: false,
    },
    {
      label: 'renders as a focusable button when onopen is supplied',
      onopen: vi.fn() as () => void,
      role: 'button',
      tabindex: '0',
      interactive: true,
    },
  ])('$label', async ({ onopen, role, tabindex, interactive }) => {
    const { container } = render(Block, { event: externalEventFixture(), onopen });
    const root = container.querySelector('.external-event') as HTMLElement;
    expect(root.getAttribute('role')).toBe(role);
    expect(root.getAttribute('tabindex')).toBe(tabindex);
    expect(root.classList.contains('external-event--interactive')).toBe(interactive);
  });

  it('calls onopen with the event on click', async () => {
    const onopen = vi.fn();
    const event = externalEventFixture({ event_id: 'evt-click' });
    const { container } = render(Block, { event, onopen });
    const root = container.querySelector('.external-event') as HTMLElement;
    await fireEvent.click(root);
    expect(onopen).toHaveBeenCalledTimes(1);
    expect(onopen.mock.calls[0][0].event_id).toBe('evt-click');
  });

  const keyCases = [
    { label: 'Enter key triggers onopen', key: 'Enter', expectedCalls: 1 },
    { label: 'space key triggers onopen', key: ' ', expectedCalls: 1 },
    { label: 'other keys are ignored', key: 'a', expectedCalls: 0 },
  ];

  it.each(keyCases)('$label', async ({ key, expectedCalls }) => {
    const onopen = vi.fn();
    const { container } = render(Block, { event: externalEventFixture(), onopen });
    const root = container.querySelector('.external-event') as HTMLElement;
    await fireEvent.keyDown(root, { key });
    expect(onopen).toHaveBeenCalledTimes(expectedCalls);
  });
});

describe('ExternalEventBlock — all-day', () => {
  const allDayEvent = () =>
    externalEventFixture({
      all_day: true,
      title: 'Vacation',
      start_time: '2026-03-28T00:00:00.000Z',
      end_time: '2026-03-29T00:00:00.000Z',
    });

  type CheckFns = {
    container: HTMLElement;
    root: HTMLElement;
  };

  it.each([
    {
      label: 'adds --allday modifier class',
      check: ({ root }: CheckFns) => {
        expect(root.classList.contains('external-event--allday')).toBe(true);
      },
    },
    {
      label: 'labels "all day" not a time range in aria-label',
      check: ({ root }: CheckFns) => {
        const label = root.getAttribute('aria-label') ?? '';
        expect(label).toContain('all day');
        expect(label).not.toMatch(/\d{1,2}:\d{2}/);
      },
    },
    {
      label: 'no .time element',
      check: ({ container }: CheckFns) => {
        expect(container.querySelector('.time')).toBeNull();
      },
    },
    {
      label: 'no inline top positioning',
      check: ({ root }: CheckFns) => {
        expect(root.style.top).toBe('');
      },
    },
  ])('$label', async ({ check }) => {
    const { container } = render(Block, { event: allDayEvent() });
    const root = container.querySelector('.external-event') as HTMLElement;
    check({ container, root });
  });

  it('still clickable with onopen', async () => {
    const onopen = vi.fn();
    const { container } = render(Block, { event: allDayEvent(), onopen });
    const root = container.querySelector('.external-event') as HTMLElement;
    expect(root.getAttribute('role')).toBe('button');
    await fireEvent.click(root);
    expect(onopen).toHaveBeenCalledTimes(1);
  });
});

describe('ExternalEventBlock — disconnected', () => {
  it.each([
    {
      label: 'adds --disconnected class when disconnected=true',
      disconnected: true,
      expected: true,
    },
    {
      label: 'no --disconnected class when disconnected=false',
      disconnected: false,
      expected: false,
    },
    { label: 'no --disconnected class by default', disconnected: undefined, expected: false },
  ])('$label', async ({ disconnected, expected }) => {
    const { container } = render(Block, { event: externalEventFixture(), disconnected });
    const root = container.querySelector('.external-event') as HTMLElement;
    expect(root.classList.contains('external-event--disconnected')).toBe(expected);
  });
});
