<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import type { Chunk, Task, CreateTaskInput, UpdateTaskInput } from '../../types';
  import { defaultTaskFormApi, type TaskFormApi } from './taskFormShared';
  import Modal from '../shared/Modal.svelte';
  import DateTimePicker from '../shared/DateTimePicker.svelte';
  import SharedFormFields from './SharedFormFields.svelte';
  import type { SharedFormController, SharedFieldValues } from './SharedFormFields.svelte';
  import StatusBadge from '../shared/StatusBadge.svelte';
  import CommentSection from './CommentSection.svelte';
  import ChunkList from './ChunkList.svelte';
  import { loadChunkList } from './chunkListLoad';
  import { scheduleState } from '../../stores/schedules.svelte';
  import { calendarFocusState } from '../../stores/calendarFocus.svelte';
  import { router } from '../../router.svelte';
  import { TaskActions } from '../../actions/taskActions';
  import { appClock } from '../../app-clock';

  interface Props {
    open: boolean;
    task?: Task | null;
    initialDurationMinutes?: number | null;
    initialStartDate?: string | null;
    /** Injected clock used to compute the initial deadline in create mode when no initialStartDate is supplied. */
    getNow?: () => Date;
    onsubmit: (input: CreateTaskInput | UpdateTaskInput) => void;
    onclose: () => void;
    onmakerecurring?: (seed: SharedFieldValues) => void;
    /** Called after a chunk verb mutates the schedule — parents refetch their visible range. */
    onchunkschange?: () => void;
    apiClient?: TaskFormApi;
  }

  const {
    open,
    task = null,
    initialDurationMinutes = null,
    initialStartDate = null,
    getNow = appClock,
    onsubmit,
    onclose,
    onmakerecurring,
    onchunkschange,
    apiClient = defaultTaskFormApi,
  }: Props = $props();

  const isEdit = $derived(task != null);
  // Recurring instances are anchored to their template's cadence, so their start
  // date is fixed (the backend rejects changes). Lock the field to match.
  const isRecurringInstance = $derived(task?.recurring_template_id != null);
  const modalTitle = $derived(isEdit ? 'Edit Task' : 'Create Task');
  const submitLabel = $derived(isEdit ? 'Save' : 'Create');
  const uid = $props.id();
  const formId = `task-form-${uid}`;

  function endOfLocalDay(referenceIso: string): string {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const d = new Date(referenceIso);
    d.setHours(23, 59, 0, 0);
    return d.toISOString();
  }

  let deadline = $state<string | null>(null);
  let startDate = $state<string | null>(null);
  let minChunkMinutes = $state(15);
  let noSplit = $state(false);
  let sharedValues = $state<SharedFieldValues>({
    title: '',
    description: '',
    durationMinutes: 60,
    priority: 'Medium',
    scheduleId: '',
    labels: [],
  });

  let deadlineError = $state('');

  let sharedController = $state<SharedFormController | null>(null);

  const sharedInitial = $derived.by((): SharedFieldValues => {
    if (task) {
      return {
        title: task.title,
        description: task.description ?? '',
        durationMinutes: task.duration_minutes,
        priority: task.priority,
        scheduleId: task.schedule_id,
        labels: [...task.labels],
      };
    }
    return {
      title: '',
      description: '',
      durationMinutes: initialDurationMinutes ?? 60,
      priority: 'Medium',
      scheduleId: untrack(() => (scheduleState.items.length > 0 ? scheduleState.items[0].id : '')),
      labels: [],
    };
  });

  $effect(() => {
    if (open) {
      if (task) {
        deadline = task.deadline;
        startDate = task.start_date;
        minChunkMinutes = task.min_chunk_minutes;
        noSplit = task.no_split;
      } else {
        if (initialStartDate) {
          deadline = endOfLocalDay(initialStartDate);
        } else if (getNow) {
          deadline = endOfLocalDay(getNow().toISOString());
        } else {
          deadline = null;
        }
        startDate = initialStartDate;
        minChunkMinutes = 15;
        noSplit = false;
      }
      sharedValues = sharedInitial;
      deadlineError = '';
    }
  });

  function validate(): boolean {
    deadlineError = '';

    const sharedValid = sharedController?.validate() ?? false;

    let valid = sharedValid;

    if (!isEdit && !deadline) {
      deadlineError = 'Deadline is required';
      valid = false;
    }

    return valid;
  }

  function normalizeSharedFields() {
    return {
      title: sharedValues.title.trim(),
      description: sharedValues.description.trim() || null,
      duration_minutes: sharedValues.durationMinutes,
      priority: sharedValues.priority,
      start_date: startDate,
      min_chunk_minutes: minChunkMinutes,
      no_split: noSplit,
      labels: sharedValues.labels,
    };
  }

  function buildUpdateInput(): UpdateTaskInput {
    return {
      ...normalizeSharedFields(),
      deadline: deadline ?? undefined,
      schedule_id: sharedValues.scheduleId || undefined,
    };
  }

  // The stored task run through the exact normalizations buildUpdateInput applies,
  // so pristine-vs-dirty compares like with like (trimming, ''→null, ''→undefined).
  const baselineInput = $derived.by((): UpdateTaskInput | null => {
    if (!task) return null;
    return {
      title: task.title.trim(),
      description: (task.description ?? '').trim() || null,
      duration_minutes: task.duration_minutes,
      priority: task.priority,
      start_date: task.start_date,
      min_chunk_minutes: task.min_chunk_minutes,
      no_split: task.no_split,
      labels: [...task.labels],
      deadline: task.deadline ?? undefined,
      schedule_id: task.schedule_id || undefined,
    };
  });

  // Both sides are object literals with identical key order, and JSON.stringify
  // drops undefined-valued keys on both symmetrically. Create mode (baselineInput
  // null) is always dirty so the footer buttons are always visible.
  const isDirty = $derived.by(() => {
    if (!baselineInput) return true;
    return JSON.stringify(buildUpdateInput()) !== JSON.stringify(baselineInput);
  });

  function trySubmit(): boolean {
    // Opened just to read or comment: close without a task update (and without
    // the reschedule it would trigger). Checked before validate on purpose —
    // pristine fields mirror an already-persisted task.
    if (isEdit && !isDirty) {
      onclose();
      return true;
    }

    if (!validate()) return false;

    if (isEdit) {
      onsubmit(buildUpdateInput());
    } else {
      const input: CreateTaskInput = {
        ...normalizeSharedFields(),
        deadline: deadline!,
        schedule_id: sharedValues.scheduleId || null,
      };
      onsubmit(input);
    }
    return true;
  }

  function handleSubmit(event: Event) {
    event.preventDefault();
    trySubmit();
  }

  function handleBackdropClick() {
    trySubmit();
  }

  // Hand the live shared-field values to the parent so it can open the recurring
  // template editor seeded with them. Task-only fields (deadline, min chunk,
  // no-split) are intentionally dropped — templates don't have them.
  function handleMakeRecurring() {
    const values = sharedController?.getValues();
    if (values) onmakerecurring?.(values);
  }

  let chunks = $state<Chunk[]>([]);
  let chunksLoading = $state(false);

  function loadChunks(taskId: string) {
    loadChunkList(
      taskId,
      (result) => (chunks = result),
      (loading) => (chunksLoading = loading),
      (id) => apiClient.listChunksForTask(id),
    );
  }

  $effect(() => {
    if (open && task) {
      loadChunks(task.id);
    } else {
      chunks = [];
    }
  });

  const chunkActions = $derived.by(
    () =>
      new TaskActions(
        {
          refresh: () => {
            if (task) loadChunks(task.id);
            onchunkschange?.();
          },
          // Unlock/delete-fixed are unconfirmed verbs; this host never shows a dialog.
          confirm: () => Promise.resolve(true),
          openTaskEditor: () => {},
          openTemplateEditor: () => {},
        },
        {
          unlockChunk: (id) => apiClient.unlockChunk(id),
          deleteFixedChunk: (id) => apiClient.deleteFixedChunk(id),
        },
      ),
  );

  // Save-then-jump mirrors the backdrop-click convention (clicking away saves);
  // a validation failure keeps the form open and skips the jump.
  function showInCalendar(chunk: Chunk) {
    if (!trySubmit()) return;
    calendarFocusState.request(chunk.id, chunk.start_time);
    router.navigate('calendar');
  }
</script>

{#snippet taskFormFooter()}
  {#if !isEdit && onmakerecurring}
    <button type="button" class="make-recurring-btn" onclick={handleMakeRecurring}>
      Make recurring
    </button>
  {/if}
  <button type="button" onclick={onclose}>{isEdit ? 'Cancel edits' : 'Cancel'}</button>
  <button type="submit" class="btn-primary" form={formId}>{submitLabel}</button>
{/snippet}

<Modal
  {open}
  title={modalTitle}
  size="lg"
  movable={true}
  resizable={true}
  onbackdropclick={handleBackdropClick}
  {onclose}
  footer={isDirty ? taskFormFooter : undefined}
>
  <form id={formId} class="task-form" onsubmit={handleSubmit} novalidate>
    <SharedFormFields
      {open}
      initial={sharedInitial}
      idPrefix="task"
      scheduleNullable={true}
      onready={(ctrl) => {
        sharedController = ctrl;
      }}
      onvalueschange={(values) => {
        sharedValues = values;
      }}
    >
      {#snippet extraFields()}
        <div class="form-row">
          <div class="form-field">
            <DateTimePicker
              label="Deadline {isEdit ? '' : '*'}"
              value={deadline}
              defaultTime="23:59"
              now={getNow()}
              onchange={(v) => {
                deadline = v;
                if (v) deadlineError = '';
              }}
            />
            {#if deadlineError}
              <span id="task-deadline-error" class="error-text" role="alert">{deadlineError}</span>
            {/if}
          </div>

          <div class="form-field">
            <DateTimePicker
              label="Start date"
              value={startDate}
              nullable={true}
              disabled={isRecurringInstance}
              popoverAlign="end"
              now={getNow()}
              onchange={(v) => {
                startDate = v;
              }}
            />
            {#if isRecurringInstance}
              <span class="field-hint">Set by the recurring schedule.</span>
            {/if}
          </div>
        </div>

        <div class="form-row">
          <div class="form-field">
            <label class="field-label" for="task-min-chunk">Min chunk (minutes)</label>
            <input
              id="task-min-chunk"
              class="field-input"
              type="number"
              min="5"
              max={sharedValues.durationMinutes}
              bind:value={minChunkMinutes}
            />
          </div>

          <div class="form-field form-field--checkbox">
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={noSplit} />
              <span>No split</span>
            </label>
          </div>
        </div>
      {/snippet}
    </SharedFormFields>
  </form>

  {#if isEdit}
    <section class="chunk-section">
      <ChunkList {chunks} loading={chunksLoading}>
        {#snippet trailing(chunk)}
          <StatusBadge status={chunk.status} />
          {#if chunk.is_fixed}
            <span class="fixed-badge">Fixed</span>
          {/if}
          <span class="chunk-actions">
            <button type="button" class="chunk-btn" onclick={() => showInCalendar(chunk)}>
              Show in calendar
            </button>
            {#if chunk.is_fixed && chunk.status !== 'completed'}
              <button
                type="button"
                class="chunk-btn"
                onclick={() => chunkActions.unlockChunk(chunk.id)}
              >
                Unlock
              </button>
              <button
                type="button"
                class="chunk-btn chunk-btn--danger"
                onclick={() => chunkActions.deleteFixedChunk(chunk.id)}
              >
                Delete
              </button>
            {/if}
          </span>
        {/snippet}
      </ChunkList>
    </section>
  {/if}

  <!-- Sibling of the form element on purpose: CommentSection has its own
       <form>, and nested forms are invalid HTML. -->
  {#if isEdit && task}
    <section class="comments-block" aria-label="Task comments">
      <CommentSection
        {task}
        listComments={(taskId) => apiClient.listComments(taskId)}
        createComment={(input) => apiClient.createComment(input)}
        updateComment={(id, content) => apiClient.updateComment(id, content)}
        deleteComment={(id) => apiClient.deleteComment(id)}
      />
    </section>
  {/if}
</Modal>

<style>
  .task-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .make-recurring-btn {
    margin-right: auto;
  }

  /* These classes are needed for snippet content rendered from this component.
     Svelte's component-scoped styles do NOT apply to snippet content defined
     in this component when rendered inside SharedFormFields. */
  .form-field--checkbox {
    justify-content: flex-end;
    padding-bottom: var(--spacing-1);
  }

  .error-text {
    font-size: var(--font-size-xs);
    color: var(--color-error);
  }

  .field-hint {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
  }

  .checkbox-label {
    display: flex;
    user-select: none;
  }

  .checkbox-label input[type='checkbox'] {
    width: 16px;
    height: 16px;
    padding: 0;
    cursor: pointer;
  }

  .comments-block {
    margin-top: var(--spacing-4);
    padding-top: var(--spacing-3);
    border-top: 1px solid var(--color-border-light);
  }

  .chunk-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
    margin-top: var(--spacing-4);
    padding-top: var(--spacing-3);
    border-top: 1px solid var(--color-border-light);
  }

  /* Wide modal context (unlike TaskDetail's narrow sidebar): keep chunk times
     on one line. ChunkList renders in its own scope, so this must go through
     :global() to reach the .chunk-time it emits. */
  .chunk-section :global(.chunk-time) {
    white-space: nowrap;
  }

  .fixed-badge {
    padding: 0 var(--spacing-2);
    border-radius: var(--radius-sm);
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    font-weight: var(--font-weight-medium);
  }

  .chunk-actions {
    margin-left: auto;
    display: flex;
    gap: var(--spacing-1);
  }

  .chunk-btn {
    padding: var(--spacing-1) var(--spacing-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .chunk-btn:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .chunk-btn--danger {
    color: var(--color-error);
  }

  .chunk-btn--danger:hover {
    color: var(--color-error);
  }
</style>
