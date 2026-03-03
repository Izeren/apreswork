// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { ConfirmSpec } from './taskActions';

/**
 * Confirm-dialog host shared by every surface that gates a verb behind a
 * confirmation prompt (CalendarView, TaskListView, StatusView). Pass
 * `host.request` as `TaskActionsHost.confirm`; render `<ConfirmHostDialog {host} />`
 * once per surface.
 */
export interface ConfirmHost {
  readonly spec: ConfirmSpec | null;
  request: (spec: ConfirmSpec) => Promise<boolean>;
  settle: (confirmed: boolean) => void;
}

export function createConfirmHost(): ConfirmHost {
  let spec = $state<ConfirmSpec | null>(null);
  let resolve: ((confirmed: boolean) => void) | null = null;

  function request(next: ConfirmSpec): Promise<boolean> {
    spec = next;
    return new Promise((res) => {
      resolve = res;
    });
  }

  function settle(confirmed: boolean): void {
    spec = null;
    resolve?.(confirmed);
    resolve = null;
  }

  return {
    get spec() {
      return spec;
    },
    request,
    settle,
  };
}
