// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import {
  clientYToTopPx,
  getCreateBounds,
  snapMinutes,
  topPxToIso,
  HOUR_HEIGHT_PX,
  SNAP_MINUTES,
  DAY_MINUTES,
  MIN_CHUNK_PX,
  DEFAULT_CREATE_CHUNK_PX,
} from './dragState.svelte';
import type { CreateInfo, ResizeInfo } from './dragState.svelte';

const MARCH_28 = new Date(2026, 2, 28);
const MARCH_29 = new Date(2026, 2, 29); // DST transition date

/** Fresh create-selection anchor at 9:00 on 2026-03-28, shared across describes. */
const baseCreateInfo = (): CreateInfo => ({
  anchorTopPx: 9 * HOUR_HEIGHT_PX,
  currentTopPx: 9 * HOUR_HEIGHT_PX,
  columnDate: MARCH_28,
});

/** Fresh resize info at 9:00–10:00 on 2026-03-28, shared across describes. */
const baseResizeInfo = (): ResizeInfo => ({
  chunkId: 'chunk-r1',
  taskTitle: 'Resize Task',
  originalStartTime: '2026-03-28T09:00:00Z',
  originalEndTime: '2026-03-28T10:00:00Z',
  originalHeightPx: HOUR_HEIGHT_PX,
  currentHeightPx: HOUR_HEIGHT_PX,
  topPx: 9 * HOUR_HEIGHT_PX,
  columnDate: MARCH_28,
});

describe('snapMinutes', () => {
  const cases = [
    { raw: 0, dur: 60, expected: 0, label: 'exact zero' },
    { raw: 60, dur: 60, expected: 60, label: 'exact on boundary' },
    { raw: 62, dur: 60, expected: 60, label: 'rounds down to nearest 5' },
    { raw: 63, dur: 60, expected: 65, label: 'rounds up to nearest 5' },
    { raw: 125, dur: 60, expected: 125, label: 'rounds to 125 (multiple of 5)' },
    { raw: 127, dur: 60, expected: 125, label: 'rounds down from 127' },
    { raw: 128, dur: 60, expected: 130, label: 'rounds up from 128' },
    { raw: -99999, dur: 60, expected: 0, label: 'very large negative clamps to 0' },
    { raw: 120, dur: 30, expected: 120, label: 'exact multiple of 5 stays unchanged' },
    { raw: 720, dur: 30, expected: 720, label: 'exact multiple of 5 stays unchanged (noon)' },
    { raw: -30, dur: 60, expected: 0, label: 'negative raw clamps to 0' },
    { raw: 1400, dur: 60, expected: 1380, label: 'clamps to day boundary' },
    {
      raw: DAY_MINUTES - 60,
      dur: 60,
      expected: DAY_MINUTES - 60,
      label: 'exact max start is allowed',
    },
    { raw: 542, dur: 30, expected: 540, label: 'fractional raw snaps to grid' },
    { raw: 99999, dur: 0, expected: DAY_MINUTES, label: 'zero duration clamps to DAY_MINUTES' },
  ];

  it.each(cases)('raw=$raw dur=$dur → $expected ($label)', ({ raw, dur, expected }) => {
    expect(snapMinutes(raw, dur)).toBe(expected);
  });

  it.each([
    { constant: SNAP_MINUTES, expected: 5, label: 'SNAP_MINUTES' },
    { constant: HOUR_HEIGHT_PX, expected: 60, label: 'HOUR_HEIGHT_PX' },
  ])('$label constant', ({ constant, expected }) => {
    expect(constant).toBe(expected);
  });
});

describe('topPxToIso', () => {
  // TZ=UTC (set in vitest config) keeps both sides of every assertion stable across environments.
  // hours + minutes are the human-readable input; topPx is derived so the test name shows the exact expected string.
  it.each([
    {
      label: 'midnight',
      hours: 0,
      minutes: 0,
      colDate: MARCH_28,
      expected: '2026-03-28T00:00:00.000Z',
    },
    {
      label: '9:00',
      hours: 9,
      minutes: 0,
      colDate: MARCH_28,
      expected: '2026-03-28T09:00:00.000Z',
    },
    {
      label: '9:30',
      hours: 9,
      minutes: 30,
      colDate: MARCH_28,
      expected: '2026-03-28T09:30:00.000Z',
    },
    {
      label: '23:00',
      hours: 23,
      minutes: 0,
      colDate: MARCH_28,
      expected: '2026-03-28T23:00:00.000Z',
    },
    {
      label: 'DST (2026-03-29)',
      hours: 9,
      minutes: 0,
      colDate: MARCH_29,
      expected: '2026-03-29T09:00:00.000Z',
    },
  ])('$label → $expected', ({ hours, minutes, colDate, expected }) => {
    const topPx = (hours + minutes / 60) * HOUR_HEIGHT_PX;
    expect(topPxToIso(topPx, colDate)).toBe(expected);
  });
});

describe('clientYToTopPx', () => {
  const PX_PER_SNAP = (SNAP_MINUTES / 60) * HOUR_HEIGHT_PX; // 5px per 5-minute slot
  // Smallest integer offset past the midpoint of a snap slot — guarantees rounding up.
  const PAST_MIDPOINT_PX = Math.ceil(PX_PER_SNAP / 2);

  it.each([
    {
      label: 'snaps to the nearest 5-minute grid line',
      gridTop: 0,
      clientY: HOUR_HEIGHT_PX + PAST_MIDPOINT_PX, // 3px into the 1h05m slot → rounds up
      expected: HOUR_HEIGHT_PX + PX_PER_SNAP, // 1h05m
    },
    {
      label: 'clamps to midnight when pointer is above the grid',
      gridTop: HOUR_HEIGHT_PX, // grid starts 1h down the page; pointer at Y=0 is above it
      clientY: 0,
      expected: 0,
    },
  ])('$label', ({ clientY, gridTop, expected }) => {
    const gridRect = new DOMRect(0, gridTop, 300, 24 * HOUR_HEIGHT_PX);
    expect(clientYToTopPx(clientY, gridRect)).toBe(expected);
  });
});

describe('getCreateBounds', () => {
  it.each([
    {
      label: 'no drag → default height',
      currentTopPx: 9 * HOUR_HEIGHT_PX,
      topPx: 9 * HOUR_HEIGHT_PX,
      heightPx: DEFAULT_CREATE_CHUNK_PX,
    },
    {
      label: 'downward drag',
      currentTopPx: 11 * HOUR_HEIGHT_PX,
      topPx: 9 * HOUR_HEIGHT_PX,
      heightPx: 2 * HOUR_HEIGHT_PX,
    },
    {
      label: 'upward drag normalizes',
      currentTopPx: 8 * HOUR_HEIGHT_PX,
      topPx: 8 * HOUR_HEIGHT_PX,
      heightPx: HOUR_HEIGHT_PX,
    },
  ])('$label', ({ currentTopPx, topPx, heightPx }) => {
    const info = { ...baseCreateInfo(), currentTopPx };
    expect(getCreateBounds(info)).toEqual({ topPx, heightPx });
  });
});

describe('DragState', () => {
  // We can't get truly fresh instances due to module caching, so test dragState singleton carefully.
  let dragState: import('./dragState.svelte').DragState;

  beforeEach(async () => {
    const mod = await import('./dragState.svelte');
    dragState = mod.dragState as import('./dragState.svelte').DragState;
    dragState.cancel();
    dragState.cancelResize();
    dragState.cancelCreate();
  });

  const baseDragInfo = (): import('./dragState.svelte').DragInfo => ({
    chunkId: 'chunk-1',
    taskId: 'task-1',
    taskTitle: 'Test Task',
    originalStartTime: '2026-03-28T09:00:00Z',
    originalEndTime: '2026-03-28T10:00:00Z',
    durationMs: 60 * 60 * 1000,
    currentTopPx: 9 * HOUR_HEIGHT_PX,
    heightPx: HOUR_HEIGHT_PX,
    offsetY: 10,
    columnDate: MARCH_28,
    pressClientX: 0,
    pressClientY: 0,
    moved: false,
  });

  describe('start / active', () => {
    it.each([
      {
        label: 'sets active after start()',
        start: () => dragState.start(baseDragInfo()),
        field: 'active' as const,
        chunkId: 'chunk-1',
      },
      {
        label: 'startResize sets resizing state',
        start: () => dragState.startResize(baseResizeInfo()),
        field: 'resizing' as const,
        chunkId: 'chunk-r1',
      },
    ])('$label', ({ start, field, chunkId }) => {
      start();
      expect(dragState[field]).not.toBeNull();
      expect(dragState[field]?.chunkId).toBe(chunkId);
    });

    it.each([{ field: 'active' }, { field: 'resizing' }, { field: 'creating' }] as const)(
      '$field is null initially',
      ({ field }) => {
        expect(dragState[field]).toBeNull();
      },
    );
  });

  describe('cancel', () => {
    it.each([
      {
        label: 'cancel sets active to null',
        start: () => dragState.start(baseDragInfo()),
        cancel: () => dragState.cancel(),
        field: 'active' as const,
      },
      {
        label: 'cancelResize clears resizing state',
        start: () => dragState.startResize(baseResizeInfo()),
        cancel: () => dragState.cancelResize(),
        field: 'resizing' as const,
      },
    ])('$label', ({ start, cancel, field }) => {
      start();
      cancel();
      expect(dragState[field]).toBeNull();
    });
  });

  describe('end', () => {
    it('returns the final DragInfo and clears active', () => {
      const info = baseDragInfo();
      dragState.start(info);
      const result = dragState.end();
      expect(result).not.toBeNull();
      expect(result?.chunkId).toBe('chunk-1');
      expect(dragState.active).toBeNull();
    });

    it('returns null when not dragging', () => {
      expect(dragState.end()).toBeNull();
    });
  });

  describe('updateColumn', () => {
    it('updates columnDate in active info', () => {
      dragState.start(baseDragInfo());
      const newDate = MARCH_29;
      dragState.updateColumn(newDate);
      expect(dragState.active?.columnDate?.getDate()).toBe(29);
    });
  });

  describe('updateMoved', () => {
    // Press is always at (100, 100); each case applies a sequence of pointer
    // positions and asserts the resulting sticky `moved` flag.
    const cases: { label: string; moves: [number, number][]; expected: boolean }[] = [
      { label: 'within the drag threshold stays unmoved', moves: [[102, 102]], expected: false },
      { label: 'past the drag threshold flags moved', moves: [[110, 100]], expected: true },
      {
        label: 'wandering away then back to the press point stays moved (sticky)',
        moves: [
          [160, 100],
          [100, 100],
        ],
        expected: true,
      },
    ];

    it.each(cases)('$label', ({ moves, expected }) => {
      dragState.start({ ...baseDragInfo(), pressClientX: 100, pressClientY: 100 });
      for (const [x, y] of moves) dragState.updateMoved(x, y);
      expect(dragState.active?.moved).toBe(expected);
    });
  });

  describe('updatePosition', () => {
    it.each([
      { label: 'snaps to 5-min grid', overrides: {}, clientY: 552, expected: 9 * HOUR_HEIGHT_PX },
      {
        label: 'clamps to 0 before midnight',
        overrides: { offsetY: 200 },
        clientY: 0,
        expected: 0,
      },
      {
        label: 'clamps end at 24:00',
        overrides: { durationMs: 60 * 60 * 1000, offsetY: 0 },
        clientY: 99999,
        expected: 23 * HOUR_HEIGHT_PX,
      },
    ])('$label', ({ overrides, clientY, expected }) => {
      dragState.start({ ...baseDragInfo(), ...overrides });
      const gridRect = new DOMRect(0, 0, 300, 24 * HOUR_HEIGHT_PX);
      dragState.updatePosition(clientY, gridRect);
      expect(dragState.active?.currentTopPx).toBe(expected);
    });
  });

  describe('no-op guards', () => {
    it.each([
      { label: 'cancel', invoke: () => dragState.cancel() },
      { label: 'updateColumn', invoke: () => dragState.updateColumn(MARCH_29) },
      { label: 'updateMoved', invoke: () => dragState.updateMoved(10, 10) },
      {
        label: 'updatePosition',
        invoke: () => dragState.updatePosition(100, new DOMRect(0, 0, 300, 24 * HOUR_HEIGHT_PX)),
      },
    ])('$label is a no-op when not dragging', ({ invoke }) => {
      expect(() => invoke()).not.toThrow();
      expect(dragState.active).toBeNull();
    });
  });

  describe('create selection', () => {
    it('startCreate sets creating state', () => {
      dragState.startCreate(baseCreateInfo());
      expect(dragState.creating).not.toBeNull();
      expect(dragState.creating?.anchorTopPx).toBe(9 * HOUR_HEIGHT_PX);
    });

    it('updateCreatePosition snaps the selection pointer', () => {
      dragState.startCreate(baseCreateInfo());
      const gridRect = new DOMRect(0, 0, 300, 24 * HOUR_HEIGHT_PX);
      dragState.updateCreatePosition(602, gridRect);
      expect(dragState.creating?.currentTopPx).toBe(600);
    });

    it('updateCreateColumn changes the target day', () => {
      dragState.startCreate(baseCreateInfo());
      const nextDay = MARCH_29;
      dragState.updateCreateColumn(nextDay);
      expect(dragState.creating?.columnDate?.getDate()).toBe(29);
    });

    it('endCreate returns the final snapshot and clears state', () => {
      dragState.startCreate(baseCreateInfo());
      const final = dragState.endCreate();
      expect(final).not.toBeNull();
      expect(dragState.creating).toBeNull();
    });
  });
});

describe('DragState — resize', () => {
  let dragState: import('./dragState.svelte').DragState;

  beforeEach(async () => {
    const mod = await import('./dragState.svelte');
    dragState = mod.dragState as import('./dragState.svelte').DragState;
    dragState.cancel();
    dragState.cancelResize();
    dragState.cancelCreate();
  });

  describe('updateResizePosition', () => {
    it.each([
      { label: 'snaps height to 5-min grid', clientY: 602, expected: 60 },
      {
        label: 'clamps min height to 5px',
        clientY: 9 * HOUR_HEIGHT_PX + 1,
        expected: MIN_CHUNK_PX,
      },
      { label: 'clamps max at 24h', clientY: 99999, expected: 900 },
    ])('$label', ({ clientY, expected }) => {
      dragState.startResize(baseResizeInfo());
      const gridRect = new DOMRect(0, 0, 300, 24 * HOUR_HEIGHT_PX);
      dragState.updateResizePosition(clientY, gridRect);
      expect(dragState.resizing?.currentHeightPx).toBe(expected);
    });
  });

  describe('endResize', () => {
    it('returns the final ResizeInfo and clears resizing', () => {
      dragState.startResize(baseResizeInfo());
      const final = dragState.endResize();
      expect(final).not.toBeNull();
      expect(final?.chunkId).toBe('chunk-r1');
      expect(dragState.resizing).toBeNull();
    });

    it('returns null when not resizing', () => {
      expect(dragState.endResize()).toBeNull();
    });
  });

  describe('no-op guards', () => {
    it.each([
      { label: 'cancelResize', invoke: () => dragState.cancelResize() },
      {
        label: 'updateResizePosition',
        invoke: () =>
          dragState.updateResizePosition(100, new DOMRect(0, 0, 300, 24 * HOUR_HEIGHT_PX)),
      },
    ])('$label is a no-op when not resizing', ({ invoke }) => {
      expect(() => invoke()).not.toThrow();
      expect(dragState.resizing).toBeNull();
    });
  });

  describe('independence from move', () => {
    it('move and resize can be started independently without interfering', () => {
      const mod_dragState: import('./dragState.svelte').DragState = dragState;
      const baseDrag: import('./dragState.svelte').DragInfo = {
        chunkId: 'chunk-m1',
        taskId: 'task-m1',
        taskTitle: 'Move Task',
        originalStartTime: '2026-03-28T08:00:00Z',
        originalEndTime: '2026-03-28T09:00:00Z',
        durationMs: 60 * 60 * 1000,
        currentTopPx: 8 * HOUR_HEIGHT_PX,
        heightPx: HOUR_HEIGHT_PX,
        offsetY: 0,
        columnDate: MARCH_28,
        pressClientX: 0,
        pressClientY: 0,
        moved: false,
      };
      mod_dragState.start(baseDrag);
      mod_dragState.startResize(baseResizeInfo());
      expect(mod_dragState.active?.chunkId).toBe('chunk-m1');
      expect(mod_dragState.resizing?.chunkId).toBe('chunk-r1');
      mod_dragState.cancel();
      expect(mod_dragState.resizing?.chunkId).toBe('chunk-r1');
    });
  });
});
