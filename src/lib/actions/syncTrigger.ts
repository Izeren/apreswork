// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { SyncOutcome } from '../types';
import { syncSuccessText } from '../utils';
import { warningState } from '../stores/warnings.svelte';
import { toastState } from '../stores/toast.svelte';

export interface SyncApiSubset {
  syncNow: () => Promise<SyncOutcome>;
  syncErrorMessage: (e: unknown, fallback: string) => string;
}

/**
 * Runs a Google Calendar sync and reports the result — the one definition of
 * the sync trigger's warning/toast reporting (CLAUDE.md Architecture
 * Invariant #2). `setBusy` drives the caller's own busy-state signal;
 * `onSuccess`/`onError` let each caller refetch its own view of sync state.
 */
export function runSync(
  setBusy: (busy: boolean) => void,
  onSuccess: () => void,
  c: SyncApiSubset,
  onError?: () => void,
): void {
  setBusy(true);
  c.syncNow()
    .then((result) => {
      warningState.set(result.schedule.warnings);
      const pushed = result.pushed.created + result.pushed.updated + result.pushed.deleted;
      toastState.success(syncSuccessText(result.schedule.placed_chunks.length, pushed));
      onSuccess();
    })
    .catch((e) => {
      toastState.error(c.syncErrorMessage(e, 'Sync failed.'));
      onError?.();
    })
    .finally(() => {
      setBusy(false);
    });
}
