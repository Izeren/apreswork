// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import type { ExternalEvent } from '../../types';
import type { RangeLayoutItem } from './overlapLayout';
import { externalEventFixture } from './testFixtures';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importLayer() {
  const mod = await import('./DayExternalsLayer.svelte');
  return mod.default;
}

function layoutItem(
  event: ExternalEvent,
  overrides: Partial<Omit<RangeLayoutItem<ExternalEvent>, 'item'>> = {},
): RangeLayoutItem<ExternalEvent> {
  return {
    item: event,
    overlapIndex: 0,
    overlapCount: 1,
    leftPercent: 0,
    widthPercent: 100,
    ...overrides,
  };
}

describe('DayExternalsLayer', () => {
  it('renders one ExternalEventBlock per item, in order', async () => {
    const Layer = await importLayer();
    const events = [
      externalEventFixture({ event_id: 'evt-1', title: 'First' }),
      externalEventFixture({ event_id: 'evt-2', title: 'Second' }),
    ];
    const externals = events.map((e) => layoutItem(e));

    const { container } = render(Layer, {
      externals,
      eventOpenHandler: () => null,
    });

    const blocks = container.querySelectorAll('.external-event');
    expect(blocks).toHaveLength(2);
    expect(blocks[0]?.querySelector('.title')?.textContent).toContain('First');
    expect(blocks[1]?.querySelector('.title')?.textContent).toContain('Second');
  });

  it('passes overlapIndex and overlapCount to each block: overlap style differs from non-overlap', async () => {
    const Layer = await importLayer();
    const baseEvent = externalEventFixture({
      start_time: '2026-03-28T10:00:00.000Z',
      end_time: '2026-03-28T11:00:00.000Z',
    });

    const { container: c1 } = render(Layer, {
      externals: [layoutItem(baseEvent, { overlapIndex: 0, overlapCount: 1 })],
      eventOpenHandler: () => null,
    });
    const { container: c3 } = render(Layer, {
      externals: [
        layoutItem(externalEventFixture({ event_id: 'evt-overlap' }), {
          overlapIndex: 1,
          overlapCount: 3,
        }),
      ],
      eventOpenHandler: () => null,
    });

    const w1 = (c1.querySelector('.external-event') as HTMLElement).style.width;
    const w3 = (c3.querySelector('.external-event') as HTMLElement).style.width;

    expect(w1).toBeTruthy();
    expect(w3).toBeTruthy();
    // Overlapping layout gives a different (narrower) width expression
    expect(w1).not.toBe(w3);
  });

  it('passes eventOpenHandler return value as onopen: click fires the returned handler', async () => {
    const Layer = await importLayer();
    const returnedHandler = vi.fn();
    const event = externalEventFixture({ event_id: 'click-test' });
    const externals = [layoutItem(event)];

    const { container } = render(Layer, {
      externals,
      eventOpenHandler: () => returnedHandler,
    });

    const block = container.querySelector('.external-event') as HTMLElement;
    await fireEvent.click(block);

    expect(returnedHandler).toHaveBeenCalledOnce();
  });

  it('null return from eventOpenHandler → block renders as read-only (role=img)', async () => {
    const Layer = await importLayer();
    const externals = [layoutItem(externalEventFixture({ event_id: 'readonly' }))];

    const { container } = render(Layer, {
      externals,
      eventOpenHandler: () => null,
    });

    const block = container.querySelector('.external-event') as HTMLElement;
    expect(block.getAttribute('role')).toBe('img');
    expect(block.getAttribute('tabindex')).toBeNull();
  });

  it('empty externals array renders nothing', async () => {
    const Layer = await importLayer();

    const { container } = render(Layer, {
      externals: [],
      eventOpenHandler: () => null,
    });

    expect(container.querySelector('.external-event')).toBeNull();
  });
});
