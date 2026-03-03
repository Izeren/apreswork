<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import TimeGrid from './TimeGrid.svelte';
  import { pastOverlayHeightPx } from './calendarLayout';
  import ChunkBlock from './ChunkBlock.svelte';
  import ExternalEventBlock from './ExternalEventBlock.svelte';
  import DayExternalsLayer from './DayExternalsLayer.svelte';
  import ScheduleWindowOverlay from './ScheduleWindowOverlay.svelte';
  import { layoutDayColumn } from './overlapLayout';
  import DragOverlay from './DragOverlay.svelte';
  import PastWash from './PastWash.svelte';
  import ChunkCreateLayer from './ChunkCreateLayer.svelte';
  import {
    type CalendarViewCommonProps,
    earliestWindowScrollTop,
    filterByDay,
    makeEventOpenHandler,
    weekdayName,
  } from './calendarViewShared';

  interface Props extends CalendarViewCommonProps {
    date: Date;
  }

  // Non-destructuring $props() avoids a token-level clone with WeekView's destructure block.
  const props: Props = $props();

  let bodyEl: HTMLDivElement | undefined = $state();

  const dayItems = $derived(filterByDay(props.items, (item) => item.chunk.start_time, props.date));
  const dayExternals = $derived(
    filterByDay(props.externalEvents ?? [], (e) => e.start_time, props.date),
  );

  /** Timed externals share the grid; all-day ones render in the top lane. */
  const dayTimedExternals = $derived(dayExternals.filter((e) => !e.all_day));
  const dayAllDayExternals = $derived(dayExternals.filter((e) => e.all_day));

  /** Chunks and timed external events lane together; externals take the leftmost lanes. */
  const dayLayout = $derived.by(() => layoutDayColumn(dayItems, dayTimedExternals));

  const eventOpenHandler = $derived(
    makeEventOpenHandler(props.oneventopen ?? null, props.editableCalendarId ?? null),
  );

  const initialScrollTop = $derived.by(() =>
    earliestWindowScrollTop(
      (props.windows ?? []).filter((window) => window.day_of_week === weekdayName(props.date)),
    ),
  );

  $effect(() => {
    if (!bodyEl) return;
    bodyEl.scrollTop = initialScrollTop;
  });
</script>

<div class="day-view">
  <div class="day-header">
    <span class="day-label">
      {props.date.toLocaleDateString(undefined, {
        weekday: 'short',
        month: 'short',
        day: 'numeric',
      })}
    </span>
  </div>

  {#if dayAllDayExternals.length > 0}
    <div class="all-day-lane" aria-label="All-day events">
      {#each dayAllDayExternals as ext (ext.event_id)}
        <ExternalEventBlock
          event={ext}
          onopen={eventOpenHandler(ext)}
          disconnected={props.disconnected}
        />
      {/each}
    </div>
  {/if}

  <div bind:this={bodyEl} class="day-body">
    <TimeGrid date={props.date}>
      <ScheduleWindowOverlay windows={props.windows ?? []} date={props.date} />
      <PastWash heightPx={pastOverlayHeightPx(props.date, props.now)} />
      <DayExternalsLayer
        externals={dayLayout.externals}
        {eventOpenHandler}
        disconnected={props.disconnected}
      />
      <ChunkCreateLayer columnDate={props.date} oncreatechunk={props.oncreatechunk} />
      <DragOverlay />
      {#if dayLayout.chunks.length === 0}
        <p class="empty-state">No chunks scheduled</p>
      {:else}
        {#each dayLayout.chunks as layout (layout.item.chunk.id)}
          <ChunkBlock
            item={layout.item}
            overlapIndex={layout.overlapIndex}
            overlapCount={layout.overlapCount}
            columnDate={props.date}
            now={props.now}
            onopen={props.onchunkopen}
            oncomplete={props.onchunkcomplete}
            onmove={props.onchunkmove}
            onresize={props.onchunkresize}
            onmenu={props.onchunkmenu}
            onlock={props.onchunklock}
          />
        {/each}
      {/if}
    </TimeGrid>
  </div>
</div>

<style>
  .day-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }

  .day-label {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-secondary);
  }

  /* Compact all-day lane above the timed grid; stays fixed while the grid scrolls. */
  .all-day-lane {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex-shrink: 0;
    max-height: 4.5rem;
    overflow-y: auto;
    padding: var(--spacing-1) var(--spacing-2);
    border-bottom: 1px solid var(--color-border-light);
    background: var(--color-bg-secondary);
  }

  .day-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: var(--spacing-2);
  }

  .empty-state {
    padding: var(--spacing-6) var(--spacing-4);
    text-align: center;
    color: var(--color-text-tertiary);
    font-size: var(--font-size-sm);
    pointer-events: none;
  }
</style>
