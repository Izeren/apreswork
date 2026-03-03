<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { ExternalEvent } from '../../types';
  import { formatTime } from '../../utils';
  import {
    HOUR_HEIGHT_PX,
    CHUNK_MIN_HEIGHT_PX,
    TIME_LABEL_MIN_HEIGHT_PX,
    computeGridBlockStyle,
    timeToGridHeightPx,
  } from './calendarLayout';

  interface Props {
    event: ExternalEvent;
    /** Lane index within an overlapping cluster of external events, starting at 0. */
    overlapIndex?: number;
    /** Number of lanes in the overlapping cluster. Defaults to 1. */
    overlapCount?: number;
    /**
     * When supplied, the block becomes an editable button that calls this with
     * the event on click / Enter / Space. Left null for read-only externals
     * (events on non-editable calendars); the block then renders as an image.
     */
    onopen?: ((event: ExternalEvent) => void) | null;
    /** When true, renders with a disconnected visual (stale data). */
    disconnected?: boolean;
  }

  const {
    event,
    overlapIndex = 0,
    overlapCount = 1,
    onopen = null,
    disconnected = false,
  }: Props = $props();

  const interactive = $derived(onopen != null);
  const isFree = $derived(!event.busy && !event.declined);

  const normalizedOverlapCount = $derived(Math.max(1, Math.trunc(overlapCount) || 1));
  const normalizedOverlapIndex = $derived(
    Math.min(Math.max(0, Math.trunc(overlapIndex) || 0), normalizedOverlapCount - 1),
  );

  const startDate = $derived(new Date(event.start_time));
  const topPx = $derived(timeToGridHeightPx(startDate));

  const heightPx = $derived.by(() => {
    const end = new Date(event.end_time);
    const durationMin = (end.getTime() - startDate.getTime()) / 60_000;
    return Math.max(CHUNK_MIN_HEIGHT_PX, (durationMin / 60) * HOUR_HEIGHT_PX);
  });

  const isOverlap = $derived(normalizedOverlapCount > 1);

  /**
   * Inline positioning for a *timed* block in the grid. All-day events carry no
   * inline style — they are laid out by the parent's all-day lane (flex), so the
   * grid's absolute top/height would be meaningless (their span is a full day).
   */
  const blockStyle = $derived.by(() => {
    if (event.all_day) return '';

    return computeGridBlockStyle({
      topPx,
      heightPx,
      isOverlap,
      overlapIndex: normalizedOverlapIndex,
      overlapCount: normalizedOverlapCount,
      zIndex: 1,
    });
  });

  const ariaLabel = $derived(
    (event.all_day
      ? `External event: ${event.title}, all day`
      : `External event: ${event.title}, ${formatTime(event.start_time)} – ${formatTime(event.end_time)}`) +
      (event.declined ? ', declined' : '') +
      (isFree ? ', free' : ''),
  );

  const showTime = $derived(!event.all_day && heightPx >= TIME_LABEL_MIN_HEIGHT_PX);

  function handleOpen(): void {
    onopen?.(event);
  }

  function handleKeyDown(e: KeyboardEvent): void {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onopen?.(event);
    }
  }
</script>

{#snippet content()}
  <span class="title">{event.title}</span>
  {#if showTime}
    <span class="time">{formatTime(event.start_time)} – {formatTime(event.end_time)}</span>
  {/if}
{/snippet}

<!-- tabindex is undefined when role="img"; the ternary correlation is invisible to static analysis -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="external-event"
  class:external-event--interactive={interactive}
  class:external-event--declined={event.declined}
  class:external-event--free={isFree}
  class:external-event--allday={event.all_day}
  class:external-event--disconnected={disconnected}
  style={blockStyle}
  aria-label={ariaLabel}
  role={interactive ? 'button' : 'img'}
  tabindex={interactive ? 0 : undefined}
  onclick={interactive ? handleOpen : undefined}
  onkeydown={interactive ? handleKeyDown : undefined}
>
  <!-- eslint-disable-next-line sonarjs/no-use-of-empty-return-value -- false positive: snippet render, not a value use -->
  {@render content()}
</div>

<style>
  .external-event {
    position: absolute;
    overflow: hidden;
    pointer-events: none;
    box-sizing: border-box;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-text-secondary) 14%, transparent);
    border: 1px dashed color-mix(in srgb, var(--color-text-secondary) 40%, transparent);
    color: var(--color-text-secondary);
    padding: 2px 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .external-event--interactive {
    pointer-events: auto;
    cursor: pointer;
  }

  .external-event--interactive:hover {
    background: color-mix(in srgb, var(--color-text-secondary) 22%, transparent);
  }

  .external-event--interactive:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .external-event--allday {
    position: relative;
    top: auto;
    left: auto;
    width: auto;
    height: auto;
    min-height: 20px;
    flex-direction: row;
    align-items: center;
    gap: 4px;
  }

  .external-event--declined {
    opacity: 0.5;
  }

  .external-event--free {
    background: transparent;
  }

  .external-event--disconnected {
    opacity: 0.7;
  }

  /* 0.5 × 0.7 — declined+disconnected must be dimmer than either class alone. */
  .external-event--declined.external-event--disconnected {
    opacity: 0.35;
  }

  .title {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.3;
  }

  .external-event--declined .title {
    text-decoration: line-through;
  }

  .time {
    font-size: var(--font-size-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.2;
    opacity: 0.8;
  }
</style>
