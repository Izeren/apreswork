// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { Chunk } from '../../types';
import { listChunksForTask, apiErrorMessage } from '../../api';
import { toastState } from '../../stores/toast.svelte';

/**
 * Fetches chunks for a task, routing the result/error/loading state through the given setters.
 * Pass `fetchChunks` to substitute an injectable fake instead of the real api call.
 */
export function loadChunkList(
  taskId: string,
  setChunks: (chunks: Chunk[]) => void,
  setLoading: (loading: boolean) => void,
  fetchChunks: (id: string) => Promise<Chunk[]> = listChunksForTask,
): void {
  setLoading(true);
  fetchChunks(taskId)
    .then(setChunks)
    .catch((e: unknown) => {
      toastState.error(apiErrorMessage(e, 'Failed to load chunks'));
    })
    .finally(() => {
      setLoading(false);
    });
}
