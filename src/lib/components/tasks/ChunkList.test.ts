// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import type { Chunk } from '../../types';
import { chunkFixture } from '../../testFixtures';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importChunkList() {
  const mod = await import('./ChunkList.svelte');
  return mod.default;
}

const trailingSnippet = createRawSnippet<[Chunk]>((getChunk) => ({
  render: () => `<span class="chunk-trailing" data-chunk-id="${getChunk().id}"></span>`,
}));

const labelledTrailingSnippet = createRawSnippet<[Chunk]>((getChunk) => ({
  render: () => `<em class="chunk-trailing">${getChunk().id}</em>`,
}));

describe('ChunkList', () => {
  describe('loading state', () => {
    it('shows "Loading chunks…" while loading', async () => {
      const ChunkList = await importChunkList();
      const { container } = render(ChunkList, {
        chunks: [],
        loading: true,
        trailing: trailingSnippet,
      });
      expect(container.querySelector('.chunks-state')?.textContent).toContain('Loading chunks');
    });

    it('does not render the list while loading', async () => {
      const ChunkList = await importChunkList();
      const { container } = render(ChunkList, {
        chunks: [chunkFixture()],
        loading: true,
        trailing: trailingSnippet,
      });
      expect(container.querySelector('.chunks-list')).toBeNull();
    });
  });

  describe('empty state', () => {
    it('shows "No chunks scheduled" when chunks is empty and not loading', async () => {
      const ChunkList = await importChunkList();
      const { container } = render(ChunkList, {
        chunks: [],
        loading: false,
        trailing: trailingSnippet,
      });
      const msg = container.querySelector('.chunks-state');
      expect(msg?.textContent).toContain('No chunks scheduled');
      expect(msg?.classList.contains('chunks-state--empty')).toBe(true);
    });

    it('does not render the list when chunks is empty', async () => {
      const ChunkList = await importChunkList();
      const { container } = render(ChunkList, {
        chunks: [],
        loading: false,
        trailing: trailingSnippet,
      });
      expect(container.querySelector('.chunks-list')).toBeNull();
    });
  });

  describe('populated state', () => {
    it('renders a list item for each chunk', async () => {
      const ChunkList = await importChunkList();
      const chunks = [chunkFixture(), chunkFixture({ id: 'chunk-2' })];
      const { container } = render(ChunkList, {
        chunks,
        loading: false,
        trailing: trailingSnippet,
      });
      expect(container.querySelectorAll('.chunk-item')).toHaveLength(2);
    });

    it('renders the trailing snippet with the correct chunk for each item', async () => {
      const ChunkList = await importChunkList();
      const chunks = [chunkFixture({ id: 'c-1' }), chunkFixture({ id: 'c-2' })];
      const { container } = render(ChunkList, {
        chunks,
        loading: false,
        trailing: trailingSnippet,
      });

      const items = container.querySelectorAll('.chunk-item');
      expect(items[0]?.querySelector('[data-chunk-id]')?.getAttribute('data-chunk-id')).toBe('c-1');
      expect(items[1]?.querySelector('[data-chunk-id]')?.getAttribute('data-chunk-id')).toBe('c-2');
    });

    it('shows start and end times formatted in each item', async () => {
      const ChunkList = await importChunkList();
      const chunk = chunkFixture({
        start_time: '2026-03-28T12:00:00.000Z',
        end_time: '2026-03-28T13:00:00.000Z',
      });
      const { container } = render(ChunkList, {
        chunks: [chunk],
        loading: false,
        trailing: trailingSnippet,
      });

      const timeSpan = container.querySelector('.chunk-time');
      expect(timeSpan).toBeTruthy();
      expect(timeSpan?.textContent).toContain('–');
    });

    it('aria-label on the list defaults to "Scheduled chunks"', async () => {
      const ChunkList = await importChunkList();
      const { container } = render(ChunkList, {
        chunks: [chunkFixture()],
        loading: false,
        trailing: trailingSnippet,
      });
      expect(container.querySelector('ul')?.getAttribute('aria-label')).toBe('Scheduled chunks');
    });
  });

  describe('label prop', () => {
    it.each([
      {
        testLabel: 'default (no label prop)',
        labelProp: undefined as string | undefined,
        expectedText: 'Scheduled chunks',
      },
      { testLabel: 'custom label', labelProp: 'Fixed chunks', expectedText: 'Fixed chunks' },
    ])(
      '$testLabel: section label and aria-label show "$expectedText"',
      async ({ labelProp, expectedText }) => {
        const ChunkList = await importChunkList();
        const { container } = render(ChunkList, {
          chunks: [chunkFixture()],
          loading: false,
          trailing: labelledTrailingSnippet,
          ...(labelProp !== undefined && { label: labelProp }),
        });
        expect(container.querySelector('.section-label')?.textContent).toBe(expectedText);
        expect(container.querySelector('ul')?.getAttribute('aria-label')).toBe(expectedText);
      },
    );
  });
});
