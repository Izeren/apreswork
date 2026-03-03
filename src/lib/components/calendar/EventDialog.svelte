<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts" module>
  export interface EventDialogInitial {
    title: string;
    description: string | null;
    /** ISO instant. For all-day, Local midnight of the first day. */
    start: string;
    /** ISO instant. For all-day, Local midnight of the day AFTER the last day (exclusive). */
    end: string;
    all_day: boolean;
  }
</script>

<script lang="ts">
  import Modal from '../shared/Modal.svelte';
  import DateTimePicker from '../shared/DateTimePicker.svelte';
  import type { UserEventPayload } from '../../types';
  import { isoToLocalDate } from '../shared/dateTimePickerShared';
  import { buildAllDayRange, allDayEndToInclusiveDate } from './eventDialogShared';
  import { appClock } from '../../app-clock';

  interface Props {
    open: boolean;
    mode: 'create' | 'edit';
    initial: EventDialogInitial;
    /** True while the parent's create/update/delete call is in flight. */
    busy?: boolean;
    /** Parent-supplied error (e.g. a rejected Google write) shown in the banner. */
    error?: string | null;
    /** Emits the write payload; the parent performs the API call and refetch. */
    onsubmit: (payload: UserEventPayload) => void;
    /** Wired in edit mode only; absent ⇒ no Delete affordance. */
    ondelete?: (() => void) | null;
    oncancel: () => void;
    /** Injected clock for date label computation in pickers. */
    getNow?: () => Date;
  }

  const {
    open,
    mode,
    initial,
    busy = false,
    error = null,
    onsubmit,
    ondelete = null,
    oncancel,
    getNow = appClock,
  }: Props = $props();

  let title = $state('');
  let description = $state('');
  let allDay = $state(false);
  let timedStart = $state<string | null>(null);
  let timedEnd = $state<string | null>(null);
  let allDayStartDate = $state('');
  /** INCLUSIVE last day shown to the user; converted to an exclusive end on submit. */
  let allDayEndDate = $state('');
  let validationError = $state<string | null>(null);
  let deleteConfirming = $state(false);

  // Seed local state each time the dialog opens (mirrors SharedFormFields). Reads
  // only `open`/`initial`, so assignments below never re-trigger the effect.
  $effect(() => {
    if (!open) return;
    title = initial.title;
    description = initial.description ?? '';
    allDay = initial.all_day;
    validationError = null;
    deleteConfirming = false;

    timedStart = initial.start;
    timedEnd = initial.end;

    if (initial.all_day) {
      allDayStartDate = isoToLocalDate(initial.start);
      allDayEndDate = allDayEndToInclusiveDate(initial.end);
    } else {
      initAllDayDatesFromTimed(initial.start);
    }
  });

  function initAllDayDatesFromTimed(iso: string): void {
    const day = isoToLocalDate(iso);
    allDayStartDate = day;
    allDayEndDate = day;
  }

  const dialogTitle = $derived(mode === 'create' ? 'New event' : 'Edit event');
  const idleLabel = $derived(mode === 'create' ? 'Create event' : 'Save changes');
  const primaryLabel = $derived(busy ? 'Saving…' : idleLabel);
  const shownError = $derived(validationError ?? error);

  /**
   * Toggle timed↔all-day. Both representations are seeded on open, so switching
   * only flips the mode; when moving to all-day we carry the timed start's date
   * across so a single-day event keeps the day the user was looking at.
   */
  function setAllDay(next: boolean): void {
    if (next === allDay) return;
    allDay = next;
    validationError = null;
    if (next && timedStart) {
      initAllDayDatesFromTimed(timedStart);
    }
  }

  function buildPayload(): UserEventPayload | null {
    const trimmed = title.trim();
    if (!trimmed) {
      validationError = 'Title is required';
      return null;
    }
    const desc = description.trim() || null;

    if (allDay) {
      if (!allDayStartDate || !allDayEndDate) {
        validationError = 'Pick a start and end date';
        return null;
      }
      if (allDayEndDate < allDayStartDate) {
        validationError = 'End date must be on or after the start date';
        return null;
      }
      const { start, end } = buildAllDayRange(allDayStartDate, allDayEndDate);
      return { title: trimmed, description: desc, start, end, all_day: true };
    }

    if (!timedStart || !timedEnd) {
      validationError = 'Pick a start and end time';
      return null;
    }
    const startMs = new Date(timedStart).getTime();
    const endMs = new Date(timedEnd).getTime();
    if (endMs <= startMs) {
      validationError = 'End must be after the start';
      return null;
    }
    return { title: trimmed, description: desc, start: timedStart, end: timedEnd, all_day: false };
  }

  function handleSubmit(event: Event): void {
    event.preventDefault();
    validationError = null;
    const payload = buildPayload();
    if (payload) onsubmit(payload);
  }

  /** Two-step confirm: first click arms, second click commits the destructive delete. */
  function handleDeleteClick(): void {
    if (!deleteConfirming) {
      deleteConfirming = true;
      return;
    }
    ondelete?.();
  }
</script>

<Modal {open} title={dialogTitle} onclose={oncancel}>
  <form class="event-form" onsubmit={handleSubmit} novalidate>
    <div class="form-field">
      <label class="field-label" for="event-title">Title <span class="required">*</span></label>
      <input
        id="event-title"
        class="field-input"
        type="text"
        placeholder="Event title"
        bind:value={title}
      />
    </div>

    <div class="form-field">
      <label class="field-label" for="event-description">Description</label>
      <textarea
        id="event-description"
        class="field-textarea"
        placeholder="Optional description"
        rows="3"
        bind:value={description}
      ></textarea>
    </div>

    <div class="form-field form-field--checkbox">
      <label class="checkbox-label">
        <input
          type="checkbox"
          checked={allDay}
          onchange={(event) => setAllDay(event.currentTarget.checked)}
        />
        <span>All day</span>
      </label>
    </div>

    {#if allDay}
      <div class="form-row">
        <div class="form-field">
          <label class="field-label" for="event-start-date">Start date</label>
          <input
            id="event-start-date"
            class="field-input"
            type="date"
            bind:value={allDayStartDate}
          />
        </div>
        <div class="form-field">
          <label class="field-label" for="event-end-date">End date</label>
          <input id="event-end-date" class="field-input" type="date" bind:value={allDayEndDate} />
        </div>
      </div>
    {:else}
      <div class="form-row">
        <div class="form-field">
          <DateTimePicker
            label="Start"
            value={timedStart}
            now={getNow()}
            onchange={(v) => (timedStart = v)}
          />
        </div>
        <div class="form-field">
          <DateTimePicker
            label="End"
            value={timedEnd}
            popoverAlign="end"
            now={getNow()}
            onchange={(v) => (timedEnd = v)}
          />
        </div>
      </div>
    {/if}

    {#if shownError}
      <div class="dialog-error" role="alert">{shownError}</div>
    {/if}

    <div class="dialog-actions">
      {#if mode === 'edit' && ondelete}
        <button type="button" class="btn-danger" onclick={handleDeleteClick} disabled={busy}>
          {deleteConfirming ? 'Confirm delete' : 'Delete'}
        </button>
      {/if}
      <span class="actions-spacer"></span>
      <button type="button" class="btn-cancel" onclick={oncancel} disabled={busy}>Cancel</button>
      <button type="submit" class="btn-primary" disabled={busy}>{primaryLabel}</button>
    </div>
  </form>
</Modal>

<style>
  .event-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .form-field--checkbox {
    flex-direction: row;
    align-items: center;
  }

  .checkbox-label {
    display: inline-flex;
  }

  .checkbox-label input {
    width: auto;
  }

  .dialog-error {
    font-size: var(--font-size-sm);
    color: var(--color-error);
    padding: var(--spacing-2) var(--spacing-3);
    border: 1px solid var(--color-error);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-error) 8%, transparent);
  }

  .dialog-actions {
    display: flex;
    align-items: center;
    gap: var(--spacing-3);
  }

  .actions-spacer {
    flex: 1 1 auto;
  }

  .btn-cancel {
    background: var(--color-surface);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
  }

  .btn-cancel:hover:enabled {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }
</style>
