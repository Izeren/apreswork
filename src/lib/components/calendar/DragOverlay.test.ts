// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, cleanup } from '@testing-library/svelte';
import type { DragInfo, ResizeInfo } from './dragState.svelte';
import { HOUR_HEIGHT_PX } from './calendarLayout';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importDragOverlay() {
  const mod = await import('./DragOverlay.svelte');
  return mod.default;
}

async function getDragState() {
  const mod = await import('./dragState.svelte');
  return mod.dragState;
}

function baseDragInfo(overrides: Partial<DragInfo> = {}): DragInfo {
  return {
    chunkId: 'chunk-1',
    taskTitle: 'Test Task',
    originalStartTime: new Date(2026, 2, 28, 9, 0, 0).toISOString(),
    originalEndTime: new Date(2026, 2, 28, 10, 0, 0).toISOString(),
    durationMs: 60 * 60 * 1000,
    currentTopPx: 9 * HOUR_HEIGHT_PX,
    heightPx: HOUR_HEIGHT_PX,
    offsetY: 10,
    columnDate: new Date(2026, 2, 28, 0, 0, 0, 0),
    pressClientX: 0,
    pressClientY: 0,
    moved: false,
    ...overrides,
  };
}

function expectOverlayTopHeight(
  container: HTMLElement,
  selector: string,
  topPx: number,
  heightPx: number,
): void {
  const overlay = container.querySelector(selector) as HTMLElement | null;
  expect(overlay).toBeTruthy();
  expect(overlay!.style.top).toBe(`${topPx}px`);
  expect(overlay!.style.height).toBe(`${heightPx}px`);
}

describe('DragOverlay — not visible when no drag', () => {
  beforeEach(async () => {
    const ds = await getDragState();
    ds.cancel();
  });

  it('renders nothing when dragState.active is null', async () => {
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    expect(container.querySelector('.drag-overlay')).toBeNull();
  });
});

describe('DragOverlay — visible during drag', () => {
  beforeEach(async () => {
    const ds = await getDragState();
    ds.cancel();
    ds.start(baseDragInfo());
  });

  afterEach(async () => {
    const ds = await getDragState();
    ds.cancel();
  });

  it('renders .drag-overlay element when active', async () => {
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    expect(container.querySelector('.drag-overlay')).toBeTruthy();
  });

  it('renders the task title', async () => {
    const DragOverlay = await importDragOverlay();
    const { getByText } = render(DragOverlay);
    expect(getByText('Test Task')).toBeTruthy();
  });

  it('applies correct top and height from drag info', async () => {
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    expectOverlayTopHeight(container, '.drag-overlay', 9 * HOUR_HEIGHT_PX, HOUR_HEIGHT_PX);
  });

  it('is aria-hidden (not in accessibility tree)', async () => {
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    const overlay = container.querySelector('.drag-overlay');
    expect(overlay?.getAttribute('aria-hidden')).toBe('true');
  });

  it('renders the time range label', async () => {
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    const timeEl = container.querySelector('.drag-time');
    expect(timeEl).toBeTruthy();
    expect(timeEl!.textContent).toContain('–');
  });
});

function baseResizeInfo(overrides: Partial<ResizeInfo> = {}): ResizeInfo {
  return {
    chunkId: 'chunk-r1',
    taskTitle: 'Resize Task',
    originalStartTime: new Date(2026, 2, 28, 9, 0, 0).toISOString(),
    originalEndTime: new Date(2026, 2, 28, 10, 0, 0).toISOString(),
    originalHeightPx: HOUR_HEIGHT_PX,
    currentHeightPx: HOUR_HEIGHT_PX,
    topPx: 9 * HOUR_HEIGHT_PX,
    columnDate: new Date(2026, 2, 28, 0, 0, 0, 0),
    ...overrides,
  };
}

async function renderResizeOverlay(
  overrides: Partial<ResizeInfo> = {},
): Promise<{ container: HTMLElement }> {
  const ds = await getDragState();
  ds.startResize(baseResizeInfo(overrides));
  const DragOverlay = await importDragOverlay();
  return render(DragOverlay);
}

describe('DragOverlay — resize overlay', () => {
  beforeEach(async () => {
    const ds = await getDragState();
    ds.cancel();
    ds.cancelResize();
  });

  afterEach(async () => {
    const ds = await getDragState();
    ds.cancel();
    ds.cancelResize();
  });

  it('renders nothing when no resize active', async () => {
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    expect(container.querySelector('.drag-overlay--resize')).toBeNull();
  });

  it('renders resize overlay when resizing is active', async () => {
    const { container } = await renderResizeOverlay();
    expect(container.querySelector('.drag-overlay--resize')).toBeTruthy();
  });

  it('resize overlay applies correct top and height', async () => {
    const { container } = await renderResizeOverlay({
      topPx: 9 * HOUR_HEIGHT_PX,
      currentHeightPx: HOUR_HEIGHT_PX,
    });
    expectOverlayTopHeight(container, '.drag-overlay--resize', 9 * HOUR_HEIGHT_PX, HOUR_HEIGHT_PX);
  });

  it('resize overlay shows correct time range', async () => {
    const { container } = await renderResizeOverlay({
      topPx: 9 * HOUR_HEIGHT_PX,
      currentHeightPx: HOUR_HEIGHT_PX,
    });
    const timeEl = container.querySelector('.drag-overlay--resize .drag-time');
    expect(timeEl).toBeTruthy();
    expect(timeEl!.textContent).toContain('–');
  });

  it('resize overlay is aria-hidden', async () => {
    const { container } = await renderResizeOverlay();
    const overlay = container.querySelector('.drag-overlay--resize');
    expect(overlay?.getAttribute('aria-hidden')).toBe('true');
  });
});

describe('DragOverlay — edge cases', () => {
  beforeEach(async () => {
    const ds = await getDragState();
    ds.cancel();
  });

  afterEach(async () => {
    const ds = await getDragState();
    ds.cancel();
  });

  it.each([
    { label: 'top=0 for midnight', topPx: 0, expectedTop: '0px' as string },
    {
      label: 'end-of-day position',
      topPx: 23 * HOUR_HEIGHT_PX,
      expectedTop: `${23 * HOUR_HEIGHT_PX}px`,
    },
  ])('renders overlay at $label', async ({ topPx, expectedTop }) => {
    const ds = await getDragState();
    ds.start(baseDragInfo({ currentTopPx: topPx }));
    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    const overlay = container.querySelector('.drag-overlay') as HTMLElement | null;
    expect(overlay!.style.top).toBe(expectedTop);
  });

  it('handles null columnDate by falling back to originalStartTime', async () => {
    const ds = await getDragState();
    ds.start(baseDragInfo({ columnDate: null }));

    const DragOverlay = await importDragOverlay();
    const { container } = render(DragOverlay);
    expect(container.querySelector('.drag-overlay')).toBeTruthy();
    const timeEl = container.querySelector('.drag-time');
    expect(timeEl!.textContent).toContain('–');
  });
});
