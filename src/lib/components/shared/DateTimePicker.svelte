<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import {
    FALLBACK_TIME,
    PICKER_MARGIN,
    PICKER_GAP,
    isoToLocalDate,
    getRelativeDateLabel,
    formatDisplayDate,
    formatShortcutDate,
    buildQuickDateOptions,
    getInitialTime,
    getTimezoneHint,
    type QuickDateOption,
  } from './dateTimePickerShared';
  import { repositionOnViewportChange } from './viewportReposition.svelte';
  import { computePositioningStyle } from './popoverPosition';
  import TimeMenu from './TimeMenu.svelte';
  import MiniCalendar from './MiniCalendar.svelte';
  import { loadWeekStart, saveWeekStart } from '../../weekStartPref';
  import { loadQuickDateAnchor } from '../../quickDateAnchorPref';
  import type { WeekStart } from '../../utils';

  interface Props {
    value: string | null;
    onchange: (iso: string | null) => void;
    label?: string;
    nullable?: boolean;
    disabled?: boolean;
    defaultTime?: string | null;
    popoverAlign?: 'start' | 'end';
    now: Date;
  }

  const {
    value,
    onchange,
    label,
    nullable = false,
    disabled = false,
    defaultTime = null,
    popoverAlign = 'start',
    now,
  }: Props = $props();

  const POPOVER_MAX_WIDTH = 560;

  const timezoneHint = $derived(getTimezoneHint(now));

  // Set from the Settings page; read once per mount (pickers are short-lived).
  const quickDateAnchor = loadQuickDateAnchor(window.localStorage);

  // State — weekStart is declared before syncFromValue's first call (it reads
  // weekStart to build quick-date options).

  let weekStart = $state<WeekStart>(loadWeekStart(window.localStorage));
  let rootEl: HTMLDivElement | null = $state(null);
  let triggerEl: HTMLButtonElement | null = $state(null);
  let popoverOpen = $state(false);
  let timeMenuOpen = $state(false);
  let lastPropValue = $state<string | null>(untrack(() => value));
  let draftDate = $state('');
  let draftTime = $state(
    getInitialTime(
      untrack(() => value),
      untrack(() => defaultTime),
    ),
  );
  let popoverStyle = $state('');
  let selectedQuickOptionId = $state<string | null>(null);

  function resolveQuickOptionId(
    localDate: string,
    preferredId: string | null,
    options: QuickDateOption[],
  ): string | null {
    if (!localDate) return null;
    if (
      preferredId &&
      options.some((option) => option.id === preferredId && option.date === localDate)
    ) {
      return preferredId;
    }
    return options.find((option) => option.date === localDate)?.id ?? null;
  }

  function syncFromValue(nextValue: string | null) {
    const nextDate = nextValue ? isoToLocalDate(nextValue) : '';
    draftDate = nextDate;
    draftTime = getInitialTime(nextValue, defaultTime);
    selectedQuickOptionId = resolveQuickOptionId(
      nextDate,
      selectedQuickOptionId,
      buildQuickDateOptions(weekStart, now, quickDateAnchor),
    );
  }

  syncFromValue(untrack(() => value));

  $effect(() => {
    if (value === lastPropValue) return;
    lastPropValue = value;
    syncFromValue(value);
  });

  const triggerDateLabel = $derived(
    draftDate
      ? (getRelativeDateLabel(draftDate, now) ?? formatDisplayDate(draftDate))
      : 'Pick a date',
  );
  const selectedDateLabel = $derived(
    draftDate ? formatShortcutDate(draftDate) : 'No date selected',
  );
  const quickDateOptions = $derived.by(() =>
    buildQuickDateOptions(weekStart, now, quickDateAnchor),
  );

  function commitSelection(nextDate: string | null, nextTime = draftTime) {
    if (!nextDate) {
      onchange(null);
      return;
    }

    const nextValue = new Date(`${nextDate}T${nextTime}:00`);
    onchange(nextValue.toISOString());
  }

  function selectDate(nextDate: string, preferredQuickOptionId: string | null = null) {
    draftDate = nextDate;
    selectedQuickOptionId = resolveQuickOptionId(
      nextDate,
      preferredQuickOptionId,
      quickDateOptions,
    );
    commitSelection(nextDate);
    closePopover();
  }

  function selectQuickDate(optionId: string, nextDate: string) {
    selectDate(nextDate, optionId);
  }

  function selectTime(nextTime: string) {
    draftTime = nextTime;
    if (draftDate) {
      commitSelection(draftDate, nextTime);
    }
  }

  function clearSelection() {
    draftDate = '';
    draftTime = defaultTime ?? FALLBACK_TIME;
    selectedQuickOptionId = null;
    closePopover();
    onchange(null);
  }

  function setWeekStart(next: WeekStart) {
    if (next === weekStart) return;
    weekStart = next;
    saveWeekStart(next, window.localStorage);
    // The anchor-dependent quick dates have shifted — re-resolve to keep the
    // highlighted option consistent with the new week anchor.
    selectedQuickOptionId = resolveQuickOptionId(
      draftDate,
      selectedQuickOptionId,
      buildQuickDateOptions(next, now, quickDateAnchor),
    );
  }

  function togglePopover() {
    if (disabled) return;
    popoverOpen = !popoverOpen;
    if (!popoverOpen) {
      timeMenuOpen = false;
    }
    if (popoverOpen) {
      queueMicrotask(() => {
        updatePopoverPosition();
      });
    }
  }

  function closePopover() {
    popoverOpen = false;
    timeMenuOpen = false;
  }

  function handleDocumentPointerDown(event: PointerEvent) {
    if (!popoverOpen || !rootEl) return;
    if (rootEl.contains(event.target as Node)) return;
    closePopover();
  }

  function handleDocumentKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      if (timeMenuOpen) {
        timeMenuOpen = false;
        return;
      }
      closePopover();
    }
  }

  function updatePopoverPosition() {
    if (!popoverOpen || !triggerEl || typeof window === 'undefined') return;

    const rect = triggerEl.getBoundingClientRect();
    const width = Math.min(POPOVER_MAX_WIDTH, window.innerWidth - PICKER_MARGIN * 2);
    const estimatedHeight = window.innerWidth <= 720 ? 520 : 360;

    let left = popoverAlign === 'end' ? rect.right - width : rect.left;
    left = Math.max(PICKER_MARGIN, Math.min(left, window.innerWidth - width - PICKER_MARGIN));

    const preferredBelow = rect.bottom + PICKER_GAP;
    const preferredAbove = rect.top - PICKER_GAP - estimatedHeight;
    const canOpenBelow = preferredBelow + estimatedHeight <= window.innerHeight - PICKER_MARGIN;
    const canOpenAbove = preferredAbove >= PICKER_MARGIN;

    let top = preferredBelow;
    if (!canOpenBelow && canOpenAbove) {
      top = preferredAbove;
    } else if (!canOpenBelow) {
      top = Math.max(PICKER_MARGIN, window.innerHeight - estimatedHeight - PICKER_MARGIN);
    }

    popoverStyle = computePositioningStyle(
      top,
      left,
      width,
      `calc(100vh - ${PICKER_MARGIN * 2}px)`,
    );
  }

  repositionOnViewportChange(() => popoverOpen, updatePopoverPosition);
</script>

<svelte:document onpointerdown={handleDocumentPointerDown} onkeydown={handleDocumentKeydown} />

<div class="datetime-picker" bind:this={rootEl}>
  {#if label}
    <span class="picker-label">{label}</span>
  {/if}

  <div class="trigger-row">
    <button
      bind:this={triggerEl}
      class="picker-trigger"
      type="button"
      aria-haspopup="dialog"
      aria-expanded={popoverOpen}
      aria-label={draftDate
        ? `Selected ${triggerDateLabel} at ${draftTime}`
        : 'Choose date and time'}
      {disabled}
      onclick={togglePopover}
    >
      <span class:trigger-placeholder={!draftDate}>{triggerDateLabel}</span>
      <span class="trigger-separator"></span>
      <span class="trigger-time">{draftTime}</span>
    </button>

    {#if nullable && draftDate}
      <button
        class="clear-btn"
        type="button"
        aria-label="Clear date and time"
        {disabled}
        onclick={clearSelection}>Clear</button
      >
    {/if}
  </div>

  {#if popoverOpen}
    <div
      class="picker-popover"
      style={popoverStyle}
      role="dialog"
      aria-label={label ?? 'Date and time picker'}
    >
      <div class="popover-grid">
        <section class="calendar-panel" aria-label="Calendar">
          <div class="calendar-topbar">
            <div class="selection-summary">
              <span class="panel-heading">Selected date</span>
              <strong class="selection-value">{selectedDateLabel}</strong>
            </div>

            <div class="time-panel">
              <span class="panel-heading">Time</span>
              <TimeMenu bind:open={timeMenuOpen} value={draftTime} onselect={selectTime} />
            </div>
          </div>

          <MiniCalendar
            selected={draftDate || null}
            onpick={selectDate}
            size="md"
            {weekStart}
            today={now}
          />
        </section>

        <aside class="options-panel">
          <div class="shortcut-panel">
            <span class="panel-heading">Quick dates</span>
            <div class="shortcut-list">
              {#each quickDateOptions as option (option.id)}
                <button
                  type="button"
                  class="option-btn option-row shortcut-btn"
                  class:option-btn--active={selectedQuickOptionId === option.id}
                  data-shortcut={option.id}
                  onclick={() => {
                    selectQuickDate(option.id, option.date);
                  }}
                >
                  <span>{option.label}</span>
                  <span>{formatShortcutDate(option.date)}</span>
                </button>
              {/each}
            </div>
          </div>

          <div class="week-start-panel">
            <span class="panel-heading">Week starts on</span>
            <div class="week-start-toggle" role="group" aria-label="Week starts on">
              <button
                type="button"
                class="option-btn week-start-btn"
                class:option-btn--active={weekStart === 'mon'}
                aria-pressed={weekStart === 'mon'}
                onclick={() => setWeekStart('mon')}>Mon</button
              >
              <button
                type="button"
                class="option-btn week-start-btn"
                class:option-btn--active={weekStart === 'sun'}
                aria-pressed={weekStart === 'sun'}
                onclick={() => setWeekStart('sun')}>Sun</button
              >
            </div>
          </div>
        </aside>
      </div>
    </div>
  {/if}

  <span class="timezone-hint">{timezoneHint}</span>
</div>

<style>
  .datetime-picker {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
  }

  .picker-label {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-text);
  }

  .trigger-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
  }

  .picker-trigger {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-3);
    text-align: left;
  }

  .trigger-placeholder {
    color: var(--color-text-tertiary);
  }

  .trigger-separator {
    flex: 1 1 auto;
    min-width: var(--spacing-2);
    border-bottom: 1px solid var(--color-border-light);
  }

  .trigger-time {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-secondary);
  }

  .picker-popover {
    position: fixed;
    z-index: 1100;
    border: 1px solid var(--color-border-light);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: var(--shadow-lg);
    overflow: auto;
  }

  .popover-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 252px;
  }

  .calendar-panel {
    padding: var(--spacing-3);
    border-right: 1px solid var(--color-border-light);
  }

  .calendar-topbar {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--spacing-3);
    margin-bottom: var(--spacing-2);
  }

  .selection-summary {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .selection-value {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    white-space: nowrap;
  }

  .options-panel {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
    padding: var(--spacing-2);
    background: var(--color-bg-secondary);
  }

  .time-panel,
  .shortcut-panel,
  .week-start-panel {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
  }

  .panel-heading {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .shortcut-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .shortcut-btn {
    font-size: var(--font-size-xs);
  }

  .shortcut-btn span {
    white-space: nowrap;
  }

  /* Week-start toggle — minimal segmented pair, mirrors shortcut-btn visuals. */
  .week-start-toggle {
    display: flex;
    gap: 2px;
  }

  .week-start-btn {
    flex: 1 1 0;
    padding: 5px 8px;
    font-size: var(--font-size-xs);
    text-align: center;
  }

  .clear-btn {
    font-size: var(--font-size-sm);
    padding: var(--spacing-2) var(--spacing-3);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .clear-btn:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .clear-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .timezone-hint {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  @media (max-width: 720px) {
    .picker-popover {
      width: min(100%, calc(100vw - 2rem));
    }

    .popover-grid {
      grid-template-columns: 1fr;
    }

    .calendar-topbar {
      flex-direction: column;
      align-items: stretch;
    }

    .calendar-panel {
      border-right: 0;
      border-bottom: 1px solid var(--color-border-light);
    }
  }
</style>
