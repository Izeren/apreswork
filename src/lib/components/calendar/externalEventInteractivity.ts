// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { ExternalEvent } from '../../types';

/**
 * Resolve the open handler for an external event, or null when it is read-only.
 *
 * An event is editable only when it lives on the configured editable (primary)
 * calendar — events from other calendars stay display-only. This is the single
 * definition of the primary-calendar editability policy shared by DayView and
 * WeekView (Architecture Invariant 2: one definition per policy).
 */
export function resolveEventOpenHandler(
  oneventopen: ((event: ExternalEvent) => void) | null,
  editableCalendarId: string | null,
  event: ExternalEvent,
): ((event: ExternalEvent) => void) | null {
  if (oneventopen && editableCalendarId !== null && event.calendar_id === editableCalendarId) {
    return oneventopen;
  }
  return null;
}
