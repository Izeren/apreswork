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

/** Fresh create-selection anchor at 9:00 on 2026-03-28, shared across describes. */
const baseCreateInfo = (): CreateInfo => ({
  anchorTopPx: 9 * HOUR_HEIGHT_PX,
  currentTopPx: 9 * HOUR_HEIGHT_PX,
  columnDate: new Date(2026, 2, 28),
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

  it('SNAP_MINUTES constant is 5', () => {
    expect(SNAP_MINUTES).toBe(5);
  });

  it('HOUR_HEIGHT_PX constant is 60', () => {
    expect(HOUR_HEIGHT_PX).toBe(60);
  });
});

describe('topPxToIso', () => {
  function makeColDate(): Date {
    return new Date(2026, 2, 28, 0, 0, 0, 0);
  }

  it.each([
    { label: '0:00', topPx: 0, hours: 0, minutes: 0 },
    { label: '9:00', topPx: 9 * HOUR_HEIGHT_PX, hours: 9, minutes: 0 },
    { label: '9:30', topPx: 9.5 * HOUR_HEIGHT_PX, hours: 9, minutes: 30 },
    { label: '23:00', topPx: 23 * HOUR_HEIGHT_PX, hours: 23, minutes: 0 },
  ])('returns ISO string at $label for topPx = $topPx', ({ topPx, hours, minutes }) => {
    const d = new Date(topPxToIso(topPx, makeColDate()));
    expect(d.getHours()).toBe(hours);
    expect(d.getMinutes()).toBe(minutes);
  });

  it('preserves the calendar date from columnDate', () => {
    const col = new Date(2026, 2, 28, 0, 0, 0, 0); // March 28
    const iso = topPxToIso(10 * HOUR_HEIGHT_PX, col);
    const d = new Date(iso);
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(2); // March (0-indexed)
    expect(d.getDate()).toBe(28);
  });

  it('returns a valid ISO string (parseable)', () => {
    const col = makeColDate();
    const iso = topPxToIso(6 * HOUR_HEIGHT_PX, col);
    expect(isNaN(new Date(iso).getTime())).toBe(false);
  });

  it('matches local calendar semantics on DST transition dates', () => {
    const col = new Date(2026, 2, 29, 0, 0, 0, 0);
    const iso = topPxToIso(9 * HOUR_HEIGHT_PX, col);
    const expected = new Date(2026, 2, 29, 9, 0, 0, 0).toISOString();
    expect(iso).toBe(expected);
  });
});

describe('clientYToTopPx', () => {
  it('snaps a pointer coordinate to the nearest 5-minute grid line', () => {
    const gridRect = new DOMRect(0, 0, 300, 24 * HOUR_HEIGHT_PX);
    expect(clientYToTopPx(63, gridRect)).toBe(65);
  });

  it('clamps to midnight when pointer is above the grid', () => {
    const gridRect = new DOMRect(0, 100, 300, 24 * HOUR_HEIGHT_PX);
    expect(clientYToTopPx(0, gridRect)).toBe(0);
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
    // Use the exported singleton but reset it
    dragState = mod.dragState as import('./dragState.svelte').DragState;
    dragState.cancel();
    dragState.cancelResize();
    dragState.cancelCreate();
    dragState.lastEnded = null;
  });

  const baseDragInfo = (): import('./dragState.svelte').DragInfo => ({
    chunkId: 'chunk-1',
    taskTitle: 'Test Task',
    originalStartTime: '2026-03-28T09:00:00Z',
    originalEndTime: '2026-03-28T10:00:00Z',
    durationMs: 60 * 60 * 1000,
    currentTopPx: 9 * HOUR_HEIGHT_PX,
    heightPx: HOUR_HEIGHT_PX,
    offsetY: 10,
    columnDate: new Date(2026, 2, 28),
    pressClientX: 0,
    pressClientY: 0,
    moved: false,
  });

  describe('start / active', () => {
    it('sets active after start()', () => {
      dragState.start(baseDragInfo());
      expect(dragState.active).not.toBeNull();
      expect(dragState.active?.chunkId).toBe('chunk-1');
    });

    it('active is null initially', () => {
      expect(dragState.active).toBeNull();
    });
  });

  describe('cancel', () => {
    it('sets active to null', () => {
      dragState.start(baseDragInfo());
      dragState.cancel();
      expect(dragState.active).toBeNull();
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

  describe('lastEnded', () => {
    // WeekView drives moves from a container it captures the pointer on, so the
    // chunk's own DOM element never moves and the browser's follow-up click still
    // lands on it (pointer capture retargets pointer events, not click). lastEnded
    // is the shared signal a chunk's own click handler consults to tell a real
    // drag apart from a click when its own pointerup handler never ran.
    // Each case replays a sequence of drag operations and asserts the signal left
    // behind ('startMoved' = a drag already past the drag threshold).
    const cases: {
      label: string;
      ops: ('start' | 'startMoved' | 'end')[];
      expected: { chunkId: string; moved: boolean } | null;
    }[] = [
      { label: 'is null initially', ops: [], expected: null },
      {
        label: 'records moved: true after a drag that crossed the threshold',
        ops: ['startMoved', 'end'],
        expected: { chunkId: 'chunk-1', moved: true },
      },
      {
        label: 'records moved: false after a release that never crossed the threshold',
        ops: ['start', 'end'],
        expected: { chunkId: 'chunk-1', moved: false },
      },
      {
        label: 'is cleared by the next start(), so it cannot outlive a later unrelated click',
        ops: ['startMoved', 'end', 'start'],
        expected: null,
      },
      {
        label: 'is left untouched when end() is called with nothing active',
        ops: ['end'],
        expected: null,
      },
    ];

    it.each(cases)('$label', ({ ops, expected }) => {
      for (const op of ops) {
        if (op === 'end') dragState.end();
        else dragState.start({ ...baseDragInfo(), moved: op === 'startMoved' });
      }
      expect(dragState.lastEnded).toEqual(expected);
    });
  });

  describe('updateColumn', () => {
    it('updates columnDate in active info', () => {
      dragState.start(baseDragInfo());
      const newDate = new Date(2026, 2, 29);
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
      { label: 'updateColumn', invoke: () => dragState.updateColumn(new Date()) },
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
      const nextDay = new Date(2026, 2, 29);
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

  const baseResizeInfo = (): ResizeInfo => ({
    chunkId: 'chunk-r1',
    taskTitle: 'Resize Task',
    originalStartTime: '2026-03-28T09:00:00Z',
    originalEndTime: '2026-03-28T10:00:00Z',
    originalHeightPx: HOUR_HEIGHT_PX,
    currentHeightPx: HOUR_HEIGHT_PX,
    topPx: 9 * HOUR_HEIGHT_PX,
    columnDate: new Date(2026, 2, 28),
  });

  beforeEach(async () => {
    const mod = await import('./dragState.svelte');
    dragState = mod.dragState as import('./dragState.svelte').DragState;
    dragState.cancel();
    dragState.cancelResize();
  });

  describe('startResize / resizing', () => {
    it('resizing is null initially', () => {
      expect(dragState.resizing).toBeNull();
    });

    it('startResize sets resizing state', () => {
      dragState.startResize(baseResizeInfo());
      expect(dragState.resizing).not.toBeNull();
      expect(dragState.resizing?.chunkId).toBe('chunk-r1');
    });
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

  describe('cancelResize', () => {
    it('clears resizing state', () => {
      dragState.startResize(baseResizeInfo());
      dragState.cancelResize();
      expect(dragState.resizing).toBeNull();
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
        taskTitle: 'Move Task',
        originalStartTime: '2026-03-28T08:00:00Z',
        originalEndTime: '2026-03-28T09:00:00Z',
        durationMs: 60 * 60 * 1000,
        currentTopPx: 8 * HOUR_HEIGHT_PX,
        heightPx: HOUR_HEIGHT_PX,
        offsetY: 0,
        columnDate: new Date(2026, 2, 28),
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
