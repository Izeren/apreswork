<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { AgendaItem, ExternalEvent } from '../../types';
  import TimeGrid from './TimeGrid.svelte';
  import ChunkBlock from './ChunkBlock.svelte';
  import ExternalEventBlock from './ExternalEventBlock.svelte';
  import DayExternalsLayer from './DayExternalsLayer.svelte';
  import { layoutDayColumn } from './overlapLayout';
  import ScheduleWindowOverlay from './ScheduleWindowOverlay.svelte';
  import DragOverlay from './DragOverlay.svelte';
  import PastWash from './PastWash.svelte';
  import ChunkCreateLayer from './ChunkCreateLayer.svelte';
  import { dragState, topPxToIso, findColumnDateAt } from './dragState.svelte';
  import { addMillisecondsToIso, pastOverlayHeightPx, timeToGridHeightPx } from './calendarLayout';
  import {
    type CalendarViewCommonProps,
    earliestWindowScrollTop,
    filterByDay,
    makeEventOpenHandler,
    weekdayName,
  } from './calendarViewShared';
  import {
    EdgeFlipController,
    edgeDirection,
    WEEK_EDGE_ZONE_PX,
    WEEK_FLIP_DWELL_MS,
    type FlipDirection,
  } from './weekEdgeFlip';
  import { isSameLocalDate } from '../../utils';

  interface Props extends CalendarViewCommonProps {
    days: Date[];
    /** Called when an edge-dwell during a drag flips the week by ±1. */
    onweekflip?: ((direction: FlipDirection) => void) | null;
  }

  const {
    days,
    now,
    items,
    windows = [],
    externalEvents = [],
    oneventopen = null,
    editableCalendarId = null,
    onchunkopen = null,
    onchunkcomplete = null,
    onchunkmove = null,
    onchunkresize = null,
    onchunkmenu = null,
    onchunklock = null,
    oncreatechunk = null,
    onweekflip = null,
    disconnected = false,
  }: Props = $props();

  let bodyEl: HTMLDivElement | undefined = $state();

  /** The `.day-columns` container — a stable drag driver across week flips. */
  let dayColumnsEl: HTMLDivElement | undefined = $state();
  let activeEdge: FlipDirection | null = $state(null);
  /** Dwell-timer controller for edge-triggered week flips (lazily created). */
  let flipController: EdgeFlipController | null = null;

  const indicatorTop = $derived(timeToGridHeightPx(now));

  function columnLabel(day: Date): string {
    const dayName = day.toLocaleDateString(undefined, { weekday: 'short' });
    const dayNum = day.getDate();
    return `${dayName} ${dayNum}`;
  }

  function itemsForDay(day: Date): AgendaItem[] {
    return filterByDay(items, (item) => item.chunk.start_time, day);
  }

  function externalsForDay(day: Date): ExternalEvent[] {
    return filterByDay(externalEvents, (e) => e.start_time, day);
  }

  function externalsByAllDay(day: Date, allDay: boolean): ExternalEvent[] {
    return externalsForDay(day).filter((e) => e.all_day === allDay);
  }

  /** Whether any visible day has an all-day event (drives the lane's presence). */
  const weekHasAllDay = $derived(days.some((day) => externalsByAllDay(day, true).length > 0));

  const eventOpenHandler = $derived(makeEventOpenHandler(oneventopen, editableCalendarId));

  const initialScrollTop = $derived.by(() => {
    const earliestDay = days[0];
    if (!earliestDay) return 0;

    const earliestDayName = weekdayName(earliestDay);
    const preferredWindows = windows.filter((window) => window.day_of_week === earliestDayName);
    const fallbackWindows =
      preferredWindows.length > 0
        ? preferredWindows
        : windows.filter((window) => days.some((day) => window.day_of_week === weekdayName(day)));

    return earliestWindowScrollTop(fallbackWindows);
  });

  function handleColumnPointerEnter(day: Date): void {
    if (dragState.active) {
      dragState.updateColumn(day);
    }
    if (dragState.creating) {
      dragState.updateCreateColumn(day);
    }
  }

  $effect(() => {
    if (!bodyEl) return;
    bodyEl.scrollTop = initialScrollTop;
  });

  // Move drag — driven from the stable `.day-columns` container so it survives a
  // week flip (the dragged chunk's own column unmounts when the week changes).

  /** Bounding rect of the time grid (Y origin for snapping), or null. */
  function gridRectFor(): DOMRect | null {
    const grid = dayColumnsEl?.closest('[aria-label="Time grid"]');
    return grid instanceof HTMLElement ? grid.getBoundingClientRect() : null;
  }

  function detectMoveColumn(clientX: number): Date | null {
    if (!dayColumnsEl) return null;
    return findColumnDateAt(dayColumnsEl, clientX);
  }

  function ensureFlipController(): EdgeFlipController {
    flipController ??= new EdgeFlipController(WEEK_FLIP_DWELL_MS, (direction) => {
      onweekflip?.(direction);
    });
    return flipController;
  }

  function endEdgeTracking(): void {
    activeEdge = null;
    flipController?.stop();
  }

  function handleMovePointerMove(event: PointerEvent): void {
    if (!dragState.active) return;

    dragState.updateMoved(event.clientX, event.clientY);

    const gridRect = gridRectFor();
    if (gridRect) dragState.updatePosition(event.clientY, gridRect);

    const column = detectMoveColumn(event.clientX);
    if (column) dragState.updateColumn(column);

    const bounds = dayColumnsEl?.getBoundingClientRect();
    const direction = bounds ? edgeDirection(event.clientX, bounds, WEEK_EDGE_ZONE_PX) : null;
    activeEdge = direction;
    ensureFlipController().update(direction);
  }

  function handleMovePointerUp(event: PointerEvent): void {
    if (!dragState.active) return;
    event.preventDefault();
    endEdgeTracking();

    const final = dragState.end();
    if (!final) return;

    // Travel within the drag threshold is a click, not a drag — nothing to
    // commit here. Pointer capture on this container does not retarget the
    // browser's follow-up click, which still lands on the chunk's own element
    // and opens it via its onopen={onchunkopen} prop (see dragState.lastEnded,
    // set by dragState.end() above, for how that click tells drag from click).
    if (!final.moved) return;

    // A drag: commit the reposition only if it landed on a different slot.
    if (!onchunkmove) return;
    const originalDate = new Date(final.originalStartTime);
    const targetDate = final.columnDate ?? originalDate;
    const movedDay = !isSameLocalDate(targetDate, originalDate);
    const originalTopPx = timeToGridHeightPx(originalDate);
    const movedTime = final.currentTopPx !== originalTopPx;
    if (!movedTime && !movedDay) return;

    const newStart = topPxToIso(final.currentTopPx, targetDate);
    const newEnd = addMillisecondsToIso(newStart, final.durationMs);
    onchunkmove(final.chunkId, newStart, newEnd);
  }

  function handleMovePointerCancel(): void {
    if (!dragState.active) return;
    endEdgeTracking();
    dragState.cancel();
  }

  /** True while a move drag is in flight (stable across position updates). */
  const moveActive = $derived(dragState.active !== null);

  function isDragOverlayForDay(day: Date): boolean {
    return (
      (dragState.active?.columnDate != null && isSameLocalDate(dragState.active.columnDate, day)) ||
      (dragState.resizing?.columnDate != null &&
        isSameLocalDate(dragState.resizing.columnDate, day))
    );
  }

  $effect(() => {
    if (!moveActive) return;
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === 'Escape') {
        endEdgeTracking();
        dragState.cancel();
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });
</script>

<div class="week-view">
  <div class="week-headers" role="row" aria-label="Week day headers">
    <!-- Spacer aligning headers with the TimeGrid content area (52px time labels) -->
    <div class="header-spacer" aria-hidden="true"></div>
    {#each days as day (day.toDateString())}
      <div
        class="day-header"
        class:day-header--today={isSameLocalDate(day, now)}
        role="columnheader"
        aria-label={columnLabel(day)}
      >
        {columnLabel(day)}
      </div>
    {/each}
  </div>

  {#if weekHasAllDay}
    <div class="week-all-day" aria-label="All-day events">
      <div class="all-day-gutter" aria-hidden="true"></div>
      {#each days as day (day.toDateString())}
        <div class="all-day-cell">
          {#each externalsByAllDay(day, true) as ext (ext.event_id)}
            <ExternalEventBlock event={ext} onopen={eventOpenHandler(ext)} {disconnected} />
          {/each}
        </div>
      {/each}
    </div>
  {/if}

  <div bind:this={bodyEl} class="week-body">
    <TimeGrid date={days[0] ?? now} showCurrentTimeIndicator={false}>
      <!-- 7 day columns overlaid on the grid content area. Pointer handlers live
           here (a stable element) so a move drag survives a week flip. -->
      <div
        class="day-columns"
        role="presentation"
        bind:this={dayColumnsEl}
        onpointermove={handleMovePointerMove}
        onpointerup={handleMovePointerUp}
        onpointercancel={handleMovePointerCancel}
      >
        {#each days as day (day.toDateString())}
          {@const dayLayout = layoutDayColumn(itemsForDay(day), externalsByAllDay(day, false))}
          <div
            class="day-column"
            aria-label={columnLabel(day)}
            role="presentation"
            data-column-date={day.getTime()}
            onpointerenter={() => handleColumnPointerEnter(day)}
          >
            <ScheduleWindowOverlay {windows} date={day} />
            <PastWash heightPx={pastOverlayHeightPx(day, now)} />
            <DayExternalsLayer externals={dayLayout.externals} {eventOpenHandler} {disconnected} />
            <ChunkCreateLayer
              columnDate={day}
              ariaLabel="Create chunk for {columnLabel(day)}"
              {oncreatechunk}
            />
            {#if isSameLocalDate(day, now)}
              <div
                class="column-time-indicator"
                role="presentation"
                aria-hidden="true"
                style="top: {indicatorTop}px"
              ></div>
            {/if}
            <!-- Drag overlay lives inside each column so it inherits column positioning -->
            {#if isDragOverlayForDay(day)}
              <DragOverlay />
            {/if}
            {#if dayLayout.chunks.length === 0}
              <p class="empty-state" aria-label="No chunks for {columnLabel(day)}">—</p>
            {:else}
              {#each dayLayout.chunks as layout (layout.item.chunk.id)}
                <ChunkBlock
                  item={layout.item}
                  overlapIndex={layout.overlapIndex}
                  overlapCount={layout.overlapCount}
                  density="compact"
                  columnDate={day}
                  selfDriveMove={false}
                  {now}
                  onopen={onchunkopen}
                  oncomplete={onchunkcomplete}
                  onmove={onchunkmove}
                  onresize={onchunkresize}
                  onmenu={onchunkmenu}
                  onlock={onchunklock}
                />
              {/each}
            {/if}
          </div>
        {/each}
      </div>
    </TimeGrid>
  </div>

  {#if moveActive}
    <div
      class="edge-zone edge-zone--left"
      class:edge-zone--armed={activeEdge === -1}
      aria-hidden="true"
      style="width: {WEEK_EDGE_ZONE_PX}px"
    ></div>
    <div
      class="edge-zone edge-zone--right"
      class:edge-zone--armed={activeEdge === 1}
      aria-hidden="true"
      style="width: {WEEK_EDGE_ZONE_PX}px"
    ></div>
  {/if}
</div>

<style>
  .week-view {
    --gutter-width: 52px;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    min-height: 0;
    position: relative;
  }

  .edge-zone {
    position: absolute;
    top: 0;
    bottom: 0;
    z-index: 4;
    pointer-events: none;
    background: color-mix(in srgb, var(--color-primary) 6%, transparent);
    transition:
      background var(--transition-fast),
      box-shadow var(--transition-fast);
  }

  /* Aligns with the time-label gutter so the zone overlays the day columns. */
  .edge-zone--left {
    left: var(--gutter-width);
  }

  .edge-zone--right {
    right: 0;
  }

  .edge-zone--armed {
    background: color-mix(in srgb, var(--color-primary) 22%, transparent);
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--color-primary) 45%, transparent);
  }

  .week-headers {
    display: flex;
    flex-shrink: 0;
    border-bottom: 1px solid var(--color-border-light);
    background: var(--color-bg-secondary);
  }

  .header-spacer {
    width: var(--gutter-width);
    flex-shrink: 0;
  }

  .day-header {
    flex: 1;
    padding: var(--spacing-2) var(--spacing-1);
    text-align: center;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-secondary);
    border-left: 1px solid var(--color-border-light);
    user-select: none;
  }

  .day-header--today {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
  }

  .week-all-day {
    display: flex;
    flex-shrink: 0;
    max-height: 4.5rem;
    overflow-y: auto;
    border-bottom: 1px solid var(--color-border-light);
    background: var(--color-bg-secondary);
  }

  /* Aligns the lane cells with the time-label gutter (matches .header-spacer). */
  .all-day-gutter {
    width: var(--gutter-width);
    flex-shrink: 0;
  }

  .all-day-cell {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--spacing-1);
    border-left: 1px solid var(--color-border-light);
  }

  /* The first cell sits flush against the gutter; :first-of-type would match the
     gutter's sibling type, so target the cell adjacent to the gutter instead. */
  .all-day-gutter + .all-day-cell {
    border-left: none;
  }

  .week-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .day-columns {
    display: flex;
    height: 100%;
    pointer-events: auto;
  }

  .day-column {
    flex: 1;
    min-width: 0;
    position: relative;
    border-left: 1px solid var(--color-border-light);
    overflow: hidden;
  }

  .column-time-indicator {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 2px;
    background: var(--color-time-indicator);
    z-index: 3;
    pointer-events: none;
  }

  .day-column:first-child {
    border-left: none;
  }

  .empty-state {
    padding: var(--spacing-2);
    text-align: center;
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    margin: 0;
  }
</style>
