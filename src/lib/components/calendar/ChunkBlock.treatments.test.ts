// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
// Visual state treatments split out of ChunkBlock.test.ts (file-length cap):
// past wash, focus flash, and the overdue deadline indicator.
import { describe, it, expect, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { ChunkStatus } from '../../types';
import { calendarFocusState } from '../../stores/calendarFocus.svelte';
import {
  baseItem,
  importChunkBlock,
  installChunkBlockCleanup,
  localISO,
} from './ChunkBlock.testHelpers';

installChunkBlockCleanup();

describe('ChunkBlock — past treatment', () => {
  // baseItem chunk: 09:00–10:00 on 2026-03-28 local
  it.each([
    {
      label: 'now after chunk end → is-past class present',
      now: new Date(2026, 2, 28, 11, 0), // 11:00, after 10:00 end
      expectIsPast: true,
    },
    {
      label: 'now before chunk end → no is-past class',
      now: new Date(2026, 2, 28, 9, 30), // 09:30, before 10:00 end
      expectIsPast: false,
    },
    {
      label: 'now exactly at chunk end → no is-past (not strictly before)',
      now: new Date(2026, 2, 28, 10, 0), // exactly 10:00
      expectIsPast: false,
    },
    {
      label: 'now null → no is-past class',
      now: null,
      expectIsPast: false,
    },
  ])('$label', async ({ now, expectIsPast }) => {
    const ChunkBlock = await importChunkBlock();
    const item = baseItem();
    const { container } = render(ChunkBlock, { item, now });
    const block = container.querySelector('.chunk-block');
    expect(block!.classList.contains('is-past')).toBe(expectIsPast);
  });

  it('root element carries title attribute equal to aria-label', async () => {
    const ChunkBlock = await importChunkBlock();
    const item = baseItem({ task_title: 'Focus Block' });
    const { container } = render(ChunkBlock, { item, now: new Date(2026, 2, 28, 12, 0) });
    const block = container.querySelector('.chunk-block');
    const ariaLabel = block!.getAttribute('aria-label');
    const title = block!.getAttribute('title');
    expect(title).toBeTruthy();
    expect(title).toBe(ariaLabel);
  });

  it('title attribute updates when chunk is fixed', async () => {
    const ChunkBlock = await importChunkBlock();
    const item = baseItem({
      task_title: 'Fixed Task',
      chunk: { ...baseItem().chunk, is_fixed: true },
    });
    const { container } = render(ChunkBlock, { item, now: new Date(2026, 2, 28, 12, 0) });
    const block = container.querySelector('.chunk-block');
    const title = block!.getAttribute('title') ?? '';
    expect(title).toContain('fixed');
  });
});

describe('ChunkBlock — focus flash', () => {
  afterEach(() => {
    calendarFocusState.clear();
  });

  it.each([
    {
      label: 'targeted chunk flashes and clears when store cleared',
      chunkId: 'chunk-1',
      check: async (container: HTMLElement) => {
        const block = container.querySelector('.chunk-block')!;
        expect(block.classList.contains('is-flashing')).toBe(true);
        calendarFocusState.clear();
        await tick();
        expect(block.classList.contains('is-flashing')).toBe(false);
      },
    },
    {
      label: 'untargeted chunk does not flash',
      chunkId: 'chunk-other',
      check: async (container: HTMLElement) => {
        expect(container.querySelector('.chunk-block')!.classList.contains('is-flashing')).toBe(
          false,
        );
      },
    },
  ])('$label', async ({ chunkId, check }) => {
    const ChunkBlock = await importChunkBlock();
    calendarFocusState.request(chunkId, localISO(9));
    const { container } = render(ChunkBlock, { item: baseItem() });
    await check(container);
  });
});

describe('ChunkBlock — overdue treatment', () => {
  // Use fixed UTC strings so deadline comparisons are timezone-independent.
  const CHUNK_START = '2026-03-28T09:00:00.000Z';
  const CHUNK_END = '2026-03-28T10:00:00.000Z';

  const overdueCases: Array<{
    label: string;
    deadline: string | null;
    status: ChunkStatus;
    isFixed?: boolean;
    expectOverdue: boolean;
  }> = [
    {
      label: 'deadline before end_time (scheduled) → overdue',
      deadline: '2026-03-28T09:59:00.000Z',
      status: 'scheduled',
      expectOverdue: true,
    },
    {
      label: 'null deadline → not overdue',
      deadline: null,
      status: 'scheduled',
      expectOverdue: false,
    },
    {
      label: 'deadline equal to end_time → not overdue (strict boundary)',
      deadline: CHUNK_END,
      status: 'scheduled',
      expectOverdue: false,
    },
    {
      label: 'deadline after end_time → not overdue',
      deadline: '2026-03-28T10:01:00.000Z',
      status: 'scheduled',
      expectOverdue: false,
    },
    {
      label: 'completed + deadline before end_time → not overdue',
      deadline: '2026-03-28T09:59:00.000Z',
      status: 'completed',
      expectOverdue: false,
    },
    {
      label: 'fixed scheduled + deadline before end_time → both is-fixed and is-overdue',
      deadline: '2026-03-28T09:59:00.000Z',
      status: 'scheduled',
      isFixed: true,
      expectOverdue: true,
    },
  ];

  it.each(overdueCases)('$label', async ({ deadline, status, isFixed, expectOverdue }) => {
    const ChunkBlock = await importChunkBlock();
    const item = baseItem({
      chunk: {
        ...baseItem().chunk,
        start_time: CHUNK_START,
        end_time: CHUNK_END,
        status,
        is_fixed: isFixed ?? false,
      },
      task_deadline: deadline,
    });
    const { container } = render(ChunkBlock, { item });
    const block = container.querySelector('.chunk-block');
    if (isFixed) {
      expect(block!.classList.contains('is-fixed')).toBe(true);
    }
    expect(block!.classList.contains('is-overdue')).toBe(expectOverdue);
    if (expectOverdue) {
      expect(block!.getAttribute('aria-label')).toContain('past deadline');
    } else {
      expect(block!.getAttribute('aria-label')).not.toContain('past deadline');
    }
  });
});
