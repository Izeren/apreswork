// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { ScheduleResult } from '../types';
import { warningState } from '../stores/warnings.svelte';
import { toastState } from '../stores/toast.svelte';

export interface RescheduleApiSubset {
  triggerReschedule: () => Promise<ScheduleResult>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
}

/**
 * Runs a reschedule and reports the result — the one definition of the
 * reschedule trigger's mode/immediacy (CLAUDE.md Architecture Invariant #2).
 * `setBusy` drives the caller's own busy-state signal; `onSuccess` refetches
 * the caller's visible range (a reschedule cascades to other chunks).
 */
export function runReschedule(
  setBusy: (busy: boolean) => void,
  onSuccess: () => void,
  c: RescheduleApiSubset,
): void {
  setBusy(true);
  c.triggerReschedule()
    .then((result) => {
      warningState.set(result.warnings);
      toastState.success('Reschedule complete');
      onSuccess();
    })
    .catch((e) => {
      toastState.error(c.apiErrorMessage(e, 'Reschedule failed'));
    })
    .finally(() => {
      setBusy(false);
    });
}
