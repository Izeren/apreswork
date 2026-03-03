<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import type { Cadence, Period, Weekday, Window } from '../../types';

  interface Props {
    cadence: Cadence;
    onchange: (cadence: Cadence) => void;
  }

  const { cadence, onchange }: Props = $props();

  const ALL_DAYS: Weekday[] = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  /** Highest selectable day of month — capped at 28 so every month has it. */
  const MAX_DAY_OF_MONTH = 28;

  // Internal state — seeded from the prop once, then managed locally.
  // untrack() suppresses the Svelte warning about reading a prop's initial
  // value in a $state initializer (the one-time seed is intentional here).
  // Windows are the canonical weekly form and are edited directly.

  let period = $state<Period>(untrack(() => cadence.period));

  // Weekly: the in-period day windows, edited directly (click + drag).
  let windows = $state<Window[]>(
    untrack(() =>
      cadence.period === 'Weekly' ? cadence.windows.map((w) => ({ ...w })) : [{ start: 0, end: 0 }],
    ),
  );

  // Monthly: a single 1-based day of month (window start + 1).
  let dayOfMonth = $state<number>(
    untrack(() => (cadence.period === 'Monthly' ? cadence.windows[0].start + 1 : 1)),
  );

  let interval = $state<number>(untrack(() => cadence.interval));
  const intervalUnitBase = $derived(period === 'Weekly' ? 'week' : 'month');
  const intervalUnit = $derived(interval === 1 ? intervalUnitBase : `${intervalUnitBase}s`);

  let daysError = $state('');
  let dayOfMonthError = $state('');
  let intervalError = $state('');

  // Drag-to-select gesture state (weekly). A pointer-down on a day starts a
  // gesture, entering further days extends it, and release commits one window.
  // A plain click (no movement) toggles a single-day window — that path also
  // serves keyboard users, since Enter/Space fire `click` on the native button.

  let pointerActive = $state(false);
  let dragAnchor = $state<number | null>(null);
  let dragCurrent = $state<number | null>(null);
  let didMove = $state(false);
  // Set on a committed drag so the trailing synthetic click is ignored.
  let justDragged = false;

  const pendingRange = $derived(
    pointerActive && didMove && dragAnchor !== null && dragCurrent !== null
      ? { lo: Math.min(dragAnchor, dragCurrent), hi: Math.max(dragAnchor, dragCurrent) }
      : null,
  );

  function findContainingWindow(offset: number): Window | undefined {
    return windows.find((w) => w.start <= offset && w.end >= offset);
  }

  function isCovered(offset: number): boolean {
    return findContainingWindow(offset) !== undefined;
  }

  function inPending(offset: number): boolean {
    return pendingRange !== null && offset >= pendingRange.lo && offset <= pendingRange.hi;
  }

  type MarkerRole = 'none' | 'single' | 'start' | 'mid' | 'end';

  function markerRole(offset: number): MarkerRole {
    const w = findContainingWindow(offset);
    if (!w) return 'none';
    if (w.start === w.end) return 'single';
    if (offset === w.start) return 'start';
    if (offset === w.end) return 'end';
    return 'mid';
  }

  const markers = $derived(ALL_DAYS.map((_, offset) => markerRole(offset)));

  function sortWindows(wins: Window[]): Window[] {
    return [...wins].sort((a, b) => a.start - b.start || a.end - b.end);
  }

  function weeklyCadence(wins: Window[]): Cadence {
    return { period: 'Weekly', interval, windows: sortWindows(wins).map((w) => ({ ...w })) };
  }

  function monthlyCadence(dom: number): Cadence {
    return { period: 'Monthly', interval, windows: [{ start: dom - 1, end: dom - 1 }] };
  }

  function validateMonthly(dom: number): boolean {
    dayOfMonthError =
      dom < 1 || dom > MAX_DAY_OF_MONTH ? `Must be between 1 and ${MAX_DAY_OF_MONTH}` : '';
    return dayOfMonthError === '';
  }

  function emitWeekly(): void {
    if (windows.length === 0) {
      daysError = 'Select at least one day';
      return;
    }
    daysError = '';
    if (intervalError) return;
    onchange(weeklyCadence(windows));
  }

  function emitMonthly(): void {
    if (!validateMonthly(dayOfMonth)) return;
    if (intervalError) return;
    onchange(monthlyCadence(dayOfMonth));
  }

  /** Toggle a single-day window: add it when the day is free, otherwise clear
   *  the window that contains it (a click on any covered day removes its whole
   *  window — the simplest predictable "remove" that never grows the count). */
  function toggleDay(offset: number): void {
    const containing = findContainingWindow(offset);
    windows = containing
      ? windows.filter((w) => w !== containing)
      : sortWindows([...windows, { start: offset, end: offset }]);
    emitWeekly();
  }

  /** Commit a drag span [lo, hi]: union it with any overlapping windows so the
   *  result stays sorted and non-overlapping (the domain rejects overlap). */
  function paintRange(lo: number, hi: number): void {
    const overlapping = windows.filter((w) => w.start <= hi && w.end >= lo);
    const start = Math.min(lo, ...overlapping.map((w) => w.start));
    const end = Math.max(hi, ...overlapping.map((w) => w.end));
    windows = sortWindows([...windows.filter((w) => !overlapping.includes(w)), { start, end }]);
    emitWeekly();
  }

  function handleDayClick(offset: number): void {
    if (justDragged) {
      justDragged = false;
      return;
    }
    toggleDay(offset);
  }

  function handlePointerDown(offset: number): void {
    justDragged = false;
    pointerActive = true;
    dragAnchor = offset;
    dragCurrent = offset;
    didMove = false;
  }

  function handlePointerEnter(offset: number): void {
    if (!pointerActive) return;
    dragCurrent = offset;
    if (offset !== dragAnchor) didMove = true;
  }

  function handlePointerUp(): void {
    if (!pointerActive) return;
    if (didMove && dragAnchor !== null && dragCurrent !== null) {
      paintRange(Math.min(dragAnchor, dragCurrent), Math.max(dragAnchor, dragCurrent));
      justDragged = true;
    }
    pointerActive = false;
    dragAnchor = null;
    dragCurrent = null;
    didMove = false;
  }

  function handleTypeChange(next: Period): void {
    period = next;
    daysError = '';
    dayOfMonthError = '';

    if (next === 'Monthly') {
      emitMonthly();
    } else {
      if (windows.length === 0) windows = [{ start: 0, end: 0 }];
      emitWeekly();
    }
  }

  function handleIntervalChange(raw: string): void {
    const value = Number(raw);
    if (!Number.isFinite(value) || !Number.isInteger(value) || value < 1) {
      intervalError = 'Must be a whole number ≥ 1';
      return;
    }
    intervalError = '';
    interval = value;
    if (period === 'Monthly') emitMonthly();
    else emitWeekly();
  }

  function handleDayOfMonthChange(raw: string): void {
    const value = parseInt(raw, 10);
    if (!Number.isFinite(value)) {
      dayOfMonthError = 'Enter a number';
      return;
    }
    // Persist only a valid value (mirrors the interval handler) — keeping a bad
    // value would make a later valid edit, e.g. the interval, silently no-op.
    if (!validateMonthly(value)) return;
    dayOfMonth = value;
    if (intervalError) return;
    onchange(monthlyCadence(value));
  }
</script>

<svelte:window onpointerup={handlePointerUp} />

<div class="recurring-section">
  <div class="form-field">
    <span class="field-label">Cadence type <span class="required">*</span></span>
    <div class="type-selector">
      <label class="radio-label">
        <input
          type="radio"
          name="cadence-type"
          value="Weekly"
          checked={period === 'Weekly'}
          onchange={() => handleTypeChange('Weekly')}
        />
        <span>Weekly</span>
      </label>
      <label class="radio-label">
        <input
          type="radio"
          name="cadence-type"
          value="Monthly"
          checked={period === 'Monthly'}
          onchange={() => handleTypeChange('Monthly')}
        />
        <span>Monthly</span>
      </label>
    </div>
  </div>

  <div class="form-field">
    <label class="field-label" for="cadence-interval">Repeat every</label>
    <div class="interval-row">
      <input
        id="cadence-interval"
        class="field-input field-input--narrow"
        class:field-error={!!intervalError}
        type="number"
        min="1"
        value={interval}
        oninput={(e) => handleIntervalChange((e.target as HTMLInputElement).value)}
        aria-describedby={intervalError ? 'cadence-interval-error' : undefined}
        aria-invalid={!!intervalError}
      />
      <span class="interval-unit">{intervalUnit}</span>
    </div>
    {#if intervalError}
      <span id="cadence-interval-error" class="error-text" role="alert">{intervalError}</span>
    {/if}
  </div>

  {#if period === 'Weekly'}
    <div class="form-field">
      <span class="field-label">Days <span class="required">*</span></span>
      <div class="day-picker" role="group" aria-label="Days of week">
        {#each ALL_DAYS as day, offset (day)}
          <button
            type="button"
            class="day-btn"
            class:day-btn--selected={isCovered(offset)}
            class:day-btn--pending={inPending(offset)}
            aria-pressed={isCovered(offset)}
            onclick={() => handleDayClick(offset)}
            onpointerdown={() => handlePointerDown(offset)}
            onpointerenter={() => handlePointerEnter(offset)}
          >
            {day}
          </button>
        {/each}
        {#each markers as role, offset (offset)}
          <span class="marker marker--{role}" aria-hidden="true"></span>
        {/each}
      </div>
      <p class="cadence-hint" aria-live="polite">
        {windows.length} instance{windows.length === 1 ? '' : 's'} per week
      </p>
      {#if daysError}
        <span class="error-text" role="alert">{daysError}</span>
      {/if}
    </div>
  {:else}
    <div class="form-field">
      <label class="field-label" for="day-of-month">
        Day of month <span class="required">*</span>
      </label>
      <input
        id="day-of-month"
        class="field-input field-input--narrow"
        class:field-error={!!dayOfMonthError}
        type="number"
        min="1"
        max={MAX_DAY_OF_MONTH}
        value={dayOfMonth}
        oninput={(e) => handleDayOfMonthChange((e.target as HTMLInputElement).value)}
        aria-describedby={dayOfMonthError ? 'day-of-month-error' : undefined}
        aria-invalid={!!dayOfMonthError}
      />
      {#if dayOfMonthError}
        <span id="day-of-month-error" class="error-text" role="alert">{dayOfMonthError}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .recurring-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .type-selector {
    display: flex;
    gap: var(--spacing-4);
  }

  .radio-label {
    display: flex;
    user-select: none;
  }

  .interval-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
  }

  .interval-unit {
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
  }

  /* Two-row grid: day buttons (row 1) over window markers (row 2), sharing the
     same 7 columns so each bracket lines up under its days. column-gap is 0 so
     a multi-day marker reads as one continuous bracket. */
  .day-picker {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    column-gap: 0;
    row-gap: var(--spacing-1);
  }

  .day-btn {
    margin: 0 2px;
    padding: var(--spacing-1) var(--spacing-2);
    border: 1px solid var(--color-border);
    border-radius: 4px;
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    cursor: pointer;
    touch-action: none;
    transition:
      background 0.15s,
      border-color 0.15s,
      color 0.15s;
  }

  .day-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .day-btn--selected {
    background: var(--color-primary-light);
    border-color: var(--color-primary);
    color: var(--color-primary);
    font-weight: var(--font-weight-medium);
  }

  .day-btn--pending {
    border-color: var(--color-primary);
    border-style: dashed;
  }

  .marker {
    height: 7px;
    margin-top: 1px;
  }

  .marker--single,
  .marker--start,
  .marker--mid,
  .marker--end {
    border-bottom: 2px solid var(--color-primary);
  }

  .marker--start,
  .marker--single {
    border-left: 2px solid var(--color-primary);
  }

  .marker--end,
  .marker--single {
    border-right: 2px solid var(--color-primary);
  }

  .cadence-hint {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
  }

  .field-input--narrow {
    width: 96px;
  }

  .error-text {
    font-size: var(--font-size-xs);
    color: var(--color-error);
  }
</style>
