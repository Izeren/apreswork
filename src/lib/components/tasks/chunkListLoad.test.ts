// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Chunk } from '../../types';
import { busyCallSequence, chunkFixture, flushPromises, lastToastError } from '../../testFixtures';

const toastMod = await import('../../stores/toast.svelte');
const { loadChunkList } = await import('./chunkListLoad');

describe('loadChunkList', () => {
  let setChunks: ReturnType<typeof vi.fn<(chunks: Chunk[]) => void>>;
  let setLoading: ReturnType<typeof vi.fn<(loading: boolean) => void>>;
  let fetchChunks: ReturnType<typeof vi.fn<(id: string) => Promise<Chunk[]>>>;

  beforeEach(() => {
    setChunks = vi.fn<(chunks: Chunk[]) => void>();
    setLoading = vi.fn<(loading: boolean) => void>();
    fetchChunks = vi.fn<(id: string) => Promise<Chunk[]>>().mockResolvedValue([]);
    toastMod.toastState.items = [];
  });

  async function rejectAndRun(error: unknown): Promise<void> {
    fetchChunks.mockRejectedValue(error);
    loadChunkList('task-1', setChunks, setLoading, fetchChunks);
    await flushPromises();
  }

  describe('success path', () => {
    it('calls setLoading(true) synchronously before the API call', () => {
      loadChunkList('task-1', setChunks, setLoading, fetchChunks);
      expect(setLoading).toHaveBeenCalledWith(true);
    });

    it('passes the fetched chunks to setChunks', async () => {
      const chunks = [chunkFixture(), chunkFixture({ id: 'chunk-2' })];
      fetchChunks.mockResolvedValue(chunks);

      loadChunkList('task-1', setChunks, setLoading, fetchChunks);
      await flushPromises();

      expect(setChunks).toHaveBeenCalledWith(chunks);
    });

    it('calls fetchChunks with the given taskId', async () => {
      loadChunkList('task-abc', setChunks, setLoading, fetchChunks);
      await flushPromises();

      expect(fetchChunks).toHaveBeenCalledWith('task-abc');
    });
  });

  describe('when it fails', () => {
    it.each([
      {
        label: 'generic error',
        error: new Error('network'),
        expectedMessage: 'Failed to load chunks',
      },
      {
        label: 'validation error message verbatim',
        error: { error: 'validation', message: 'task not found' },
        expectedMessage: 'task not found',
      },
    ])('error toast: $label', async ({ error, expectedMessage }) => {
      await rejectAndRun(error);
      expect(lastToastError(toastMod.toastState.items)).toBe(expectedMessage);
    });

    it('does not call setChunks on failure', async () => {
      await rejectAndRun(new Error('network'));

      expect(setChunks).not.toHaveBeenCalled();
    });
  });

  describe('setLoading call sequence', () => {
    it.each([
      {
        label: 'success path',
        run: async () => {
          loadChunkList('task-1', setChunks, setLoading, fetchChunks);
          await flushPromises();
        },
      },
      {
        label: 'failure path',
        run: async () => {
          await rejectAndRun(new Error('network'));
        },
      },
    ])('always fires [true, false] on $label', async ({ run }) => {
      await run();
      expect(busyCallSequence(setLoading)).toEqual([true, false]);
    });
  });
});
