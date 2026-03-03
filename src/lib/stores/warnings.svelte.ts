// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { ScheduleWarning } from '../types';

export class WarningState {
  items: ScheduleWarning[] = $state([]);

  count: number = $derived(this.items.length);

  /**
   * Severity mix for the sidebar indicator: `Unschedulable` means the task
   * cannot be placed at all (blocking, danger color); `DeadlineViolation`
   * still schedules, just late (warning color).
   */
  hasBlocking: boolean = $derived(this.items.some((w) => 'Unschedulable' in w.kind));

  set(warnings: ScheduleWarning[]): void {
    this.items = warnings;
  }

  clear(): void {
    this.items = [];
  }
}

export const warningState = new WarningState();
