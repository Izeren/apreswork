// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Edge-dwell week flipping for cross-week drag.
//!
//! While a chunk is being dragged in the week view, holding the pointer near the
//! left (Monday) or right (Sunday) edge for a short dwell flips the calendar by
//! one week, then resets so a sustained hold advances one week at a time rather
//! than racing through several.

/** Direction of a week flip: -1 = previous week, +1 = next week. */
export type FlipDirection = -1 | 1;

/** Dwell time (ms) the pointer must rest at an edge before the week flips. */
export const WEEK_FLIP_DWELL_MS = 500;

/** Width (px) of the left/right trigger zones at the edges of the week grid. */
export const WEEK_EDGE_ZONE_PX = 40;

/** Horizontal bounds of the week grid (a `DOMRect` satisfies this). */
export interface EdgeBounds {
  left: number;
  right: number;
}

/**
 * Classify a pointer X coordinate against the week grid's edge zones.
 *
 * Returns `-1` when within the left zone (flip to the previous week), `1` when
 * within the right zone (next week), or `null` when in the neutral middle.
 * Coordinates beyond an edge count as inside that edge's zone, so dragging past
 * the grid still triggers.
 */
export function edgeDirection(
  clientX: number,
  bounds: EdgeBounds,
  zonePx: number,
): FlipDirection | null {
  if (clientX <= bounds.left + zonePx) return -1;
  if (clientX >= bounds.right - zonePx) return 1;
  return null;
}

/**
 * Dwell-timer state machine driving edge-triggered week flips.
 *
 * Feed the current edge direction via {@link update} on every pointer move.
 * When a direction is held continuously for `dwellMs`, `onFlip` fires and the
 * timer restarts (so holding flips one week per dwell). Moving to the other
 * edge re-arms; leaving the edge (or {@link stop}) cancels the pending flip.
 */
export class EdgeFlipController {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private armed: FlipDirection | null = null;

  constructor(
    private readonly dwellMs: number,
    private readonly onFlip: (direction: FlipDirection) => void,
  ) {}

  /**
   * Report the latest edge direction (`null` = pointer is in the neutral area).
   * Idempotent while the same direction is held — the in-flight dwell keeps
   * counting rather than restarting on every pointer move.
   */
  update(direction: FlipDirection | null): void {
    if (direction === null) {
      this.stop();
      return;
    }
    if (direction === this.armed) return;
    this.arm(direction);
  }

  /** Cancel any pending flip and disarm. Call on drag end or cancel. */
  stop(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.armed = null;
  }

  private arm(direction: FlipDirection): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.armed = direction;
    this.timer = setTimeout(() => this.fire(direction), this.dwellMs);
  }

  private fire(direction: FlipDirection): void {
    this.onFlip(direction);
    // Restart the dwell so a sustained hold advances one week at a time.
    this.timer = setTimeout(() => this.fire(direction), this.dwellMs);
  }
}
