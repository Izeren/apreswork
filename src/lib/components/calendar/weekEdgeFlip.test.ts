// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import {
  edgeDirection,
  EdgeFlipController,
  WEEK_EDGE_ZONE_PX,
  WEEK_FLIP_DWELL_MS,
  type FlipDirection,
} from './weekEdgeFlip';

describe('edgeDirection', () => {
  const bounds = { left: 100, right: 800 };
  const zone = 40;

  const cases: { clientX: number; expected: FlipDirection | null; label: string }[] = [
    { clientX: 450, expected: null, label: 'middle is neutral' },
    { clientX: 100, expected: -1, label: 'exact left edge' },
    { clientX: 140, expected: -1, label: 'right at left zone boundary' },
    { clientX: 141, expected: null, label: 'just inside of left zone' },
    { clientX: 50, expected: -1, label: 'past the left edge' },
    { clientX: 800, expected: 1, label: 'exact right edge' },
    { clientX: 760, expected: 1, label: 'right at right zone boundary' },
    { clientX: 759, expected: null, label: 'just inside of right zone' },
    { clientX: 900, expected: 1, label: 'past the right edge' },
  ];

  it.each(cases)('clientX=$clientX → $expected ($label)', ({ clientX, expected }) => {
    expect(edgeDirection(clientX, bounds, zone)).toBe(expected);
  });

  it('accepts a DOMRect as bounds', () => {
    const rect = new DOMRect(100, 0, 700, 500); // left=100, right=800
    expect(edgeDirection(120, rect, zone)).toBe(-1);
    expect(edgeDirection(790, rect, zone)).toBe(1);
    expect(edgeDirection(450, rect, zone)).toBeNull();
  });

  it('exposes sensible default constants', () => {
    expect(WEEK_EDGE_ZONE_PX).toBe(40);
    expect(WEEK_FLIP_DWELL_MS).toBe(500);
  });
});

describe('EdgeFlipController', () => {
  const DWELL = 500;
  let onFlip: Mock<(direction: FlipDirection) => void>;
  let controller: EdgeFlipController;

  beforeEach(() => {
    vi.useFakeTimers();
    onFlip = vi.fn<(direction: FlipDirection) => void>();
    controller = new EdgeFlipController(DWELL, onFlip);
  });

  afterEach(() => {
    controller.stop();
    vi.useRealTimers();
  });

  it('does not flip before the dwell elapses', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL - 1);
    expect(onFlip).not.toHaveBeenCalled();
  });

  it('flips once the dwell elapses', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL);
    expect(onFlip).toHaveBeenCalledExactlyOnceWith(1);
  });

  it('flips toward the armed direction', () => {
    controller.update(-1);
    vi.advanceTimersByTime(DWELL);
    expect(onFlip).toHaveBeenCalledExactlyOnceWith(-1);
  });

  it('a sustained hold flips one week per dwell (timer resets)', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL * 3);
    // 3 dwells → 3 flips, one at a time, not all at once.
    expect(onFlip).toHaveBeenCalledTimes(3);
    expect(onFlip).toHaveBeenNthCalledWith(1, 1);
    expect(onFlip).toHaveBeenNthCalledWith(3, 1);
  });

  it('re-feeding the same direction does not restart the dwell', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL - 100);
    controller.update(1); // same direction — should not reset the countdown
    vi.advanceTimersByTime(100);
    expect(onFlip).toHaveBeenCalledTimes(1);
  });

  it('leaving the edge (null) cancels a pending flip', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL - 100);
    controller.update(null);
    vi.advanceTimersByTime(DWELL);
    expect(onFlip).not.toHaveBeenCalled();
  });

  it('switching to the other edge re-arms the dwell', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL - 100);
    controller.update(-1); // switch before the first fires
    vi.advanceTimersByTime(100);
    expect(onFlip).not.toHaveBeenCalled(); // old timer was cancelled
    vi.advanceTimersByTime(DWELL - 100);
    expect(onFlip).toHaveBeenCalledExactlyOnceWith(-1);
  });

  it('stop() cancels a pending flip', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL - 100);
    controller.stop();
    vi.advanceTimersByTime(DWELL);
    expect(onFlip).not.toHaveBeenCalled();
  });

  it('stop() halts the repeating flips after a hold', () => {
    controller.update(1);
    vi.advanceTimersByTime(DWELL);
    expect(onFlip).toHaveBeenCalledTimes(1);
    controller.stop();
    vi.advanceTimersByTime(DWELL * 5);
    expect(onFlip).toHaveBeenCalledTimes(1);
  });
});
