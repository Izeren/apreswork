// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Builds the inline-style string for a fixed-position popover.
 * `top`, `left`, and `width` are rounded to whole pixels; `maxHeight` is
 * appended verbatim so callers can pass either a `calc(...)` expression or a
 * plain pixel value.
 */
export function computePositioningStyle(
  top: number,
  left: number,
  width: number,
  maxHeight: string,
): string {
  return (
    `top:${Math.round(top)}px;` +
    `left:${Math.round(left)}px;` +
    `width:${Math.round(width)}px;` +
    `max-height:${maxHeight};`
  );
}
