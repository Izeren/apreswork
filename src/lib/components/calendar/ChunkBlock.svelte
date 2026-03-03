<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { AgendaItem, ChunkStatus } from '../../types';
  import { formatTime, formatDuration, isSameLocalDate } from '../../utils';
  import {
    HOUR_HEIGHT_PX,
    CHUNK_MIN_HEIGHT_PX as MIN_HEIGHT_PX,
    TIME_LABEL_MIN_HEIGHT_PX,
    computeGridBlockStyle,
    timeToGridHeightPx,
  } from './calendarLayout';
  import { dragState, topPxToIso, findColumnDateAt } from './dragState.svelte';
  import { calendarFocusState } from '../../stores/calendarFocus.svelte';

  type ChunkDensity = 'regular' | 'compact';

  interface ChunkRenderProfile {
    statusClass: string;
    showFixedAppearance: boolean;
    showCompleteAction: boolean;
    completeActionChecked: boolean;
    includeFixedInAria: boolean;
    includeCompletedInAria: boolean;
  }

  interface Props {
    item: AgendaItem;
    /** Lane index within an overlapping cluster, starting at 0. */
    overlapIndex?: number;
    /** Number of lanes in the overlapping cluster. Defaults to 1. */
    overlapCount?: number;
    /** Rendering density hint from the parent view. */
    density?: ChunkDensity;
    /** Column date used to resolve the target day when the user drops. Defaults to item's own date. */
    columnDate?: Date | null;
    /**
     * When true (default), this block drives its own move drag via pointer
     * capture on itself. When false, the parent view drives the move from a
     * stable element (so the drag survives a week flip); this block only
     * initiates the drag.
     */
    selfDriveMove?: boolean;
    /** Called when the user clicks the chunk to edit its parent task. */
    onopen?: ((taskId: string) => void) | null;
    /** Called when a drag-drop move is committed: (chunkId, newStart, newEnd) => void */
    onmove?: ((chunkId: string, newStart: string, newEnd: string) => void) | null;
    /** Called when a resize is committed: (chunkId, newEnd) => void */
    onresize?: ((chunkId: string, newEnd: string) => void) | null;
    /** Called when the user clicks the completion toggle. */
    oncomplete?: ((item: AgendaItem) => void) | null;
    /** Called to open the context menu at viewport coords (right-click or kebab). */
    onmenu?: ((item: AgendaItem, x: number, y: number) => void) | null;
    /** Called when the user clicks the lock/unlock toggle on a non-completed chunk. */
    onlock?: ((item: AgendaItem) => void) | null;
    /**
     * Current time used for past-treatment. When provided and later than the
     * chunk's end time, the block is rendered desaturated. Defaults to null
     * (no past treatment) so existing callers are unaffected.
     */
    now?: Date | null;
  }

  const {
    item,
    overlapIndex = 0,
    overlapCount = 1,
    density = 'regular',
    columnDate = null,
    selfDriveMove = true,
    onopen = null,
    onmove = null,
    onresize = null,
    oncomplete = null,
    onmenu = null,
    onlock = null,
    now = null,
  }: Props = $props();

  const TITLE_LABEL_MIN_HEIGHT_PX = MIN_HEIGHT_PX;
  const DURATION_LABEL_MIN_HEIGHT_PX = 72;

  function diffMinutes(a: Date, b: Date): number {
    return (b.getTime() - a.getTime()) / 60_000;
  }

  /** Bottom handle zone in px — pointer within this area from the bottom triggers resize, not move. */
  const RESIZE_HANDLE_PX = 8;

  /** Cross-view focus jump landed on this chunk — flash it until the carrier clears. */
  const isFlashing = $derived(calendarFocusState.chunkId === item.chunk.id);

  const normalizedOverlapCount = $derived.by(() => Math.max(1, Math.trunc(overlapCount) || 1));

  const normalizedOverlapIndex = $derived.by(() =>
    Math.min(Math.max(0, Math.trunc(overlapIndex) || 0), normalizedOverlapCount - 1),
  );

  const chunkStart = $derived(new Date(item.chunk.start_time));
  const chunkEnd = $derived(new Date(item.chunk.end_time));

  const topPx = $derived(timeToGridHeightPx(chunkStart));

  const heightPx = $derived.by(() => {
    const dur = diffMinutes(chunkStart, chunkEnd);
    return Math.max(MIN_HEIGHT_PX, (dur / 60) * HOUR_HEIGHT_PX);
  });

  const durationMin = $derived(Math.round(diffMinutes(chunkStart, chunkEnd)));

  const showTitle = $derived(
    item.task_title.trim().length > 0 && heightPx >= TITLE_LABEL_MIN_HEIGHT_PX,
  );

  const isOverlap = $derived(normalizedOverlapCount > 1);
  const isDense = $derived(density === 'compact' || isOverlap);
  const isShort = $derived(heightPx < TIME_LABEL_MIN_HEIGHT_PX);

  const showTime = $derived.by(() => {
    if (isOverlap) return heightPx >= DURATION_LABEL_MIN_HEIGHT_PX;
    if (density === 'compact') return heightPx >= 60;
    return heightPx >= TIME_LABEL_MIN_HEIGHT_PX;
  });

  const showDuration = $derived.by(() => !isDense && heightPx >= DURATION_LABEL_MIN_HEIGHT_PX);

  const compactTitle = $derived(showTitle && !showTime && !showDuration);

  const titleLineClamp = $derived.by(() => {
    if (!isDense) return 1;
    if (showTime) return 2;
    return heightPx >= 54 ? 3 : 2;
  });

  function createRenderProfile(status: ChunkStatus, isFixed: boolean): ChunkRenderProfile {
    const isCompleted = status === 'completed';
    const showFixedAppearance = isFixed && !isCompleted;

    return {
      statusClass: `chunk-block--${status}`,
      showFixedAppearance,
      showCompleteAction: true,
      completeActionChecked: isCompleted,
      includeFixedInAria: showFixedAppearance,
      includeCompletedInAria: isCompleted,
    };
  }

  const renderProfile = $derived.by(() =>
    createRenderProfile(item.chunk.status, item.chunk.is_fixed),
  );

  /** True when the chunk ends after its task's deadline (and isn't completed). */
  const isOverdue = $derived(
    item.task_deadline !== null &&
      item.chunk.status !== 'completed' &&
      chunkEnd.getTime() > new Date(item.task_deadline).getTime(),
  );

  const ariaLabel = $derived(
    `${item.task_title}: ${formatTime(item.chunk.start_time)} – ${formatTime(item.chunk.end_time)}` +
      (renderProfile.includeFixedInAria ? ', fixed' : '') +
      (renderProfile.includeCompletedInAria ? ', completed' : '') +
      (isOverdue ? ', past deadline' : ''),
  );

  /** True when the chunk ended before the current time (past-treatment). */
  const isPast = $derived(now !== null && chunkEnd.getTime() < now.getTime());

  /** True while this chunk is being dragged. */
  const isDragging = $derived(dragState.active?.chunkId === item.chunk.id);

  /** True while this chunk is being resized. */
  const isResizing = $derived(dragState.resizing?.chunkId === item.chunk.id);

  const blockStyle = $derived.by(() =>
    computeGridBlockStyle({
      topPx,
      heightPx,
      isOverlap,
      overlapIndex: normalizedOverlapIndex,
      overlapCount: normalizedOverlapCount,
      zIndex: isDragging || isResizing ? 6 : 2 + normalizedOverlapIndex,
      extra: [`--chunk-title-lines: ${titleLineClamp}`],
    }),
  );

  let blockEl: HTMLDivElement | undefined = $state();
  let capturedPointerId: number | null = null;
  let suppressNextClick = false;

  function getTimeGridRect(): DOMRect | null {
    if (!blockEl) return null;
    let el: Element | null = blockEl;
    while (el) {
      if (el.getAttribute('aria-label') === 'Time grid') return el.getBoundingClientRect();
      el = el.parentElement;
    }
    return null;
  }

  function getDayColumnsEl(): HTMLElement | null {
    let el: Element | null = blockEl ?? null;
    while (el) {
      if (el.classList.contains('day-columns')) return el as HTMLElement;
      el = el.parentElement;
    }
    return null;
  }

  function releaseCapturedPointer(): void {
    if (capturedPointerId !== null) {
      blockEl?.releasePointerCapture(capturedPointerId);
      capturedPointerId = null;
    }
  }

  function handlePointerDown(e: PointerEvent): void {
    if (!blockEl) return;

    if (e.button !== 0) return;

    const rect = blockEl.getBoundingClientRect();
    const fromBottom = rect.bottom - e.clientY;

    // If pointer is in the bottom RESIZE_HANDLE_PX zone, start resize
    if (fromBottom <= RESIZE_HANDLE_PX) {
      e.preventDefault();
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      capturedPointerId = e.pointerId;
      suppressNextClick = true;
      dragState.startResize({
        chunkId: item.chunk.id,
        taskTitle: item.task_title,
        originalStartTime: item.chunk.start_time,
        originalEndTime: item.chunk.end_time,
        originalHeightPx: heightPx,
        currentHeightPx: heightPx,
        topPx,
        columnDate: columnDate ?? chunkStart,
      });
      return;
    }

    e.preventDefault();

    const offsetY = e.clientY - rect.top;
    const durationMs = chunkEnd.getTime() - chunkStart.getTime();

    if (selfDriveMove) {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      capturedPointerId = e.pointerId;
    } else {
      // The parent view drives the move; capture on the stable .day-columns
      // container so the drag survives its source column unmounting on a flip.
      getDayColumnsEl()?.setPointerCapture(e.pointerId);
    }

    const col = columnDate ?? chunkStart;
    dragState.start({
      chunkId: item.chunk.id,
      taskTitle: item.task_title,
      originalStartTime: item.chunk.start_time,
      originalEndTime: item.chunk.end_time,
      durationMs,
      currentTopPx: topPx,
      heightPx,
      offsetY,
      columnDate: col,
      pressClientX: e.clientX,
      pressClientY: e.clientY,
      moved: false,
    });
  }

  /** Walk up from blockEl to find the .day-columns container, then locate clientX within it. */
  function detectColumnDate(clientX: number): Date | null {
    const el = getDayColumnsEl();
    if (!el) return null;
    return findColumnDateAt(el, clientX);
  }

  function handlePointerMove(e: PointerEvent): void {
    const gridRect = getTimeGridRect();
    if (!gridRect) return;

    if (isResizing) {
      dragState.updateResizePosition(e.clientY, gridRect);
      return;
    }

    // When the parent view drives the move, it owns position + column updates.
    if (!isDragging || !selfDriveMove) return;
    dragState.updateMoved(e.clientX, e.clientY);
    dragState.updatePosition(e.clientY, gridRect);

    const colDate = detectColumnDate(e.clientX);
    if (colDate) {
      dragState.updateColumn(colDate);
    }
  }

  function handlePointerUp(e: PointerEvent): void {
    if (isResizing) {
      e.preventDefault();
      releaseCapturedPointer();
      const final = dragState.endResize();
      if (final && onresize) {
        const didResize = final.currentHeightPx !== final.originalHeightPx;
        if (!didResize) return;
        const targetDate = final.columnDate ?? new Date(final.originalStartTime);
        const newEnd = topPxToIso(final.topPx + final.currentHeightPx, targetDate);
        onresize(final.chunkId, newEnd);
      }
      return;
    }

    // When the parent view drives the move, it owns the drop commit too.
    if (!isDragging || !selfDriveMove) return;
    e.preventDefault();

    releaseCapturedPointer();

    const final = dragState.end();
    if (!final) return;

    // Travel within the drag threshold is a click, not a drag — let handleClick
    // open the task. A real drag never opens it (handleClick reads moved off
    // dragState.lastEnded, set by dragState.end() above), even one that snapped back.
    if (!final.moved) return;

    const originalDate = new Date(final.originalStartTime);
    const targetDate = final.columnDate ?? originalDate;
    const slotChanged = final.currentTopPx !== topPx || !isSameLocalDate(targetDate, originalDate);
    if (!slotChanged || !onmove) return;

    const newStart = topPxToIso(final.currentTopPx, targetDate);
    const newEndMs = new Date(newStart).getTime() + final.durationMs;
    const newEnd = new Date(newEndMs).toISOString();

    onmove(final.chunkId, newStart, newEnd);
  }

  function handleKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && (isDragging || isResizing)) {
      releaseCapturedPointer();
      dragState.cancel();
      dragState.cancelResize();
    }

    if ((e.key === 'Enter' || e.key === ' ') && !isDragging && !isResizing) {
      e.preventDefault();
      onopen?.(item.chunk.task_id);
    }
  }

  function handleClick(): void {
    if (suppressNextClick) {
      suppressNextClick = false;
      return;
    }

    // Primarily for parent-driven moves (WeekView): pointer capture there sits on
    // its container, so this component's own handlePointerUp never runs and never
    // sets suppressNextClick above, yet the browser's click still lands here via
    // normal hit-testing. Also, a harmless belt-and-suspenders check when this
    // component drove the move itself.
    if (dragState.lastEnded?.chunkId === item.chunk.id) {
      const { moved } = dragState.lastEnded;
      dragState.lastEnded = null;
      if (moved) return;
    }

    onopen?.(item.chunk.task_id);
  }

  function handleCompleteClick(e: MouseEvent): void {
    e.stopPropagation();
    oncomplete?.(item);
  }

  function handleContextMenu(e: MouseEvent): void {
    if (!onmenu) return;
    e.preventDefault();
    onmenu(item, e.clientX, e.clientY);
  }

  function handleMenuClick(e: MouseEvent): void {
    e.stopPropagation();
    // Anchor below the kebab so keyboard activation gets a sensible position.
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    onmenu?.(item, rect.left, rect.bottom + 2);
  }

  function handleLockClick(e: MouseEvent): void {
    e.stopPropagation();
    onlock?.(item);
  }
</script>

<div
  bind:this={blockEl}
  class="chunk-block draggable {renderProfile.statusClass}"
  class:is-past={isPast}
  class:is-dragging={isDragging}
  class:is-resizing={isResizing}
  class:is-overlap={isOverlap}
  class:is-dense={isDense}
  class:is-short={isShort}
  class:is-fixed={renderProfile.showFixedAppearance}
  class:is-overdue={isOverdue}
  class:has-complete-action={renderProfile.showCompleteAction}
  class:is-compact={compactTitle}
  class:is-flashing={isFlashing}
  data-density={density}
  data-overlap-count={normalizedOverlapCount}
  data-overlap-index={normalizedOverlapIndex}
  style={blockStyle}
  aria-label={ariaLabel}
  title={ariaLabel}
  role="button"
  tabindex={0}
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onclick={handleClick}
  onkeydown={handleKeyDown}
  oncontextmenu={handleContextMenu}
>
  {#if renderProfile.showCompleteAction}
    <button
      class="complete-toggle"
      class:complete-toggle--checked={renderProfile.completeActionChecked}
      aria-label={renderProfile.completeActionChecked
        ? 'Reopen completed chunk'
        : 'Complete chunk or task'}
      role="checkbox"
      aria-checked={renderProfile.completeActionChecked}
      onpointerdown={(e: PointerEvent) => e.stopPropagation()}
      onclick={handleCompleteClick}
    >
      <span aria-hidden="true">✓</span>
    </button>
  {/if}
  {#if onlock && !renderProfile.completeActionChecked}
    <button
      class="lock-btn"
      aria-label={item.chunk.is_fixed ? 'Unlock chunk' : 'Lock chunk'}
      onpointerdown={(e: PointerEvent) => e.stopPropagation()}
      onclick={handleLockClick}
    >
      <span aria-hidden="true">{item.chunk.is_fixed ? '🔒' : '🔓'}</span>
    </button>
  {/if}
  {#if onmenu}
    <button
      class="menu-btn"
      aria-label="Open chunk menu"
      aria-haspopup="menu"
      onpointerdown={(e: PointerEvent) => e.stopPropagation()}
      onclick={handleMenuClick}
    >
      <span aria-hidden="true">⋯</span>
    </button>
  {/if}
  {#if showTitle}
    <span class="chunk-title">{item.task_title}</span>
  {/if}
  {#if showTime}
    <span class="chunk-time">
      {formatTime(item.chunk.start_time)} – {formatTime(item.chunk.end_time)}
    </span>
  {/if}
  {#if showDuration}
    <span class="chunk-duration">{formatDuration(durationMin)}</span>
  {/if}
  <div class="resize-handle" aria-hidden="true"></div>
</div>

<style>
  .chunk-block {
    position: absolute;
    padding: var(--spacing-1) var(--spacing-2);
    border-radius: var(--radius-sm);
    border-left: 3px solid var(--color-chunk-scheduled-border);
    background: var(--color-chunk-scheduled);
    font-size: var(--font-size-xs);
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow: hidden;
    pointer-events: auto;
    box-sizing: border-box;
    user-select: none;
  }

  .chunk-block.draggable {
    cursor: grab;
  }

  .chunk-block.is-compact,
  .chunk-block.is-short {
    gap: 0;
    justify-content: center;
    padding-top: 1px;
    padding-bottom: 1px;
  }

  .chunk-block.is-dense {
    gap: 0;
    padding-top: 2px;
    padding-bottom: 2px;
  }

  .chunk-block.has-complete-action {
    padding-left: calc(var(--spacing-2) + 0.95rem);
  }

  .chunk-block.is-past {
    filter: saturate(var(--calendar-past-chunk-saturate, 0.35));
    opacity: 0.85;
  }

  .chunk-block.is-dragging {
    opacity: 0.3;
    cursor: grabbing;
  }

  .chunk-block.is-resizing {
    opacity: 0.6;
    cursor: ns-resize;
  }

  .chunk-block.is-flashing {
    animation: focus-flash 0.8s ease-in-out infinite alternate;
    z-index: 5;
    /* keep the attention flash at full saturation even on past chunks */
    filter: none;
  }

  @keyframes focus-flash {
    from {
      outline: 2px solid var(--color-primary);
      outline-offset: 1px;
    }
    to {
      outline: 2px solid transparent;
      outline-offset: 1px;
    }
  }

  .resize-handle {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 8px;
    cursor: ns-resize;
  }

  .chunk-block--scheduled {
    background: var(--color-chunk-scheduled);
    border-left-color: var(--color-chunk-scheduled-border);
    box-shadow: inset 0 -1px 0
      color-mix(in srgb, var(--color-chunk-scheduled-border) 45%, transparent);
  }

  .chunk-block--completed {
    background: var(--color-chunk-completed);
    border-left-color: var(--color-chunk-completed-border);
    box-shadow: inset 0 -1px 0
      color-mix(in srgb, var(--color-chunk-completed-border) 45%, transparent);
  }

  .chunk-block.is-dense {
    padding-left: 4px;
    padding-right: 4px;
  }

  .chunk-block.is-dense.has-complete-action {
    padding-left: calc(4px + 0.8rem);
  }

  .chunk-block--scheduled.is-fixed {
    background: var(--color-chunk-fixed);
    border-left-color: var(--color-chunk-fixed-border);
    box-shadow: inset 0 -1px 0 color-mix(in srgb, var(--color-chunk-fixed-border) 55%, transparent);
  }

  .chunk-block--completed.is-fixed {
    box-shadow:
      inset 0 0 0 1px var(--color-chunk-fixed-border),
      inset 0 -1px 0 color-mix(in srgb, var(--color-chunk-fixed-border) 55%, transparent);
  }

  .chunk-block--scheduled.is-overdue,
  .chunk-block--scheduled.is-fixed.is-overdue {
    border-left-color: var(--color-error);
    background: color-mix(in srgb, var(--color-error) 14%, var(--color-surface));
    box-shadow: inset 0 -1px 0 color-mix(in srgb, var(--color-error) 45%, transparent);
  }

  .complete-toggle {
    position: absolute;
    top: 2px;
    left: 4px;
    width: 0.75rem;
    height: 0.75rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--color-chunk-scheduled-border);
    border-radius: 3px;
    background: color-mix(in srgb, var(--color-surface) 92%, transparent);
    color: var(--color-chunk-scheduled-border);
    font-size: 0.55rem;
    line-height: 1;
    padding: 0;
    cursor: pointer;
    pointer-events: auto;
    z-index: 5;
  }

  .complete-toggle:hover {
    background: var(--color-surface);
  }

  .complete-toggle.complete-toggle--checked {
    border-color: var(--color-chunk-completed-border);
    background: var(--color-chunk-completed);
    color: var(--color-status-completed);
  }

  .complete-toggle:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .chunk-block.is-dense .complete-toggle {
    top: 1px;
    left: 2px;
    width: 0.65rem;
    height: 0.65rem;
    font-size: 0.5rem;
  }

  .lock-btn,
  .menu-btn {
    position: absolute;
    top: 1px;
    width: 0.9rem;
    height: 0.9rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
    background: color-mix(in srgb, var(--color-surface) 85%, transparent);
    border: none;
    border-radius: var(--radius-sm);
    padding: 0;
    color: var(--color-text-secondary);
    cursor: pointer;
    pointer-events: auto;
    z-index: 5;
  }

  .lock-btn {
    right: calc(3px + 0.9rem + 2px);
    font-size: 0.65rem;
  }

  .lock-btn:hover,
  .menu-btn:hover {
    color: var(--color-text);
    background: var(--color-surface);
  }

  .lock-btn:focus-visible,
  .menu-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .chunk-block.is-dense .lock-btn {
    right: calc(2px + 0.9rem + 2px);
    font-size: 0.55rem;
  }

  .menu-btn {
    right: 3px;
    font-size: 0.7rem;
    opacity: 0;
  }

  .chunk-block:hover .menu-btn,
  .menu-btn:focus-visible {
    opacity: 1;
  }

  .chunk-block.is-dense .menu-btn {
    top: 1px;
    right: 2px;
    font-size: 0.6rem;
  }

  .chunk-title {
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    line-height: 1.15;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chunk-block.is-dense .chunk-title {
    white-space: normal;
    display: -webkit-box;
    line-clamp: var(--chunk-title-lines, 2);
    -webkit-line-clamp: var(--chunk-title-lines, 2);
    -webkit-box-orient: vertical;
    overflow-wrap: anywhere;
    word-break: break-word;
  }

  .chunk-time {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-secondary);
    line-height: 1.1;
    white-space: nowrap;
  }

  .chunk-block.is-dense .chunk-time {
    font-size: 0.625rem;
  }

  .chunk-duration {
    color: var(--color-text-secondary);
    line-height: 1.1;
    white-space: nowrap;
  }
</style>
