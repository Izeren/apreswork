// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi, type MockedFunction } from 'vitest';
import { chunkFixture as mockChunk } from '../../testFixtures';
import type { AgendaItem, Chunk, Task } from '../../types';
import type { CompletionFlowApi } from './completionFlow.svelte';
import { CompletionFlow } from './completionFlow.svelte';
import { toastState } from '../../stores/toast.svelte';

type FakeCompletionApi = {
  [K in keyof CompletionFlowApi]: MockedFunction<CompletionFlowApi[K]>;
};

const mockItem = (overrides?: Partial<Chunk>): AgendaItem => ({
  chunk: mockChunk(overrides),
  task_title: 'Test Task',
  task_priority: 'Medium',
  task_labels: [],
  task_recurring_template_id: null,
  task_deadline: null,
});

describe('CompletionFlow', () => {
  let fakeApi: FakeCompletionApi;
  let onchanged: MockedFunction<() => void>;
  let flow: CompletionFlow;

  beforeEach(() => {
    toastState.items = [];
    onchanged = vi.fn();
    fakeApi = {
      listChunksForTask: vi.fn<() => Promise<Chunk[]>>().mockResolvedValue([]),
      completeChunk: vi
        .fn<() => Promise<[Chunk, Task]>>()
        .mockResolvedValue([mockChunk(), {} as Task]),
      completeTask: vi.fn<() => Promise<Task>>().mockResolvedValue({} as Task),
      reopenChunk: vi
        .fn<() => Promise<[Chunk, Task]>>()
        .mockResolvedValue([mockChunk(), {} as Task]),
      apiErrorMessage: vi.fn<(e: unknown, f: string) => string>().mockImplementation((_e, f) => f),
    };
    flow = new CompletionFlow(onchanged, fakeApi);
  });

  describe('open — completed chunk (reopen path)', () => {
    it('reopens the chunk, toasts, and notifies', async () => {
      await flow.open(mockItem({ status: 'completed' }));

      expect(fakeApi.reopenChunk).toHaveBeenCalledWith('chunk-1');
      expect(toastState.items[0]?.text).toBe('Chunk reopened');
      expect(onchanged).toHaveBeenCalledOnce();
      expect(flow.dialogOpen).toBe(false);
    });

    it('shows an error toast and does not notify on failure', async () => {
      fakeApi.reopenChunk.mockRejectedValue(new Error('network'));

      await flow.open(mockItem({ status: 'completed' }));

      expect(toastState.items[0]?.level).toBe('error');
      expect(toastState.items[0]?.text).toBe('Failed to reopen chunk');
      expect(onchanged).not.toHaveBeenCalled();
    });
  });

  describe('open — last scheduled chunk (direct completion, B2)', () => {
    async function completeChunkWith(chunks: Chunk[]) {
      fakeApi.listChunksForTask.mockResolvedValue(chunks);

      await flow.open(mockItem());

      expect(fakeApi.completeChunk).toHaveBeenCalledWith('chunk-1');
    }

    it('completes the chunk (never the whole task) and notifies', async () => {
      await completeChunkWith([mockChunk()]);

      expect(fakeApi.completeTask).not.toHaveBeenCalled();
      expect(toastState.items[0]?.text).toBe('Chunk completed');
      expect(onchanged).toHaveBeenCalledOnce();
      expect(flow.dialogOpen).toBe(false);
    });

    it('ignores non-scheduled chunks when deciding "last"', async () => {
      await completeChunkWith([mockChunk(), mockChunk({ id: 'chunk-done', status: 'completed' })]);

      expect(flow.dialogOpen).toBe(false);
    });

    it('shows an error toast and keeps the dialog closed on failure', async () => {
      fakeApi.listChunksForTask.mockResolvedValue([mockChunk()]);
      fakeApi.completeChunk.mockRejectedValue(new Error('network'));

      await flow.open(mockItem());

      expect(toastState.items[0]?.level).toBe('error');
      expect(toastState.items[0]?.text).toBe('Failed to complete chunk');
      expect(onchanged).not.toHaveBeenCalled();
      expect(flow.dialogOpen).toBe(false);
    });
  });

  describe('open — dialog path', () => {
    it('opens the dialog with chunk preselected when several chunks are scheduled', async () => {
      fakeApi.listChunksForTask.mockResolvedValue([mockChunk(), mockChunk({ id: 'chunk-2' })]);

      await flow.open(mockItem());

      expect(flow.dialogOpen).toBe(true);
      expect(flow.target).toBe('chunk');
      expect(flow.item?.chunk.id).toBe('chunk-1');
      expect(fakeApi.completeChunk).not.toHaveBeenCalled();
      expect(fakeApi.completeTask).not.toHaveBeenCalled();
    });

    it('falls back to the dialog when the chunk list cannot be loaded', async () => {
      fakeApi.listChunksForTask.mockRejectedValue(new Error('network'));

      await flow.open(mockItem());

      expect(flow.dialogOpen).toBe(true);
      expect(fakeApi.completeChunk).not.toHaveBeenCalled();
    });
  });

  describe('confirm', () => {
    beforeEach(async () => {
      fakeApi.listChunksForTask.mockResolvedValue([mockChunk(), mockChunk({ id: 'chunk-2' })]);
      await flow.open(mockItem());
    });

    it('completes the chunk by default, resets, and notifies', async () => {
      await flow.confirm();

      expect(fakeApi.completeChunk).toHaveBeenCalledWith('chunk-1');
      expect(toastState.items[0]?.text).toBe('Chunk completed');
      expect(flow.dialogOpen).toBe(false);
      expect(flow.item).toBeNull();
      expect(onchanged).toHaveBeenCalledOnce();
    });

    it('completes the whole task when that target is selected', async () => {
      flow.selectTarget('task');

      await flow.confirm();

      expect(fakeApi.completeTask).toHaveBeenCalledWith('task-1');
      expect(toastState.items[0]?.text).toBe('Task completed');
      expect(flow.dialogOpen).toBe(false);
    });

    it('keeps the dialog open and toasts on failure', async () => {
      fakeApi.completeChunk.mockRejectedValue(new Error('network'));

      await flow.confirm();

      expect(toastState.items[0]?.level).toBe('error');
      expect(toastState.items[0]?.text).toBe('Failed to complete chunk');
      expect(flow.dialogOpen).toBe(true);
      expect(flow.busy).toBe(false);
      expect(onchanged).not.toHaveBeenCalled();
    });

    it('is a no-op without an item', async () => {
      flow.close();

      await flow.confirm();

      expect(fakeApi.completeChunk).not.toHaveBeenCalled();
      expect(fakeApi.completeTask).not.toHaveBeenCalled();
    });
  });

  describe('close', () => {
    it('resets the dialog state', async () => {
      fakeApi.listChunksForTask.mockResolvedValue([mockChunk(), mockChunk({ id: 'chunk-2' })]);
      await flow.open(mockItem());
      flow.selectTarget('task');

      flow.close();

      expect(flow.dialogOpen).toBe(false);
      expect(flow.item).toBeNull();
      expect(flow.target).toBe('chunk');
    });

    it('is a no-op while a completion is in flight', () => {
      flow.busy = true;
      flow.dialogOpen = true;

      flow.close();

      expect(flow.dialogOpen).toBe(true);
    });
  });
});
