<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { dragState, topPxToIso } from './dragState.svelte';
  import { formatTime } from '../../utils';

  /** The snapped start time label shown inside the ghost. */
  const snappedTimeLabel = $derived.by(() => {
    const d = dragState.active;
    if (!d) return '';
    const col = d.columnDate ?? new Date(d.originalStartTime);
    const startIso = topPxToIso(d.currentTopPx, col);
    const endMs = new Date(startIso).getTime() + d.durationMs;
    return `${formatTime(startIso)} – ${formatTime(new Date(endMs).toISOString())}`;
  });

  /** The time range label shown during resize. */
  const resizeTimeLabel = $derived.by(() => {
    const r = dragState.resizing;
    if (!r) return '';
    const col = r.columnDate ?? new Date(r.originalStartTime);
    const endIso = topPxToIso(r.topPx + r.currentHeightPx, col);
    return `${formatTime(r.originalStartTime)} – ${formatTime(endIso)}`;
  });
</script>

{#if dragState.active}
  <div
    class="drag-overlay"
    style="top: {dragState.active.currentTopPx}px; height: {dragState.active.heightPx}px;"
    aria-hidden="true"
  >
    <span class="drag-title">{dragState.active.taskTitle}</span>
    <span class="drag-time">{snappedTimeLabel}</span>
  </div>
{/if}

{#if dragState.resizing}
  <div
    class="drag-overlay drag-overlay--resize"
    style="top: {dragState.resizing.topPx}px; height: {dragState.resizing.currentHeightPx}px;"
    aria-hidden="true"
  >
    <span class="drag-title">{dragState.resizing.taskTitle}</span>
    <span class="drag-time">{resizeTimeLabel}</span>
  </div>
{/if}

<style>
  .drag-overlay {
    position: absolute;
    left: 2px;
    right: 2px;
    padding: var(--spacing-1) var(--spacing-2);
    border-radius: var(--radius-sm);
    border: 2px dashed var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
    font-size: var(--font-size-xs);
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow: hidden;
    pointer-events: none;
    box-sizing: border-box;
    z-index: 20;
    opacity: 0.85;
  }

  .drag-overlay--resize {
    border-color: var(--color-success, var(--color-primary));
    background: color-mix(in srgb, var(--color-success, var(--color-primary)) 18%, transparent);
  }

  .drag-title {
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .drag-time {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-secondary);
    white-space: nowrap;
  }
</style>
