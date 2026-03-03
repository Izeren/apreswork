<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<!-- Empty-slot chunk creation for one day column: a crosshair hit area that
     drives the shared create drag state, plus the live selection rectangle.
     Rendered inside a positioned column within the time grid. -->

<script lang="ts">
  import { clientYToTopPx, dragState, getCreateBounds, topPxToIso } from './dragState.svelte';
  import { HOUR_HEIGHT_PX } from './calendarLayout';
  import { isSameLocalDate } from '../../utils';

  interface Props {
    /** The day this layer creates chunks for (its column's date). */
    columnDate: Date;
    /** Accessible label for the hit area. */
    ariaLabel?: string;
    /** Called when the user creates a chunk from an empty slot selection. */
    oncreatechunk?: ((start: string, end: string) => void) | null;
  }

  const { columnDate, ariaLabel = 'Create chunk', oncreatechunk = null }: Props = $props();

  let createPointerId: number | null = $state(null);

  /** Whether the in-flight create selection belongs to this layer's column. */
  function isCreatingHere(): boolean {
    const creatingDate = dragState.creating?.columnDate;
    return creatingDate != null && isSameLocalDate(creatingDate, columnDate);
  }

  function commitCreateSelection(): void {
    if (!oncreatechunk) return;
    const final = dragState.endCreate();
    if (!final || !final.columnDate) return;
    const { topPx, heightPx } = getCreateBounds(final);
    const start = topPxToIso(topPx, final.columnDate);
    const durationMinutes = Math.round((heightPx / HOUR_HEIGHT_PX) * 60);
    const end = new Date(new Date(start).getTime() + durationMinutes * 60_000).toISOString();
    oncreatechunk(start, end);
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    if (dragState.active || dragState.resizing) return;

    const grid = (event.currentTarget as HTMLElement).closest('[aria-label="Time grid"]');
    if (!(grid instanceof HTMLElement)) return;

    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    createPointerId = event.pointerId;
    const anchorTopPx = clientYToTopPx(event.clientY, grid.getBoundingClientRect());
    dragState.startCreate({ anchorTopPx, currentTopPx: anchorTopPx, columnDate });
  }

  function handlePointerMove(event: PointerEvent): void {
    if (!dragState.creating) return;
    const grid = (event.currentTarget as HTMLElement).closest('[aria-label="Time grid"]');
    if (!(grid instanceof HTMLElement)) return;
    dragState.updateCreatePosition(event.clientY, grid.getBoundingClientRect());
  }

  function handlePointerUp(event: PointerEvent): void {
    if (!dragState.creating) return;
    event.preventDefault();
    if (createPointerId !== null) {
      (event.currentTarget as HTMLElement).releasePointerCapture(createPointerId);
      createPointerId = null;
    }
    commitCreateSelection();
  }

  function handlePointerCancel(event: PointerEvent): void {
    if (createPointerId !== null) {
      (event.currentTarget as HTMLElement).releasePointerCapture(createPointerId);
      createPointerId = null;
    }
    dragState.cancelCreate();
  }
</script>

<div
  class="create-hit-area"
  aria-label={ariaLabel}
  role="presentation"
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerCancel}
></div>
{#if dragState.creating && isCreatingHere()}
  {@const bounds = getCreateBounds(dragState.creating)}
  <div
    class="create-selection"
    aria-hidden="true"
    style="top: {bounds.topPx}px; height: {bounds.heightPx}px"
  ></div>
{/if}

<style>
  .create-hit-area {
    position: absolute;
    inset: 0;
    cursor: crosshair;
    z-index: 0;
  }

  .create-selection {
    position: absolute;
    left: 2px;
    right: 12px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 45%, transparent);
    pointer-events: none;
    z-index: 1;
  }
</style>
