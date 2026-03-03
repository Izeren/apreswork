// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Pixel scale of the calendar time grid — the single source of truth shared by
 * ChunkBlock (chunk sizing), dragState (drag/resize snapping) and overlapLayout
 * (overlap detection). Previously each of these hardcoded its own copy.
 *
 * These are FIXED today. If a zoom feature is ever added, HOUR_HEIGHT_PX becomes
 * dynamic (px-per-hour scales with the zoom level) and every consumer must read
 * the live scale instead of these constants. In particular MIN_VISUAL_MS below is
 * scale-dependent: it would silently report the wrong overlap threshold under
 * zoom. `layoutDayColumn` already takes the threshold as a parameter
 * precisely so a zoomed scale can be threaded through without touching this file.
 */

/** Pixel height per hour — must match the CSS custom property --calendar-hour-height. */
export const HOUR_HEIGHT_PX = 60;

/**
 * Horizontal insets and gap shared by ChunkBlock and ExternalEventBlock.
 * One definition — both block types import from here.
 */
export const COLUMN_INSET_LEFT_PX = 2;
export const COLUMN_INSET_RIGHT_PX = 12;
export const OVERLAP_GAP_PX = 4;

/** Smallest box height ChunkBlock paints for a chunk, whatever its real duration. */
export const CHUNK_MIN_HEIGHT_PX = 22;

export const TIME_LABEL_MIN_HEIGHT_PX = 42;

const MS_PER_HOUR = 60 * 60 * 1000;

/**
 * Minimum time span (ms) a chunk's drawn box covers: CHUNK_MIN_HEIGHT_PX worth of
 * minutes at the current scale. Overlap detection treats every chunk as at least
 * this long, so chunks whose painted boxes touch lane side by side even when their
 * real times don't overlap. Scale-dependent — see the zoom note above.
 */
export const MIN_VISUAL_MS = (CHUNK_MIN_HEIGHT_PX / HOUR_HEIGHT_PX) * MS_PER_HOUR;

export interface GridBlockStyleInput {
  topPx: number;
  heightPx: number;
  isOverlap: boolean;
  overlapIndex: number;
  overlapCount: number;
  zIndex: number | string;
  /** Extra CSS declarations (e.g. custom properties) appended after z-index. */
  extra?: string[];
}

/**
 * Inline positioning for an absolutely-positioned block in the timed
 * day-column grid — shared by ChunkBlock and ExternalEventBlock. Splits width
 * evenly across overlap lanes when the block shares a slot with siblings.
 */
export function computeGridBlockStyle(input: GridBlockStyleInput): string {
  const { topPx, heightPx, isOverlap, overlapIndex, overlapCount, zIndex, extra = [] } = input;
  const baseInsetPx = COLUMN_INSET_LEFT_PX + COLUMN_INSET_RIGHT_PX;

  if (!isOverlap) {
    return [
      `top: ${topPx}px`,
      `height: ${heightPx}px`,
      `left: ${COLUMN_INSET_LEFT_PX}px`,
      `width: calc(100% - ${baseInsetPx}px)`,
      `z-index: ${zIndex}`,
      ...extra,
    ].join('; ');
  }

  const totalGapPx = OVERLAP_GAP_PX * (overlapCount - 1);
  const availableWidth = `calc(100% - ${baseInsetPx + totalGapPx}px)`;
  const slotWidth = `calc(${availableWidth} / ${overlapCount})`;
  const left =
    overlapIndex === 0
      ? `${COLUMN_INSET_LEFT_PX}px`
      : `calc(${COLUMN_INSET_LEFT_PX}px + (${slotWidth} + ${OVERLAP_GAP_PX}px) * ${overlapIndex})`;

  return [
    `top: ${topPx}px`,
    `height: ${heightPx}px`,
    `left: ${left}`,
    `width: ${slotWidth}`,
    `z-index: ${zIndex}`,
    ...extra,
  ].join('; ');
}

export function timeToGridHeightPx(date: Date): number {
  return (date.getHours() + date.getMinutes() / 60) * HOUR_HEIGHT_PX;
}

export function addMillisecondsToIso(isoStart: string, durationMs: number): string {
  return new Date(new Date(isoStart).getTime() + durationMs).toISOString();
}

/**
 * Height in px of the past-time wash for a day column: full height for days
 * before today, up to the current time for today, 0 for future days.
 *
 * Both arguments are compared by local calendar date (year/month/day) so the
 * result is TZ-independent when the caller passes local Date objects.
 */
export function pastOverlayHeightPx(day: Date, now: Date): number {
  const dayStart = new Date(day.getFullYear(), day.getMonth(), day.getDate());
  const nowDayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if (dayStart < nowDayStart) return 24 * HOUR_HEIGHT_PX;
  if (dayStart > nowDayStart) return 0;
  return timeToGridHeightPx(now);
}
