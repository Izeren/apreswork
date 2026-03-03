// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { AgendaItem, ExternalEvent } from '../../types';
import { MIN_VISUAL_MS } from './calendarLayout';

/** Lane geometry returned by both the generic and agenda-specific layout functions. */
export interface RangeLayoutItem<T> {
  item: T;
  overlapIndex: number;
  overlapCount: number;
  leftPercent: number;
  widthPercent: number;
}

/** Convenience alias for the agenda-item variant consumed by ChunkBlock. */
export type OverlapLayoutItem = RangeLayoutItem<AgendaItem>;

interface IndexedItem<T> {
  item: T;
  index: number;
  startMs: number;
  endMs: number;
}

/**
 * Effective range an item occupies on screen — its real start to the later of its
 * real end or start + minVisualMs (the minimum drawn box). Used for overlap
 * detection only; the block still positions itself from the real times, so two
 * items lane side by side exactly when their drawn boxes would collide.
 */
function visualRange(
  startMs: number,
  endMs: number,
  minVisualMs: number,
): { startMs: number; endMs: number } {
  return { startMs, endMs: Math.max(endMs, startMs + minVisualMs) };
}

/**
 * Generic lane layout for any sequence of items with a start/end time.
 *
 * `getStartMs` / `getEndMs` extract millisecond timestamps from each item.
 * `minVisualMs` is the minimum on-screen span an item occupies (defaults to the
 * fixed calendar scale — see calendarLayout.ts). Pass a scale-adjusted value if
 * the grid is ever zoomed, so collision detection matches what is actually painted.
 *
 * `getLaneGroup` optionally splits each overlap cluster into ordered bands:
 * items in a lower group take the leftmost lanes of their cluster and never
 * share a lane with another group; each group lanes internally by first-fit.
 * Without it, every item competes for the same lanes.
 */
export function layoutOverlappingRanges<T>(
  items: T[],
  getStartMs: (item: T) => number,
  getEndMs: (item: T) => number,
  minVisualMs: number = MIN_VISUAL_MS,
  getLaneGroup?: (item: T) => number,
): RangeLayoutItem<T>[] {
  if (items.length === 0) return [];

  const indexedItems: IndexedItem<T>[] = items
    .map((item, index) => {
      const startMs = getStartMs(item);
      const endMs = getEndMs(item);
      const range = visualRange(startMs, endMs, minVisualMs);
      return {
        item,
        index,
        startMs: range.startMs,
        endMs: range.endMs,
      };
    })
    .sort((a, b) => {
      if (a.startMs !== b.startMs) return a.startMs - b.startMs;
      if (a.endMs !== b.endMs) return a.endMs - b.endMs;
      return a.index - b.index;
    });

  const layouts: Array<RangeLayoutItem<T> | undefined> = new Array(items.length);
  let cluster: IndexedItem<T>[] = [];
  let clusterEndMs = Number.NEGATIVE_INFINITY;

  function assignCluster(itemsInCluster: IndexedItem<T>[]): void {
    if (itemsInCluster.length === 0) return;

    // Partition into lane groups; sweep order (and thus first-fit correctness)
    // is preserved inside each group because the cluster is already sorted.
    const grouped = new Map<number, IndexedItem<T>[]>();
    for (const entry of itemsInCluster) {
      const group = getLaneGroup ? getLaneGroup(entry.item) : 0;
      const bucket = grouped.get(group);
      if (bucket) {
        bucket.push(entry);
      } else {
        grouped.set(group, [entry]);
      }
    }

    const laneByIndex = new Map<number, number>();
    let laneCount = 0;
    for (const group of [...grouped.keys()].sort((a, b) => a - b)) {
      const laneEnds: number[] = [];
      for (const entry of grouped.get(group)!) {
        const laneIndex = laneEnds.findIndex((endMs) => endMs <= entry.startMs);
        if (laneIndex === -1) {
          laneByIndex.set(entry.index, laneCount + laneEnds.length);
          laneEnds.push(entry.endMs);
          continue;
        }

        laneByIndex.set(entry.index, laneCount + laneIndex);
        laneEnds[laneIndex] = entry.endMs;
      }
      laneCount += laneEnds.length;
    }

    const overlapCount = Math.max(1, laneCount);
    const widthPercent = 100 / overlapCount;

    for (const entry of itemsInCluster) {
      const overlapIndex = laneByIndex.get(entry.index) ?? 0;
      layouts[entry.index] = {
        item: entry.item,
        overlapIndex,
        overlapCount,
        leftPercent: overlapIndex * widthPercent,
        widthPercent,
      };
    }
  }

  for (const entry of indexedItems) {
    if (cluster.length === 0) {
      cluster = [entry];
      clusterEndMs = entry.endMs;
      continue;
    }

    if (entry.startMs < clusterEndMs) {
      cluster.push(entry);
      clusterEndMs = Math.max(clusterEndMs, entry.endMs);
      continue;
    }

    assignCluster(cluster);
    cluster = [entry];
    clusterEndMs = entry.endMs;
  }

  assignCluster(cluster);

  return layouts.map((layout, index) => {
    if (layout) return layout;

    const item = items[index];
    return {
      item,
      overlapIndex: 0,
      overlapCount: 1,
      leftPercent: 0,
      widthPercent: 100,
    };
  });
}

/** Combined lane layout for one day column: chunks and external events share lanes. */
export interface DayColumnLayout {
  chunks: OverlapLayoutItem[];
  externals: RangeLayoutItem<ExternalEvent>[];
}

type DayColumnEntry =
  | { kind: 'external'; startMs: number; endMs: number; event: ExternalEvent }
  | { kind: 'chunk'; startMs: number; endMs: number; agendaItem: AgendaItem };

function extractTimeRange(obj: { start_time: string; end_time: string }): {
  startMs: number;
  endMs: number;
} {
  return {
    startMs: new Date(obj.start_time).getTime(),
    endMs: new Date(obj.end_time).getTime(),
  };
}

/**
 * Lay out one day column so chunks AND external events lane together: when the
 * two populations overlap, external events take the leftmost lanes and chunks
 * shift right, mirroring how overlapping chunks already share the column.
 *
 * `minVisualMs` is the minimum on-screen span an item occupies (defaults to the
 * fixed calendar scale — see calendarLayout.ts). Pass a scale-adjusted value if
 * the grid is ever zoomed, so collision detection matches what is actually painted.
 */
export function layoutDayColumn(
  items: AgendaItem[],
  events: ExternalEvent[],
  minVisualMs: number = MIN_VISUAL_MS,
): DayColumnLayout {
  const merged: DayColumnEntry[] = [
    ...events.map(
      (event): DayColumnEntry => ({
        kind: 'external',
        ...extractTimeRange(event),
        event,
      }),
    ),
    ...items.map(
      (agendaItem): DayColumnEntry => ({
        kind: 'chunk',
        ...extractTimeRange(agendaItem.chunk),
        agendaItem,
      }),
    ),
  ];

  const laid = layoutOverlappingRanges(
    merged,
    (entry) => entry.startMs,
    (entry) => entry.endMs,
    minVisualMs,
    (entry) => (entry.kind === 'external' ? 0 : 1),
  );

  const chunks: OverlapLayoutItem[] = [];
  const externals: RangeLayoutItem<ExternalEvent>[] = [];
  for (const layout of laid) {
    const { item: entry, ...lane } = layout;
    if (entry.kind === 'external') {
      externals.push({ item: entry.event, ...lane });
    } else {
      chunks.push({ item: entry.agendaItem, ...lane });
    }
  }

  return { chunks, externals };
}
