// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { AgendaItem, Chunk, Task } from '../../types';
import { toastState } from '../../stores/toast.svelte';
import type { CompletionTarget } from './CompleteChunkDialog.svelte';

export interface CompletionFlowApi {
  listChunksForTask: (taskId: string) => Promise<Chunk[]>;
  completeChunk: (chunkId: string) => Promise<[Chunk, Task]>;
  completeTask: (taskId: string) => Promise<Task>;
  reopenChunk: (chunkId: string) => Promise<[Chunk, Task]>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
}

export class CompletionFlow {
  dialogOpen: boolean = $state(false);
  target: CompletionTarget = $state('chunk');
  item: AgendaItem | null = $state(null);
  busy: boolean = $state(false);

  /** Called after every successful mutation so the owner can refetch. */
  readonly #onchanged: () => void;
  readonly #api: CompletionFlowApi;

  constructor(onchanged: () => void, api: CompletionFlowApi) {
    this.#onchanged = onchanged;
    this.#api = api;
  }

  async #callAndNotify(
    apiCall: () => Promise<unknown>,
    successMsg: string,
    errorMsg: string,
  ): Promise<void> {
    try {
      await apiCall();
      toastState.success(successMsg);
      this.#onchanged();
    } catch (e) {
      toastState.error(this.#api.apiErrorMessage(e, errorMsg));
    }
  }

  /**
   * Handle a ✓ click on a chunk: reopen it if completed; complete it directly
   * if it is the task's only scheduled chunk (the chunk-vs-task choice would
   * be the only dialog content); otherwise open the dialog.
   *
   * The direct path completes the *chunk* — logging the planned block, not
   * the task's remaining budget. On an under-placed task the difference is
   * real: the backend requeues the remainder instead of silently logging it,
   * and it still auto-completes the task once logged time covers the duration.
   */
  async open(item: AgendaItem): Promise<void> {
    if (item.chunk.status === 'completed') {
      await this.#callAndNotify(
        () => this.#api.reopenChunk(item.chunk.id),
        'Chunk reopened',
        'Failed to reopen chunk',
      );
      return;
    }

    let isLastScheduledChunk = false;
    try {
      const scheduledIds = (await this.#api.listChunksForTask(item.chunk.task_id))
        .filter((chunk) => chunk.status === 'scheduled')
        .map((chunk) => chunk.id);
      isLastScheduledChunk = scheduledIds.length === 1 && scheduledIds[0] === item.chunk.id;
    } catch {
      // Fall back to the dialog if we cannot precompute the completion context.
    }

    if (isLastScheduledChunk) {
      await this.#callAndNotify(
        () => this.#api.completeChunk(item.chunk.id),
        'Chunk completed',
        'Failed to complete chunk',
      );
      return;
    }

    this.item = item;
    this.target = 'chunk';
    this.dialogOpen = true;
  }

  close(): void {
    if (this.busy) return;
    this.#reset();
  }

  selectTarget(target: CompletionTarget): void {
    this.target = target;
  }

  async confirm(): Promise<void> {
    if (!this.item) return;

    this.busy = true;
    try {
      if (this.target === 'task') {
        await this.#api.completeTask(this.item.chunk.task_id);
        toastState.success('Task completed');
      } else {
        await this.#api.completeChunk(this.item.chunk.id);
        toastState.success('Chunk completed');
      }

      this.#reset();
      this.#onchanged();
    } catch (e) {
      toastState.error(
        this.#api.apiErrorMessage(
          e,
          this.target === 'task' ? 'Failed to complete task' : 'Failed to complete chunk',
        ),
      );
    } finally {
      this.busy = false;
    }
  }

  #reset(): void {
    this.dialogOpen = false;
    this.item = null;
    this.target = 'chunk';
  }
}
