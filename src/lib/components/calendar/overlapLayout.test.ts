// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import type { AgendaItem } from '../../types';
import { layoutDayColumn, layoutOverlappingRanges } from './overlapLayout';
import { externalEventFixture } from './testFixtures';

function item(id: string, start: string, end: string): AgendaItem {
  return {
    chunk: {
      id,
      task_id: `task-${id}`,
      start_time: start,
      end_time: end,
      status: 'scheduled',
      is_fixed: false,
      logged_minutes: null,
      completed_at: null,
      google_event_id: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    },
    task_title: id,
    task_priority: 'Medium',
    task_labels: [],
    task_recurring_template_id: null,
    task_deadline: null,
  };
}

/** Expected lane geometry for one item, in input order. */
interface ExpectedLane {
  overlapCount: number;
  overlapIndex: number;
  leftPercent: number;
  widthPercent: number;
}

interface LayoutCase {
  name: string;
  items: AgendaItem[];
  /** Overlap threshold override; omitted cases use the default calendar scale. */
  minVisualMs?: number;
  expected: ExpectedLane[];
}

const FULL: ExpectedLane = {
  overlapCount: 1,
  overlapIndex: 0,
  leftPercent: 0,
  widthPercent: 100,
};

/** Two equal side-by-side lanes — the common 2-way overlap split. */
const SPLIT_2: ExpectedLane[] = [
  { overlapCount: 2, overlapIndex: 0, leftPercent: 0, widthPercent: 50 },
  { overlapCount: 2, overlapIndex: 1, leftPercent: 50, widthPercent: 50 },
];

/** Assert a 2-item layout split into side-by-side lanes (indices 0 and 1 of a 2-cluster). */
function expectTwoLanes(result: ReadonlyArray<{ overlapCount: number; overlapIndex: number }>) {
  expect(result[0].overlapCount).toBe(2);
  expect(result[1].overlapCount).toBe(2);
  expect(result[0].overlapIndex).toBe(0);
  expect(result[1].overlapIndex).toBe(1);
}

const cases: LayoutCase[] = [
  { name: 'no chunks → empty layout', items: [], expected: [] },
  {
    name: 'non-overlapping chunks each stay full width',
    items: [
      item('a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:00:00.000Z'),
      item('b', '2026-03-28T10:00:00.000Z', '2026-03-28T11:00:00.000Z'),
    ],
    expected: [FULL, FULL],
  },
  {
    name: 'overlapping chunks split into equal lanes',
    items: [
      item('a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:00:00.000Z'),
      item('b', '2026-03-28T09:30:00.000Z', '2026-03-28T10:30:00.000Z'),
    ],
    expected: SPLIT_2,
  },
  {
    name: 'an overlap chain is one cluster and reuses a freed lane',
    items: [
      item('a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:00:00.000Z'),
      item('b', '2026-03-28T09:30:00.000Z', '2026-03-28T10:30:00.000Z'),
      item('c', '2026-03-28T10:15:00.000Z', '2026-03-28T11:15:00.000Z'),
    ],
    expected: [
      { overlapCount: 2, overlapIndex: 0, leftPercent: 0, widthPercent: 50 },
      { overlapCount: 2, overlapIndex: 1, leftPercent: 50, widthPercent: 50 },
      { overlapCount: 2, overlapIndex: 0, leftPercent: 0, widthPercent: 50 },
    ],
  },
  {
    // Each chunk is < the min drawn height, so all three boxes collide.
    name: 'short back-to-back chunks lane via minimum drawn height',
    items: [
      item('a', '2026-03-28T20:00:00.000Z', '2026-03-28T20:05:00.000Z'),
      item('b', '2026-03-28T20:05:00.000Z', '2026-03-28T20:10:00.000Z'),
      item('c', '2026-03-28T20:10:00.000Z', '2026-03-28T20:15:00.000Z'),
    ],
    expected: [
      { overlapCount: 3, overlapIndex: 0, leftPercent: 0, widthPercent: 100 / 3 },
      { overlapCount: 3, overlapIndex: 1, leftPercent: 100 / 3, widthPercent: 100 / 3 },
      { overlapCount: 3, overlapIndex: 2, leftPercent: 200 / 3, widthPercent: 100 / 3 },
    ],
  },
  {
    // Real-time gap (20:05 → 20:12) but within ~22min, so a's box reaches b.
    name: 'a real-time gap within the min drawn height still lanes',
    items: [
      item('a', '2026-03-28T20:00:00.000Z', '2026-03-28T20:05:00.000Z'),
      item('b', '2026-03-28T20:12:00.000Z', '2026-03-28T20:42:00.000Z'),
    ],
    expected: SPLIT_2,
  },
  {
    name: 'chunks beyond the min drawn height stay separate',
    items: [
      item('a', '2026-03-28T20:00:00.000Z', '2026-03-28T20:05:00.000Z'),
      item('b', '2026-03-28T20:25:00.000Z', '2026-03-28T20:30:00.000Z'),
    ],
    expected: [FULL, FULL],
  },
  {
    // Guards against the over-merge a coarse grid would cause.
    name: 'large back-to-back chunks stay full width',
    items: [
      item('a', '2026-03-28T09:05:00.000Z', '2026-03-28T10:35:00.000Z'),
      item('b', '2026-03-28T10:40:00.000Z', '2026-03-28T12:10:00.000Z'),
    ],
    expected: [FULL, FULL],
  },
  {
    // Zoom seam: a tiny threshold makes short back-to-back boxes no longer collide.
    name: 'a small minVisualMs keeps short back-to-back chunks separate',
    items: [
      item('a', '2026-03-28T20:00:00.000Z', '2026-03-28T20:05:00.000Z'),
      item('b', '2026-03-28T20:05:00.000Z', '2026-03-28T20:10:00.000Z'),
    ],
    minVisualMs: 60 * 1000,
    expected: [FULL, FULL],
  },
  {
    // Zoom seam: a large threshold merges chunks far apart in real time.
    name: 'a large minVisualMs merges chunks that are far apart',
    items: [
      item('a', '2026-03-28T20:00:00.000Z', '2026-03-28T20:30:00.000Z'),
      item('b', '2026-03-28T21:00:00.000Z', '2026-03-28T21:30:00.000Z'),
    ],
    minVisualMs: 90 * 60 * 1000,
    expected: SPLIT_2,
  },
];

describe('layoutDayColumn — chunks only', () => {
  it.each(cases)('$name', ({ items, minVisualMs, expected }) => {
    const layout = layoutDayColumn(items, [], minVisualMs).chunks;

    expect(layout).toHaveLength(expected.length);
    expected.forEach((lane, i) => {
      expect(layout[i]?.overlapCount).toBe(lane.overlapCount);
      expect(layout[i]?.overlapIndex).toBe(lane.overlapIndex);
      expect(layout[i]?.leftPercent).toBeCloseTo(lane.leftPercent);
      expect(layout[i]?.widthPercent).toBeCloseTo(lane.widthPercent);
    });
  });
});

interface TimeRange {
  startMs: number;
  endMs: number;
}

function range(startMs: number, endMs: number): TimeRange {
  return { startMs, endMs };
}

interface OverlapCase {
  name: string;
  items: TimeRange[];
  expected: { overlapCount: number; widthPercent: number }[];
}

const overlapCases: OverlapCase[] = [
  { name: 'empty input returns empty layout', items: [], expected: [] },
  {
    name: 'non-overlapping ranges each stay full width',
    items: [range(0, 1000), range(2000, 3000)],
    expected: [
      { overlapCount: 1, widthPercent: 100 },
      { overlapCount: 1, widthPercent: 100 },
    ],
  },
  {
    name: 'two overlapping ranges split into two lanes',
    items: [range(0, 2000), range(1000, 3000)],
    expected: [
      { overlapCount: 2, widthPercent: 50 },
      { overlapCount: 2, widthPercent: 50 },
    ],
  },
];

describe('layoutOverlappingRanges', () => {
  it.each(overlapCases)('$name', ({ items, expected }) => {
    const result = layoutOverlappingRanges(
      items,
      (r) => r.startMs,
      (r) => r.endMs,
      0,
    );
    expect(result).toHaveLength(expected.length);
    expected.forEach((exp, i) => {
      expect(result[i].overlapCount).toBe(exp.overlapCount);
      expect(result[i].widthPercent).toBeCloseTo(exp.widthPercent);
    });
  });

  it('preserves the original item reference in each layout result', () => {
    const a = range(0, 1000);
    const b = range(500, 1500);
    const result = layoutOverlappingRanges(
      [a, b],
      (r) => r.startMs,
      (r) => r.endMs,
      0,
    );
    expect(result[0].item).toBe(a);
    expect(result[1].item).toBe(b);
  });
});

function ext(id: string, start: string, end: string) {
  return externalEventFixture({
    id: `row-${id}`,
    event_id: id,
    start_time: start,
    end_time: end,
  });
}

describe('layoutDayColumn — externals only', () => {
  type ExtEvent = ReturnType<typeof ext>;
  type ExtLayoutResult = ReturnType<typeof layoutDayColumn>['externals'];

  it.each([
    {
      label: 'overlapping events lane side by side',
      makeEvents: () =>
        [
          ext('provider-a', '2026-03-28T12:00:00.000Z', '2026-03-28T13:00:00.000Z'),
          ext('provider-b', '2026-03-28T12:30:00.000Z', '2026-03-28T13:30:00.000Z'),
        ] as ExtEvent[],
      check: (result: ExtLayoutResult, evts: ExtEvent[]) => {
        expect(result[0].item).toBe(evts[0]);
        expect(result[1].item).toBe(evts[1]);
        expectTwoLanes(result);
      },
    },
    {
      label: 'non-overlapping events stay at full width',
      makeEvents: () =>
        [
          ext('provider-a', '2026-03-28T12:00:00.000Z', '2026-03-28T13:00:00.000Z'),
          ext('provider-b', '2026-03-28T15:00:00.000Z', '2026-03-28T16:00:00.000Z'),
        ] as ExtEvent[],
      check: (result: ExtLayoutResult) => {
        expect(result[0].widthPercent).toBe(100);
        expect(result[1].widthPercent).toBe(100);
      },
    },
  ])('$label', ({ makeEvents, check }) => {
    const evts = makeEvents();
    const result = layoutDayColumn([], evts).externals;
    check(result, evts);
  });
});

describe('layoutDayColumn — merged lanes', () => {
  function layoutAndExpectExternalFirst(
    chunks: ReturnType<typeof item>[],
    events: ReturnType<typeof ext>[],
  ) {
    const result = layoutDayColumn(chunks, events);
    expect(result.externals[0]?.overlapIndex).toBe(0);
    expect(result.chunks[0]?.overlapIndex).toBe(1);
    return result;
  }

  it('empty inputs return empty layouts', () => {
    const result = layoutDayColumn([], []);
    expect(result.chunks).toHaveLength(0);
    expect(result.externals).toHaveLength(0);
  });

  it('an external overlapping a chunk takes the left lane even when the chunk starts first', () => {
    const chunk = item('a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:30:00.000Z');
    const event = ext('provider-a', '2026-03-28T10:00:00.000Z', '2026-03-28T11:00:00.000Z');

    const result = layoutDayColumn([chunk], [event]);

    expect(result.externals[0]).toMatchObject({
      item: event,
      overlapCount: 2,
      overlapIndex: 0,
      leftPercent: 0,
    });
    expect(result.chunks[0]).toMatchObject({
      item: chunk,
      overlapCount: 2,
      overlapIndex: 1,
      leftPercent: 50,
    });
  });

  it('a non-overlapping external and chunk both stay full width', () => {
    const chunk = item('a', '2026-03-28T12:00:00.000Z', '2026-03-28T13:00:00.000Z');
    const event = ext('provider-a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:00:00.000Z');

    const result = layoutDayColumn([chunk], [event]);

    expect(result.chunks[0]).toMatchObject({ overlapCount: 1, widthPercent: 100 });
    expect(result.externals[0]).toMatchObject({ overlapCount: 1, widthPercent: 100 });
  });

  it('two overlapping externals push an overlapping chunk to the third lane', () => {
    const chunk = item('a', '2026-03-28T10:00:00.000Z', '2026-03-28T11:30:00.000Z');
    const eventA = ext('provider-a', '2026-03-28T09:00:00.000Z', '2026-03-28T11:00:00.000Z');
    const eventB = ext('provider-b', '2026-03-28T09:30:00.000Z', '2026-03-28T10:30:00.000Z');

    const result = layoutDayColumn([chunk], [eventA, eventB]);

    expect(result.externals[0]?.overlapIndex).toBe(0);
    expect(result.externals[1]?.overlapIndex).toBe(1);
    expect(result.chunks[0]?.overlapIndex).toBe(2);
    expect(result.chunks[0]?.overlapCount).toBe(3);
    expect(result.chunks[0]?.leftPercent).toBeCloseTo(200 / 3);
  });

  it('an external pushes two overlapping chunks to lanes 1 and 2', () => {
    const chunkA = item('a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:00:00.000Z');
    const chunkB = item('b', '2026-03-28T09:30:00.000Z', '2026-03-28T10:30:00.000Z');
    const event = ext('provider-a', '2026-03-28T09:00:00.000Z', '2026-03-28T11:00:00.000Z');

    const result = layoutAndExpectExternalFirst([chunkA, chunkB], [event]);

    expect(result.chunks[1]?.overlapIndex).toBe(2);
    expect(result.chunks[0]?.overlapCount).toBe(3);
  });

  it('chunk lanes free independently of external lanes within a cluster', () => {
    // Both chunks overlap the long external but not each other, so they share
    // the single chunk lane to the right of the external lane.
    const chunkA = item('a', '2026-03-28T09:00:00.000Z', '2026-03-28T10:00:00.000Z');
    const chunkB = item('b', '2026-03-28T10:30:00.000Z', '2026-03-28T11:30:00.000Z');
    const event = ext('provider-a', '2026-03-28T09:00:00.000Z', '2026-03-28T12:00:00.000Z');

    const result = layoutAndExpectExternalFirst([chunkA, chunkB], [event]);

    expect(result.chunks[1]?.overlapIndex).toBe(1);
    expect(result.chunks[0]?.overlapCount).toBe(2);
    expect(result.chunks[1]?.overlapCount).toBe(2);
  });
});
