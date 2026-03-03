<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { timeToGridHeightPx } from './calendarLayout';
  import { isSameLocalDate } from '../../utils';

  interface Props {
    date: Date;
    showCurrentTimeIndicator?: boolean | null;
    children?: Snippet;
  }

  const { date, showCurrentTimeIndicator = null, children }: Props = $props();

  const HOURS = Array.from({ length: 24 }, (_, i) => i);

  let indicatorTop = $state(timeToGridHeightPx(new Date()));
  let showIndicator = $derived.by(
    () => showCurrentTimeIndicator ?? isSameLocalDate(date, new Date()),
  );

  $effect(() => {
    const id = setInterval(() => {
      indicatorTop = timeToGridHeightPx(new Date());
    }, 60_000);
    return () => clearInterval(id);
  });
</script>

<div class="time-grid" aria-label="Time grid">
  {#each HOURS as hour (hour)}
    <div class="hour-row" style="height: var(--calendar-hour-height)">
      <div class="time-label" aria-hidden="true">
        {hour}:00
      </div>
      <div class="hour-line" aria-hidden="true"></div>
      <div class="half-hour-marker" aria-hidden="true"></div>
    </div>
  {/each}

  <div class="bottom-boundary" aria-hidden="true">
    <div class="time-label" aria-hidden="true"></div>
    <div class="hour-line" aria-hidden="true"></div>
  </div>

  {#if showIndicator}
    <div
      class="time-indicator"
      role="presentation"
      aria-hidden="true"
      style="top: {indicatorTop}px"
    ></div>
  {/if}

  <div class="grid-content" aria-label="Scheduled content">
    {#if children}
      {@render children()}
    {/if}
  </div>
</div>

<style>
  .time-grid {
    position: relative;
    height: calc(24 * var(--calendar-hour-height));
    min-width: 0;
  }

  .hour-row {
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .time-label {
    position: absolute;
    top: -0.5em;
    left: 0;
    width: 52px;
    text-align: right;
    padding-right: var(--spacing-2);
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    font-variant-numeric: tabular-nums;
    user-select: none;
    line-height: 1;
  }

  .hour-line {
    position: absolute;
    top: 0;
    left: 52px;
    right: 0;
    height: 1px;
    background: var(--color-hour-line);
  }

  .half-hour-marker {
    position: absolute;
    top: 50%;
    left: 52px;
    right: 0;
    height: 1px;
    background: var(--color-half-hour-line);
  }

  .bottom-boundary {
    position: relative;
    height: 1px;
  }

  .bottom-boundary .hour-line {
    top: 0;
  }

  .time-indicator {
    position: absolute;
    left: 52px;
    right: 0;
    height: 2px;
    background: var(--color-time-indicator);
    z-index: 10;
    pointer-events: none;
  }

  .grid-content {
    position: absolute;
    top: 0;
    left: 52px;
    right: 0;
    bottom: 0;
    pointer-events: none;
  }
</style>
