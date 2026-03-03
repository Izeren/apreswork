// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Cross-view carrier: another view (e.g. the task form's chunk section) asks
 * the calendar to navigate to a chunk's date and flash it. Mirrors the
 * request-id + nonce pattern of taskState.requestTemplateEdit: CalendarView
 * consumes the nonce to jump the visible range; ChunkBlock derives its flash
 * highlight from `chunkId` until `clear()` runs.
 */
export class CalendarFocusState {
  chunkId: string | null = $state(null);
  startTime: string | null = $state(null);
  nonce: number = $state(0);

  request(chunkId: string, startTime: string): void {
    this.chunkId = chunkId;
    this.startTime = startTime;
    this.nonce += 1;
  }

  clear(): void {
    this.chunkId = null;
    this.startTime = null;
  }
}

export const calendarFocusState = new CalendarFocusState();
