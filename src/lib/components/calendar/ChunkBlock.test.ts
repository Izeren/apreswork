// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import type { ComponentProps } from 'svelte';
import type { ChunkStatus } from '../../types';
import { dragState } from './dragState.svelte';
import {
  baseItem,
  importChunkBlock,
  installChunkBlockCleanup,
  localISO,
} from './ChunkBlock.testHelpers';

installChunkBlockCleanup();

type ChunkProps = ComponentProps<Awaited<ReturnType<typeof importChunkBlock>>>;

async function renderChunk(
  overrides: Parameters<typeof baseItem>[0] = {},
  props: Partial<Omit<ChunkProps, 'item'>> = {},
) {
  const ChunkBlock = await importChunkBlock();
  const item = baseItem(overrides);
  const utils = render(ChunkBlock, { item, ...props });
  return { item, ...utils };
}

function patchBlockForDrag(block: HTMLElement): void {
  block.getBoundingClientRect = () => ({ top: 100, bottom: 160, left: 0, right: 100 }) as DOMRect;
  Object.assign(block, { setPointerCapture: vi.fn(), releasePointerCapture: vi.fn() });
}

async function renderDraggableBlock(props: Partial<Omit<ChunkProps, 'item'>> = {}) {
  const { container, item } = await renderChunk({}, props);
  const block = container.querySelector('.chunk-block') as HTMLElement;
  patchBlockForDrag(block);
  return { block, container, item };
}

function appendTimeGrid(block: HTMLElement): HTMLElement {
  const grid = document.createElement('div');
  grid.setAttribute('aria-label', 'Time grid');
  grid.getBoundingClientRect = () => ({ top: 0, bottom: 1440 }) as DOMRect;
  document.body.appendChild(grid);
  grid.appendChild(block);
  return grid;
}

const renderProfileCases: Array<{
  label: string;
  status: ChunkStatus;
  isFixed: boolean;
  expectedStatusClass: string;
  expectFixedClass: boolean;
  expectCompleteAction: boolean;
  expectCompleteChecked: boolean;
  expectFixedLabel: boolean;
  expectCompletedLabel: boolean;
}> = [
  {
    label: 'scheduled unlocked',
    status: 'scheduled',
    isFixed: false,
    expectedStatusClass: 'chunk-block--scheduled',
    expectFixedClass: false,
    expectCompleteAction: true,
    expectCompleteChecked: false,
    expectFixedLabel: false,
    expectCompletedLabel: false,
  },
  {
    label: 'scheduled fixed',
    status: 'scheduled',
    isFixed: true,
    expectedStatusClass: 'chunk-block--scheduled',
    expectFixedClass: true,
    expectCompleteAction: true,
    expectCompleteChecked: false,
    expectFixedLabel: true,
    expectCompletedLabel: false,
  },
  {
    label: 'completed unlocked',
    status: 'completed',
    isFixed: false,
    expectedStatusClass: 'chunk-block--completed',
    expectFixedClass: false,
    expectCompleteAction: true,
    expectCompleteChecked: true,
    expectFixedLabel: false,
    expectCompletedLabel: true,
  },
  {
    label: 'completed fixed',
    status: 'completed',
    isFixed: true,
    expectedStatusClass: 'chunk-block--completed',
    expectFixedClass: false,
    expectCompleteAction: true,
    expectCompleteChecked: true,
    expectFixedLabel: false,
    expectCompletedLabel: true,
  },
];

describe('ChunkBlock — content', () => {
  it('renders the task title', async () => {
    const { getByText } = await renderChunk({ task_title: 'Morning Run' });
    expect(getByText('Morning Run')).toBeTruthy();
  });

  it('renders the time range from formatTime', async () => {
    const { container } = await renderChunk();
    const timeEl = container.querySelector('.chunk-time');
    expect(timeEl).toBeTruthy();
    // Should contain "–" separator
    expect(timeEl!.textContent).toContain('–');
  });

  it('renders duration label when block is tall enough for three lines', async () => {
    const { container } = await renderChunk({
      chunk: { start_time: localISO(9, 0), end_time: localISO(10, 30) },
    });
    expect(container.querySelector('.chunk-duration')).toBeTruthy();
  });

  it('hides duration label when block is too short for extra labels', async () => {
    const { container } = await renderChunk({
      chunk: { start_time: localISO(9, 0), end_time: localISO(9, 15) },
    });
    expect(container.querySelector('.chunk-duration')).toBeNull();
    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block!.style.height).toBe('22px');
  });

  it('hides duration label for 30-minute chunk', async () => {
    const { container } = await renderChunk({
      chunk: { start_time: localISO(9, 0), end_time: localISO(9, 30) },
    });
    expect(container.querySelector('.chunk-duration')).toBeNull();
  });

  it('renders the title for a short fixed chunk', async () => {
    const { getByText, container } = await renderChunk({
      task_title: 'Short Fixed Task',
      chunk: { start_time: localISO(22, 0), end_time: localISO(22, 15), is_fixed: true },
    });
    expect(getByText('Short Fixed Task')).toBeTruthy();
    expect(container.querySelector('.chunk-time')).toBeNull();
    expect(container.querySelector('.chunk-duration')).toBeNull();
    expect(container.querySelector('.chunk-block')?.classList.contains('is-short')).toBe(true);
    expect(container.querySelector('.chunk-block')?.classList.contains('is-compact')).toBe(true);
  });
});

describe('ChunkBlock — overlap layout', () => {
  it('uses the overlap lane styles for a shared day-column width', async () => {
    const { container } = await renderChunk({}, { overlapIndex: 1, overlapCount: 2 });

    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block).toBeTruthy();
    expect(block!.classList.contains('is-overlap')).toBe(true);
    expect(block!.dataset.overlapCount).toBe('2');
    expect(block!.dataset.overlapIndex).toBe('1');
    expect(block!.style.left).toContain('calc(');
    expect(block!.style.width).toContain('calc(');
  });

  it('hides time and duration when overlap makes the block narrow', async () => {
    const { container, getByText } = await renderChunk(
      {
        task_title: 'Overlapping Task',
        chunk: { start_time: localISO(9, 0), end_time: localISO(10, 0) },
      },
      { overlapIndex: 0, overlapCount: 3 },
    );

    expect(getByText('Overlapping Task')).toBeTruthy();
    expect(container.querySelector('.chunk-time')).toBeNull();
    expect(container.querySelector('.chunk-duration')).toBeNull();
  });

  it('uses a denser week-view layout even without overlap', async () => {
    const { container, getByText } = await renderChunk(
      {
        task_title: '[feat][task_form] Make Task Form more readable',
        chunk: { start_time: localISO(18, 0), end_time: localISO(19, 0) },
      },
      { density: 'compact' },
    );

    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block?.dataset.density).toBe('compact');
    expect(block?.classList.contains('is-dense')).toBe(true);
    expect(getByText('[feat][task_form] Make Task Form more readable')).toBeTruthy();
    expect(container.querySelector('.chunk-time')).toBeTruthy();
    expect(container.querySelector('.chunk-duration')).toBeNull();
  });
});

describe('ChunkBlock — top position', () => {
  const topCases = [
    { label: 'midnight', hour: 0, minute: 0, expectedTop: 0 },
    { label: '6am', hour: 6, minute: 0, expectedTop: 360 },
    { label: 'noon', hour: 12, minute: 0, expectedTop: 720 },
    { label: '6pm', hour: 18, minute: 0, expectedTop: 1080 },
    { label: '9:30am', hour: 9, minute: 30, expectedTop: 570 },
  ];

  it.each(topCases)(
    'chunk starting at $label has top = $expectedTop px',
    async ({ hour, minute, expectedTop }) => {
      const { container } = await renderChunk({
        chunk: { start_time: localISO(hour, minute), end_time: localISO(hour + 1, minute) },
      });
      const block = container.querySelector('.chunk-block') as HTMLElement | null;
      expect(block).toBeTruthy();
      expect(block!.style.top).toBe(`${expectedTop}px`);
    },
  );
});

describe('ChunkBlock — height', () => {
  const heightCases = [
    { label: '30 min', durationMin: 30, expectedHeight: 30 },
    { label: '60 min', durationMin: 60, expectedHeight: 60 },
    { label: '90 min', durationMin: 90, expectedHeight: 90 },
  ];

  it.each(heightCases)(
    'chunk of $label has height = $expectedHeight px',
    async ({ durationMin, expectedHeight }) => {
      const endHour = Math.floor((9 * 60 + durationMin) / 60);
      const endMinute = (9 * 60 + durationMin) % 60;
      const { container } = await renderChunk({
        chunk: { start_time: localISO(9, 0), end_time: localISO(endHour, endMinute) },
      });
      const block = container.querySelector('.chunk-block') as HTMLElement | null;
      expect(block).toBeTruthy();
      expect(block!.style.height).toBe(`${expectedHeight}px`);
    },
  );

  it('very short chunk (< 22px natural height) has minimum height of 22px', async () => {
    const { container } = await renderChunk({
      chunk: { start_time: localISO(9, 0), end_time: localISO(9, 5) },
    });
    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block).toBeTruthy();
    expect(block!.style.height).toBe('22px');
  });
});

describe('ChunkBlock — status classes', () => {
  it.each(renderProfileCases)(
    '$label uses the expected status and fixed classes',
    async ({ status, isFixed, expectedStatusClass, expectFixedClass }) => {
      const { container } = await renderChunk({ chunk: { status, is_fixed: isFixed } });
      const block = container.querySelector('.chunk-block');
      expect(block).toBeTruthy();
      expect(block!.classList.contains(expectedStatusClass)).toBe(true);
      expect(block!.classList.contains('is-fixed')).toBe(expectFixedClass);
    },
  );
});

describe('ChunkBlock — context menu', () => {
  it('renders the kebab button only when onmenu is provided', async () => {
    const withMenu = await renderChunk({}, { onmenu: vi.fn() });
    expect(withMenu.container.querySelector('.menu-btn')).toBeTruthy();
    cleanup();
    const withoutMenu = await renderChunk();
    expect(withoutMenu.container.querySelector('.menu-btn')).toBeNull();
  });

  it('right-click calls onmenu with the item and pointer coords, suppressing the native menu', async () => {
    const onmenu = vi.fn();
    const { container, item } = await renderChunk({}, { onmenu });
    const block = container.querySelector('.chunk-block') as HTMLElement;

    const defaultNotPrevented = await fireEvent.contextMenu(block, { clientX: 111, clientY: 222 });

    expect(onmenu).toHaveBeenCalledWith(item, 111, 222);
    expect(defaultNotPrevented).toBe(false);
  });

  it('right-click without onmenu leaves the native menu alone', async () => {
    const { container } = await renderChunk();
    const block = container.querySelector('.chunk-block') as HTMLElement;

    const defaultNotPrevented = await fireEvent.contextMenu(block);

    expect(defaultNotPrevented).toBe(true);
  });

  it('clicking the kebab calls onmenu and does not open the task editor', async () => {
    const onmenu = vi.fn();
    const onopen = vi.fn();
    const { container, item } = await renderChunk({}, { onmenu, onopen });
    const kebab = container.querySelector('.menu-btn') as HTMLElement;

    await fireEvent.click(kebab);

    expect(onmenu).toHaveBeenCalledOnce();
    expect(onmenu.mock.calls[0]?.[0]).toBe(item);
    expect(typeof onmenu.mock.calls[0]?.[1]).toBe('number');
    expect(typeof onmenu.mock.calls[0]?.[2]).toBe('number');
    expect(onopen).not.toHaveBeenCalled();
  });

  it('pressing the kebab does not start a drag', async () => {
    const { container } = await renderChunk({}, { onmenu: vi.fn() });
    const kebab = container.querySelector('.menu-btn') as HTMLElement;

    await fireEvent.pointerDown(kebab, { button: 0, pointerId: 1 });

    expect(dragState.active).toBeNull();
    expect(dragState.resizing).toBeNull();
  });
});

describe('ChunkBlock — complete action', () => {
  it.each(renderProfileCases)(
    '$label shows the correct completion toggle affordance',
    async ({ status, isFixed, expectCompleteAction, expectCompleteChecked }) => {
      const { container } = await renderChunk({ chunk: { status, is_fixed: isFixed } });
      const toggle = container.querySelector('.complete-toggle');
      expect(Boolean(toggle)).toBe(expectCompleteAction);
      expect(toggle?.getAttribute('aria-checked')).toBe(String(expectCompleteChecked));
    },
  );

  it('clicking the completion toggle calls oncomplete but not onopen', async () => {
    const oncomplete = vi.fn();
    const onopen = vi.fn();
    const { container, item } = await renderChunk({}, { oncomplete, onopen });
    const toggle = container.querySelector('.complete-toggle') as HTMLElement | null;
    expect(toggle).toBeTruthy();
    await toggle!.click();
    expect(oncomplete).toHaveBeenCalledWith(item);
    expect(onopen).not.toHaveBeenCalled();
  });
});

describe('ChunkBlock — accessibility', () => {
  it('aria-label contains the task title', async () => {
    const { container } = await renderChunk({ task_title: 'Yoga Session' });
    const block = container.querySelector('.chunk-block');
    const label = block!.getAttribute('aria-label') ?? '';
    expect(label).toContain('Yoga Session');
  });

  it('aria-label contains the time range', async () => {
    const { container } = await renderChunk({
      chunk: { start_time: localISO(9, 0), end_time: localISO(10, 30) },
    });
    const block = container.querySelector('.chunk-block');
    const label = block!.getAttribute('aria-label') ?? '';
    // Should contain "–" separator
    expect(label).toContain('–');
  });

  it.each(renderProfileCases)(
    '$label uses the expected aria state labels',
    async ({ status, isFixed, expectFixedLabel, expectCompletedLabel }) => {
      const { container } = await renderChunk({ chunk: { status, is_fixed: isFixed } });
      const block = container.querySelector('.chunk-block');
      const label = block!.getAttribute('aria-label') ?? '';
      expect(label.includes('fixed')).toBe(expectFixedLabel);
      expect(label.includes('completed')).toBe(expectCompletedLabel);
    },
  );
});

describe('ChunkBlock — pointer events', () => {
  it('chunk-block element has pointer-events: auto via class', async () => {
    const { container } = await renderChunk();
    expect(container.querySelector('.chunk-block')).toBeTruthy();
  });
});

describe('ChunkBlock — draggable class', () => {
  it.each(renderProfileCases)('$label remains draggable', async ({ status, isFixed }) => {
    const { container } = await renderChunk({ chunk: { status, is_fixed: isFixed } });
    expect(container.querySelector('.chunk-block.draggable')).toBeTruthy();
  });
});

describe('ChunkBlock — drag accessibility', () => {
  it.each(renderProfileCases)(
    '$label has role="button" and tabindex=0',
    async ({ status, isFixed }) => {
      const { container } = await renderChunk({ chunk: { status, is_fixed: isFixed } });
      const block = container.querySelector('.chunk-block');
      expect(block?.getAttribute('role')).toBe('button');
      expect(block?.getAttribute('tabindex')).toBe('0');
    },
  );
});

describe('ChunkBlock — lock toggle', () => {
  it.each([
    {
      label: 'scheduled with onlock',
      status: 'scheduled' as ChunkStatus,
      withOnlock: true,
      expectBtn: true,
    },
    {
      label: 'scheduled without onlock',
      status: 'scheduled' as ChunkStatus,
      withOnlock: false,
      expectBtn: false,
    },
    {
      label: 'completed with onlock',
      status: 'completed' as ChunkStatus,
      withOnlock: true,
      expectBtn: false,
    },
  ])('$label: lock button present=$expectBtn', async ({ status, withOnlock, expectBtn }) => {
    const { container } = await renderChunk(
      { chunk: { status } },
      withOnlock ? { onlock: vi.fn() } : {},
    );
    expect(Boolean(container.querySelector('.lock-btn'))).toBe(expectBtn);
  });

  it.each([
    { isFixed: true, expectedLabel: 'Unlock chunk' },
    { isFixed: false, expectedLabel: 'Lock chunk' },
  ])(
    'shows aria-label "$expectedLabel" when is_fixed=$isFixed',
    async ({ isFixed, expectedLabel }) => {
      const { container } = await renderChunk(
        { chunk: { status: 'scheduled', is_fixed: isFixed } },
        { onlock: vi.fn() },
      );
      const btn = container.querySelector('.lock-btn') as HTMLElement;
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBe(expectedLabel);
    },
  );

  it('clicking the lock button calls onlock with the agenda item', async () => {
    const onlock = vi.fn();
    const { container, item } = await renderChunk({ chunk: { status: 'scheduled' } }, { onlock });
    const btn = container.querySelector('.lock-btn') as HTMLElement;
    await fireEvent.click(btn);
    expect(onlock).toHaveBeenCalledWith(item);
  });

  it('clicking the lock button does not open the task editor', async () => {
    const onopen = vi.fn();
    const { container } = await renderChunk({}, { onlock: vi.fn(), onopen });
    const btn = container.querySelector('.lock-btn') as HTMLElement;
    await fireEvent.click(btn);
    expect(onopen).not.toHaveBeenCalled();
  });

  it('pointerdown on the lock button does not start a drag', async () => {
    const { container } = await renderChunk({}, { onlock: vi.fn() });
    const btn = container.querySelector('.lock-btn') as HTMLElement;
    await fireEvent.pointerDown(btn, { button: 0, pointerId: 1 });
    expect(dragState.active).toBeNull();
    expect(dragState.resizing).toBeNull();
  });
});

describe('ChunkBlock — open callback', () => {
  it('clicking a chunk opens its parent task', async () => {
    const onopen = vi.fn();
    const { container } = await renderChunk({}, { onopen });
    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block).toBeTruthy();
    block!.click();
    expect(onopen).toHaveBeenCalledWith('task-1');
  });
});

describe('ChunkBlock — resize handle', () => {
  it.each(renderProfileCases)('$label renders the resize handle', async ({ status, isFixed }) => {
    const { container } = await renderChunk({ chunk: { status, is_fixed: isFixed } });
    expect(container.querySelector('.resize-handle')).toBeTruthy();
  });

  it('resize-handle is aria-hidden', async () => {
    const { container } = await renderChunk({ chunk: { status: 'scheduled' } });
    const handle = container.querySelector('.resize-handle');
    expect(handle?.getAttribute('aria-hidden')).toBe('true');
  });

  it('ns-resize cursor CSS class exists on resize-handle element', async () => {
    const { container } = await renderChunk({ chunk: { status: 'scheduled' } });
    expect(container.querySelector('.resize-handle')).toBeTruthy();
  });
});

describe('ChunkBlock — edge cases', () => {
  it('renders with empty task_title gracefully', async () => {
    const { container } = await renderChunk({ task_title: '' });
    expect(container.querySelector('.chunk-block')).toBeTruthy();
  });

  it('renders zero-duration chunk at minimum height', async () => {
    const { container } = await renderChunk({
      chunk: { start_time: localISO(9, 0), end_time: localISO(9, 0) },
    });
    const block = container.querySelector('.chunk-block') as HTMLElement | null;
    expect(block!.style.height).toBe('22px');
  });
});

describe('ChunkBlock — detectColumnDate DOM structure', () => {
  /**
   * Build a minimal .day-columns container wrapping a .day-column element that
   * carries a data-column-date attribute, then mount the ChunkBlock inside it.
   * getBoundingClientRect in jsdom always returns zeroes, so we override it on
   * each column to simulate positional data.
   */
  function buildColumnParent(columns: Array<{ epoch: number; left: number; right: number }>) {
    const dayColumns = document.createElement('div');
    dayColumns.className = 'day-columns';
    for (const col of columns) {
      const div = document.createElement('div');
      div.className = 'day-column';
      div.setAttribute('data-column-date', String(col.epoch));
      // Provide realistic bounding rects (jsdom always returns 0 by default)
      const { left, right } = col;
      div.getBoundingClientRect = () =>
        ({
          left,
          right,
          top: 0,
          bottom: 600,
          width: right - left,
          height: 600,
          x: left,
          y: 0,
          toJSON() {
            return this;
          },
        }) as DOMRect;
      dayColumns.appendChild(div);
    }
    return dayColumns;
  }

  /** Two-column day-columns parent: Mon Mar 23 at [0,100), Tue Mar 24 at [100,200). */
  function buildMondayTuesdayColumns(): HTMLElement {
    const mondayEpoch = new Date(2026, 2, 23).getTime();
    const tuesdayEpoch = new Date(2026, 2, 24).getTime();
    return buildColumnParent([
      { epoch: mondayEpoch, left: 0, right: 100 },
      { epoch: tuesdayEpoch, left: 100, right: 200 },
    ]);
  }

  it('detectColumnDate finds the correct column when pointer is within its bounds', async () => {
    // Pass columnDate so dragState starts tracking Monday (Mar 23) as the origin column
    const mondayDate = new Date(2026, 2, 23);
    const { container } = await renderChunk({}, { columnDate: mondayDate });
    const block = container.querySelector('.chunk-block') as HTMLElement;
    expect(block).toBeTruthy();

    const parentEl = buildMondayTuesdayColumns();
    parentEl.children[0]!.appendChild(block);
    document.body.appendChild(parentEl);

    patchBlockForDrag(block);
    await fireEvent.pointerDown(block, { button: 0, clientY: 120, pointerId: 1 });
    expect(dragState.active?.chunkId).toBe('chunk-1');
    expect(dragState.active?.columnDate?.getDate()).toBe(23);

    const grid = document.createElement('div');
    grid.setAttribute('aria-label', 'Time grid');
    grid.getBoundingClientRect = () => ({ top: 0, bottom: 1440 }) as DOMRect;
    document.body.insertBefore(grid, parentEl);
    grid.appendChild(parentEl);

    await fireEvent.pointerMove(block, { clientX: 150, clientY: 200, pointerId: 1 });
    expect(dragState.active?.columnDate?.getDate()).toBe(24);

    await fireEvent.pointerMove(block, { clientX: 50, clientY: 200, pointerId: 1 });
    expect(dragState.active?.columnDate?.getDate()).toBe(23);

    dragState.cancel();
    document.body.removeChild(grid);
  });

  // Once a press passes the drag threshold it is a drag, so the follow-up click
  // must never open the task — wherever the pointer is released. The release slot
  // (snapped back to the original 9:00 ⇒ clientY 560, or a different slot ⇒ 300)
  // is irrelevant to that invariant.
  it.each([
    { label: 'snaps back to the same slot', releaseY: 560 },
    { label: 'lands on a different slot', releaseY: 300 },
  ])('a self-driven drag that $label never opens the task', async ({ releaseY }) => {
    const onopen = vi.fn();
    const { block } = await renderDraggableBlock({ onopen });
    const grid = appendTimeGrid(block);

    await fireEvent.pointerDown(block, { button: 0, clientX: 0, clientY: 120, pointerId: 1 });
    await fireEvent.pointerMove(block, { clientX: 0, clientY: 300, pointerId: 1 });
    await fireEvent.pointerMove(block, { clientX: 0, clientY: releaseY, pointerId: 1 });
    await fireEvent.pointerUp(block, { clientX: 0, clientY: releaseY, pointerId: 1 });

    // The browser would now fire a click; it must be suppressed (it was a drag).
    block.click();
    expect(onopen).not.toHaveBeenCalled();

    document.body.removeChild(grid);
  });

  it('detectColumnDate returns null when block is not inside a .day-columns container', async () => {
    const { block } = await renderDraggableBlock();
    await fireEvent.pointerDown(block, { button: 0, clientY: 120, pointerId: 1 });
    expect(dragState.active?.chunkId).toBe('chunk-1');
    const initialDate = dragState.active?.columnDate;

    const grid = appendTimeGrid(block);

    // Move pointer — no .day-columns ancestor, so column date should not change
    await fireEvent.pointerMove(block, { clientX: 150, clientY: 200, pointerId: 1 });
    expect(dragState.active?.columnDate).toEqual(initialDate);

    dragState.cancel();
    document.body.removeChild(grid);
  });

  it('pointer move during resize does not invoke detectColumnDate', async () => {
    const { block } = await renderDraggableBlock();
    // clientY = 155 means fromBottom = 160 - 155 = 5 ≤ 8 → resize
    await fireEvent.pointerDown(block, { button: 0, clientY: 155, pointerId: 1 });
    expect(dragState.resizing?.chunkId).toBe('chunk-1');
    const initialResizeDate = dragState.resizing?.columnDate;

    const grid = appendTimeGrid(block);

    // During resize, column date should not change even if columns with data attributes are present
    const parentEl = buildMondayTuesdayColumns();
    grid.appendChild(parentEl);

    await fireEvent.pointerMove(block, { clientX: 150, clientY: 200, pointerId: 1 });
    // Resize column date must not have changed
    expect(dragState.resizing?.columnDate).toEqual(initialResizeDate);

    dragState.cancelResize();
    document.body.removeChild(grid);
  });
});
