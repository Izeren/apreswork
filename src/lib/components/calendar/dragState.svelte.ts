// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { HOUR_HEIGHT_PX } from './calendarLayout';

const SNAP_MINUTES = 5;

const DAY_MINUTES = 24 * 60;

const MIN_CHUNK_PX = 5;

const DRAG_THRESHOLD_PX = 4;

const DEFAULT_CREATE_CHUNK_MINUTES = 30;

const DEFAULT_CREATE_CHUNK_PX = (DEFAULT_CREATE_CHUNK_MINUTES / 60) * HOUR_HEIGHT_PX;

export interface DragInfo {
  chunkId: string;
  taskTitle: string;
  /** ISO string of chunk's original start time. */
  originalStartTime: string;
  /** ISO string of chunk's original end time. */
  originalEndTime: string;
  /** Duration in milliseconds — preserved during move. */
  durationMs: number;
  /** Current snapped top position in pixels. */
  currentTopPx: number;
  /** Chunk height in pixels — stays constant during move. */
  heightPx: number;
  /** Cursor Y offset within the chunk at drag start. */
  offsetY: number;
  /** Which day column the pointer is over (for week view). */
  columnDate: Date | null;
  /** Client X/Y where the pointer first went down — for click-vs-drag detection. */
  pressClientX: number;
  pressClientY: number;
  /** True once the pointer travels past the drag threshold — i.e. a real drag, not a click. */
  moved: boolean;
}

export interface ResizeInfo {
  chunkId: string;
  taskTitle: string;
  /** ISO string of chunk's original start time. */
  originalStartTime: string;
  /** ISO string of chunk's original end time. */
  originalEndTime: string;
  /** Original height in px at drag start. */
  originalHeightPx: number;
  /** Current snapped height in px. */
  currentHeightPx: number;
  /** Top position in px (stays fixed during resize). */
  topPx: number;
  /** Which day column. */
  columnDate: Date | null;
}

export interface CreateInfo {
  /** Snapped anchor position in px where the pointer first went down. */
  anchorTopPx: number;
  /** Current snapped pointer position in px. */
  currentTopPx: number;
  /** Which day column the selection currently targets. */
  columnDate: Date | null;
}

function snapMinutes(rawMinutes: number, durationMinutes: number): number {
  const snapped = Math.round(rawMinutes / SNAP_MINUTES) * SNAP_MINUTES;
  const maxStart = DAY_MINUTES - durationMinutes;
  return Math.max(0, Math.min(snapped, maxStart));
}

export function topPxToIso(topPx: number, columnDate: Date): string {
  const totalMinutes = Math.round((topPx / HOUR_HEIGHT_PX) * 60);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  // Build the time in local calendar space so DST transitions within the day are handled correctly.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- pure date conversion helper, not reactive state
  return new Date(
    columnDate.getFullYear(),
    columnDate.getMonth(),
    columnDate.getDate(),
    hours,
    minutes,
    0,
    0,
  ).toISOString();
}

/**
 * Which `.day-columns` child contains `clientX`, from live column rects, by its
 * `data-column-date` epoch attribute. Returns null when DOM layout is
 * unavailable (e.g. in jsdom unit tests) or no column matches.
 */
export function findColumnDateAt(container: Element, clientX: number): Date | null {
  for (const col of container.children) {
    const rect = col.getBoundingClientRect();
    if (clientX >= rect.left && clientX < rect.right) {
      const epoch = col.getAttribute('data-column-date');
      // eslint-disable-next-line svelte/prefer-svelte-reactivity -- pure date conversion helper, not reactive state
      if (epoch) return new Date(Number(epoch));
    }
  }
  return null;
}

export function clientYToTopPx(clientY: number, gridRect: DOMRect): number {
  const rawMinutes = ((clientY - gridRect.top) / HOUR_HEIGHT_PX) * 60;
  return (snapMinutes(rawMinutes, 0) / 60) * HOUR_HEIGHT_PX;
}

export function getCreateBounds(info: CreateInfo): { topPx: number; heightPx: number } {
  const topPx = Math.min(info.anchorTopPx, info.currentTopPx);
  const deltaPx = Math.abs(info.currentTopPx - info.anchorTopPx);
  const heightPx = deltaPx === 0 ? DEFAULT_CREATE_CHUNK_PX : Math.max(MIN_CHUNK_PX, deltaPx);
  return { topPx, heightPx };
}

export class DragState {
  active: DragInfo | null = $state(null);
  resizing: ResizeInfo | null = $state(null);
  creating: CreateInfo | null = $state(null);
  /**
   * chunkId + moved outcome of the most recently ended move drag. Whoever drives
   * the drag (the chunk itself, or a parent container capturing the pointer to
   * survive a week flip) calls end(); either way, the chunk's own click handler
   * reads this to tell a real drag from a click. This is needed because pointer
   * capture retargets pointer events but not the browser's follow-up click, which
   * still lands on the chunk's own (unmoved) element via normal hit-testing.
   */
  lastEnded: { chunkId: string; moved: boolean } | null = $state(null);

  start(info: DragInfo): void {
    this.lastEnded = null;
    this.active = info;
  }

  updatePosition(clientY: number, gridRect: DOMRect): void {
    if (!this.active) return;
    // getBoundingClientRect() already accounts for parent scroll — no scrollTop offset needed.
    const durationMinutes = this.active.durationMs / 60_000;
    const rawMinutes = ((clientY - gridRect.top - this.active.offsetY) / HOUR_HEIGHT_PX) * 60;
    const snapped = snapMinutes(rawMinutes, durationMinutes);
    this.active = { ...this.active, currentTopPx: (snapped / 60) * HOUR_HEIGHT_PX };
  }

  updateColumn(date: Date): void {
    if (!this.active) return;
    this.active = { ...this.active, columnDate: date };
  }

  updateMoved(clientX: number, clientY: number): void {
    if (!this.active || this.active.moved) return;
    const dx = clientX - this.active.pressClientX;
    const dy = clientY - this.active.pressClientY;
    if (Math.hypot(dx, dy) > DRAG_THRESHOLD_PX) {
      this.active = { ...this.active, moved: true };
    }
  }

  end(): DragInfo | null {
    const final = this.active;
    this.active = null;
    if (final) this.lastEnded = { chunkId: final.chunkId, moved: final.moved };
    return final;
  }

  cancel(): void {
    this.active = null;
  }

  startResize(info: ResizeInfo): void {
    this.resizing = info;
  }

  updateResizePosition(clientY: number, gridRect: DOMRect): void {
    if (!this.resizing) return;
    // Compute the raw bottom edge position in grid coordinates (pixels from grid top)
    const rawBottomPx = clientY - gridRect.top;
    const rawBottomMinutes = (rawBottomPx / HOUR_HEIGHT_PX) * 60;
    const snappedBottomMinutes = snapMinutes(rawBottomMinutes, 0);
    const topMinutes = (this.resizing.topPx / HOUR_HEIGHT_PX) * 60;
    const heightMinutes = snappedBottomMinutes - topMinutes;
    // Minimum 5 minutes (MIN_CHUNK_PX px), maximum to end of day
    const maxHeightMinutes = DAY_MINUTES - topMinutes;
    const clampedMinutes = Math.max(SNAP_MINUTES, Math.min(heightMinutes, maxHeightMinutes));
    const newHeightPx = (clampedMinutes / 60) * HOUR_HEIGHT_PX;
    this.resizing = { ...this.resizing, currentHeightPx: Math.max(MIN_CHUNK_PX, newHeightPx) };
  }

  endResize(): ResizeInfo | null {
    const final = this.resizing;
    this.resizing = null;
    return final;
  }

  cancelResize(): void {
    this.resizing = null;
  }

  startCreate(info: CreateInfo): void {
    this.creating = info;
  }

  updateCreatePosition(clientY: number, gridRect: DOMRect): void {
    if (!this.creating) return;
    this.creating = {
      ...this.creating,
      currentTopPx: clientYToTopPx(clientY, gridRect),
    };
  }

  updateCreateColumn(date: Date): void {
    if (!this.creating) return;
    this.creating = { ...this.creating, columnDate: date };
  }

  endCreate(): CreateInfo | null {
    const final = this.creating;
    this.creating = null;
    return final;
  }

  cancelCreate(): void {
    this.creating = null;
  }
}

export const dragState = new DragState();

export {
  snapMinutes,
  HOUR_HEIGHT_PX,
  SNAP_MINUTES,
  DAY_MINUTES,
  MIN_CHUNK_PX,
  DRAG_THRESHOLD_PX,
  DEFAULT_CREATE_CHUNK_MINUTES,
  DEFAULT_CREATE_CHUNK_PX,
};
