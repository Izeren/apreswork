<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import {
    buildCalendarDays,
    fromLocalDateString,
    shiftMonth,
    startOfMonth,
    weekdayLabels,
  } from './dateTimePickerShared';
  import { loadWeekStart } from '../../weekStartPref';
  import type { WeekStart } from '../../utils';

  interface Props {
    /** Local date (YYYY-MM-DD) marked as selected; also the initial month. */
    selected?: string | null;
    onpick: (localDate: string) => void;
    today: Date;
    /**
     * Size variant. 'sm' = 26 px day buttons (default, current standalone
     * usage). 'md' = 30 px (used when DateTimePicker embeds this component).
     */
    size?: 'sm' | 'md';
    /**
     * Override the week-start. When absent the persisted localStorage
     * preference is used. Pass this when the host component owns the weekStart
     * toggle (e.g. DateTimePicker) so the grid reacts to toggle changes.
     */
    weekStart?: WeekStart;
  }

  const { selected = null, onpick, today, size = 'sm', weekStart: weekStartProp }: Props = $props();

  const resolvedWeekStart = $derived(weekStartProp ?? loadWeekStart(window.localStorage));
  const weekdays = $derived(weekdayLabels(resolvedWeekStart));

  let visibleMonth = $state(
    startOfMonth(untrack(() => (selected ? fromLocalDateString(selected) : today))),
  );

  const monthLabel = $derived(
    visibleMonth.toLocaleDateString(undefined, { month: 'long', year: 'numeric' }),
  );
  const days = $derived(buildCalendarDays(visibleMonth, resolvedWeekStart, today));
</script>

<div class="mini-calendar" class:mini-calendar--md={size === 'md'}>
  <div class="calendar-header">
    <button
      type="button"
      class="calendar-nav-btn nav-btn"
      aria-label="Previous month"
      onclick={() => (visibleMonth = shiftMonth(visibleMonth, -1))}
    >
      &#x2039;
    </button>
    <strong>{monthLabel}</strong>
    <button
      type="button"
      class="calendar-nav-btn nav-btn"
      aria-label="Next month"
      onclick={() => (visibleMonth = shiftMonth(visibleMonth, 1))}
    >
      &#x203A;
    </button>
  </div>

  <div class="weekday-row" aria-hidden="true">
    {#each weekdays as weekday (weekday)}
      <span>{weekday}</span>
    {/each}
  </div>

  <div class="day-grid">
    {#each days as day (day.date)}
      <button
        type="button"
        class="calendar-day-btn day-btn"
        class:calendar-day-btn--outside={!day.isCurrentMonth}
        class:calendar-day-btn--today={day.isToday}
        class:calendar-day-btn--selected={day.date === selected}
        data-date={day.date}
        aria-label={`Choose ${day.date}`}
        onclick={() => onpick(day.date)}
      >
        {day.label}
      </button>
    {/each}
  </div>
</div>

<style>
  .mini-calendar {
    width: 230px;
  }

  .calendar-header {
    margin-bottom: var(--spacing-1);
    font-size: var(--font-size-sm);
  }

  .nav-btn {
    min-width: 26px;
    min-height: 26px;
    border: none;
    border-radius: var(--radius-sm);
  }

  .nav-btn:focus-visible {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .weekday-row,
  .day-grid {
    gap: 2px;
  }

  .weekday-row {
    margin-bottom: 2px;
  }

  .day-btn {
    min-height: 26px;
    font-size: var(--font-size-xs);
    border-radius: var(--radius-sm);
  }

  .mini-calendar--md {
    width: 100%;
  }

  .mini-calendar--md .calendar-header {
    margin-bottom: var(--spacing-2);
  }

  .mini-calendar--md .nav-btn {
    min-width: 30px;
    min-height: 30px;
    border: 1px solid transparent;
  }

  .mini-calendar--md .nav-btn:hover {
    border-color: var(--color-border-light);
  }

  .mini-calendar--md .weekday-row,
  .mini-calendar--md .day-grid {
    gap: var(--spacing-1);
  }

  .mini-calendar--md .weekday-row {
    margin-bottom: var(--spacing-1);
  }

  .mini-calendar--md .day-btn {
    min-height: 30px;
    font-size: var(--font-size-sm);
    box-shadow: none;
  }
</style>
