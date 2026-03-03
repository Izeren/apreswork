<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import type { Task, Chunk, Priority, UpdateTaskInput } from '../../types';
  import { apiErrorMessage } from '../../api';
  import { taskState, TaskState } from '../../stores/tasks.svelte';
  import { scheduleState } from '../../stores/schedules.svelte';
  import { toastState } from '../../stores/toast.svelte';
  import PriorityBadge from '../shared/PriorityBadge.svelte';
  import StatusBadge from '../shared/StatusBadge.svelte';
  import LabelChip from '../shared/LabelChip.svelte';
  import ConfirmDialog from '../shared/ConfirmDialog.svelte';
  import DateTimePicker from '../shared/DateTimePicker.svelte';
  import MarkdownView from '../shared/MarkdownView.svelte';
  import CommentSection from './CommentSection.svelte';
  import ChunkList from './ChunkList.svelte';
  import { loadChunkList } from './chunkListLoad';
  import { type TaskDetailApi, defaultTaskDetailApi } from './taskDetailShared';
  import { formatDuration, formatDateTime, formatShortDate } from '../../utils';
  import { appClock } from '../../app-clock';

  interface Props {
    task: Task;
    onclose: () => void;
    onedit: (task: Task) => void;
    /** Called when the user wants to edit the parent recurring template. */
    onedittemplate?: (templateId: string) => void;
    /** Injectable API client; defaults to the real api. Inject a fake in tests. */
    apiClient?: TaskDetailApi;
    /** Injectable task store; defaults to the singleton. Inject a fake in tests. */
    taskStore?: TaskState;
    /** Injected clock for computing relative date labels. */
    getNow?: () => Date;
  }

  const {
    task,
    onclose,
    onedit,
    onedittemplate,
    apiClient = defaultTaskDetailApi,
    taskStore = taskState,
    getNow = appClock,
  }: Props = $props();

  let chunks = $state<Chunk[]>([]);
  let chunksLoading = $state(false);

  let completeDialogOpen = $state(false);
  let cancelDialogOpen = $state(false);
  let deleteDialogOpen = $state(false);

  const PRIORITY_OPTIONS: Priority[] = ['Low', 'Medium', 'High', 'Critical'];
  let editingPriority = $state(false);
  let editingDeadline = $state(false);
  let editingDescription = $state(false);
  let descriptionDraft = $state('');

  const progressPercent = $derived(
    task.duration_minutes > 0
      ? Math.min(100, Math.round((task.time_logged_minutes / task.duration_minutes) * 100))
      : 0,
  );

  const isRecurring = $derived(task.recurring_template_id !== null);
  const hasScheduledChunks = $derived(chunks.some((c) => c.status === 'scheduled'));

  /** Falls back to raw schedule_id when schedules haven't loaded yet. */
  const scheduleName = $derived(
    scheduleState.items.find((s) => s.id === task.schedule_id)?.name ?? task.schedule_id,
  );

  function loadChunks(taskId: string) {
    loadChunkList(
      taskId,
      (result) => {
        chunks = result;
      },
      (loading) => {
        chunksLoading = loading;
      },
      (id) => apiClient.listChunksForTask(id),
    );
  }

  $effect(() => {
    const taskId = task.id;
    untrack(() => {
      chunks = [];
      editingPriority = false;
      editingDeadline = false;
      editingDescription = false;
      descriptionDraft = '';
      loadChunks(taskId);
    });
  });

  async function mutateTask(
    apiCall: () => Promise<Task>,
    successMsg: string,
    failMsg: string,
    onSuccess?: () => void,
  ): Promise<void> {
    try {
      const updated = await apiCall();
      taskStore.items = taskStore.items.map((t) => (t.id === task.id ? updated : t));
      onSuccess?.();
      toastState.success(successMsg);
    } catch (e) {
      toastState.error(apiErrorMessage(e, failMsg));
    }
  }

  async function handleComplete() {
    completeDialogOpen = false;
    await mutateTask(
      () => apiClient.completeTask(task.id),
      'Task completed',
      'Failed to complete task',
      () => {
        chunks = chunks.map((c) =>
          c.status === 'scheduled' ? { ...c, status: 'completed' as const } : c,
        );
      },
    );
  }

  async function handleCancel() {
    cancelDialogOpen = false;
    await mutateTask(
      () => apiClient.cancelTask(task.id),
      'Task cancelled',
      'Failed to cancel task',
      onclose,
    );
  }

  async function handleDelete() {
    deleteDialogOpen = false;
    // taskStore.remove() provides optimistic update + rollback + deselection.
    await taskStore.remove(task.id);
    onclose();
  }

  function chunkDurationMinutes(chunk: Chunk): number {
    const start = new Date(chunk.start_time).getTime();
    const end = new Date(chunk.end_time).getTime();
    return Math.round((end - start) / 60_000);
  }

  /** Every update triggers a backend reschedule; refetch chunks after. */
  async function quickUpdate(input: UpdateTaskInput) {
    try {
      await taskStore.update(task.id, input);
      loadChunks(task.id);
    } catch (e) {
      toastState.error(apiErrorMessage(e, 'Failed to update task'));
    }
  }

  function handlePriorityChange(e: Event) {
    const value = (e.currentTarget as HTMLSelectElement).value as Priority;
    editingPriority = false;
    void quickUpdate({ priority: value });
  }

  function handleDeadlineChange(iso: string | null) {
    if (!iso) return;
    editingDeadline = false;
    void quickUpdate({ deadline: iso });
  }

  function startEditDescription() {
    descriptionDraft = task.description ?? '';
    editingDescription = true;
  }

  function saveDescription() {
    if (descriptionDraft === (task.description ?? '')) {
      editingDescription = false;
      return;
    }
    editingDescription = false;
    void quickUpdate({ description: descriptionDraft });
  }

  function cancelDescription() {
    editingDescription = false;
  }

  function handleClose() {
    if (editingDescription) {
      saveDescription();
    }
    onclose();
  }
</script>

<aside class="task-detail" aria-label="Task detail">
  <div class="detail-header">
    <h2 class="detail-title">{task.title}</h2>
    <button class="icon-btn close-btn" aria-label="Close detail panel" onclick={handleClose}
      >✕</button
    >
  </div>

  <div class="badges-row">
    {#if editingPriority}
      <select
        class="priority-select"
        aria-label="Priority"
        value={task.priority}
        onchange={handlePriorityChange}
      >
        {#each PRIORITY_OPTIONS as option (option)}
          <option value={option}>{option}</option>
        {/each}
      </select>
    {:else}
      <PriorityBadge priority={task.priority} />
    {/if}
    <button
      class="icon-btn quick-edit-btn"
      aria-label="Edit priority"
      title="Edit priority"
      onclick={() => (editingPriority = !editingPriority)}>✎</button
    >
    <StatusBadge status={task.status} />
    {#if isRecurring}
      <span class="recurring-badge">Recurring</span>
    {/if}
  </div>

  {#if editingDescription}
    <div class="detail-description">
      <textarea
        class="description-textarea"
        bind:value={descriptionDraft}
        rows={5}
        aria-label="Description"
      ></textarea>
      <div class="description-actions">
        <button type="button" onclick={cancelDescription}>Cancel</button>
        <button type="button" class="btn-primary" onclick={saveDescription}>Save</button>
      </div>
    </div>
  {:else}
    <div class="description-row">
      {#if task.description}
        <div class="detail-description"><MarkdownView source={task.description} /></div>
      {:else}
        <button
          type="button"
          class="detail-description detail-description--empty description-empty-btn"
          onclick={startEditDescription}>No description</button
        >
      {/if}
      <button
        type="button"
        class="icon-btn quick-edit-btn"
        aria-label="Edit description"
        onclick={startEditDescription}>✎</button
      >
    </div>
  {/if}

  <dl class="metadata-grid">
    <div class="meta-item">
      <dt class="meta-label">Duration</dt>
      <dd class="meta-value">{formatDuration(task.duration_minutes)}</dd>
    </div>
    <div class="meta-item">
      <dt class="meta-label">Logged</dt>
      <dd class="meta-value">{formatDuration(task.time_logged_minutes)} ({progressPercent}%)</dd>
    </div>
    <div class="meta-item meta-item--deadline">
      <dt class="meta-label">Deadline</dt>
      <dd class="meta-value">
        {#if editingDeadline}
          <DateTimePicker
            value={task.deadline}
            defaultTime="23:59"
            now={getNow()}
            onchange={handleDeadlineChange}
          />
        {:else}
          {formatDateTime(task.deadline)}
        {/if}
        <button
          class="icon-btn quick-edit-btn"
          aria-label="Edit deadline"
          title="Edit deadline"
          onclick={() => (editingDeadline = !editingDeadline)}>✎</button
        >
      </dd>
    </div>
    <div class="meta-item">
      <dt class="meta-label">Start date</dt>
      <dd class="meta-value">{formatShortDate(task.start_date)}</dd>
    </div>
    <div class="meta-item">
      <dt class="meta-label">Min chunk</dt>
      <dd class="meta-value">{formatDuration(task.min_chunk_minutes)}</dd>
    </div>
    <div class="meta-item">
      <dt class="meta-label">No split</dt>
      <dd class="meta-value">{task.no_split ? 'Yes' : 'No'}</dd>
    </div>
    <div class="meta-item">
      <dt class="meta-label">Schedule</dt>
      <dd class="meta-value">{scheduleName}</dd>
    </div>
  </dl>

  {#if task.labels.length > 0}
    <div class="labels-section">
      <span class="section-label">Labels</span>
      <div class="labels-chips">
        {#each task.labels as label (label)}
          <LabelChip {label} />
        {/each}
      </div>
    </div>
  {/if}

  <div class="chunks-section">
    <ChunkList {chunks} loading={chunksLoading}>
      {#snippet trailing(chunk)}
        <span class="chunk-duration">{formatDuration(chunkDurationMinutes(chunk))}</span>
        <StatusBadge status={chunk.status} />
      {/snippet}
    </ChunkList>
  </div>

  <CommentSection
    {task}
    listComments={apiClient.listComments}
    createComment={apiClient.createComment}
    updateComment={apiClient.updateComment}
    deleteComment={apiClient.deleteComment}
  />

  <div class="actions-row">
    <button class="btn-primary" onclick={() => onedit(task)}>Edit</button>

    {#if isRecurring}
      <button class="btn-template" onclick={() => onedittemplate?.(task.recurring_template_id!)}
        >Edit Template</button
      >
    {/if}

    {#if task.status !== 'cancelled' && task.status !== 'completed'}
      <button
        class="btn-complete"
        onclick={() => (completeDialogOpen = true)}
        disabled={!hasScheduledChunks}
        title={hasScheduledChunks ? '' : 'No scheduled chunks to complete'}>Complete</button
      >
      <button class="btn-muted" onclick={() => (cancelDialogOpen = true)}>Cancel task</button>
    {/if}

    <button class="btn-delete" onclick={() => (deleteDialogOpen = true)}>Delete</button>
  </div>
</aside>

<ConfirmDialog
  open={completeDialogOpen}
  title="Complete task"
  message="Mark '{task.title}' as completed?"
  confirmLabel="Complete"
  cancelLabel="Not yet"
  onconfirm={handleComplete}
  oncancel={() => (completeDialogOpen = false)}
/>

<ConfirmDialog
  open={cancelDialogOpen}
  title="Cancel task"
  message="Are you sure you want to cancel '{task.title}'? This will remove all scheduled chunks."
  confirmLabel="Cancel task"
  cancelLabel="Keep task"
  destructive={true}
  onconfirm={handleCancel}
  oncancel={() => (cancelDialogOpen = false)}
/>

<ConfirmDialog
  open={deleteDialogOpen}
  title="Delete task"
  message="Are you sure you want to permanently delete '{task.title}'? This action cannot be undone."
  confirmLabel="Delete"
  cancelLabel="Keep"
  destructive={true}
  onconfirm={handleDelete}
  oncancel={() => (deleteDialogOpen = false)}
/>

<style>
  .task-detail {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
    width: 340px;
    min-width: 280px;
    height: 100%;
    overflow-y: auto;
    padding: var(--spacing-4);
    background: var(--color-bg);
    border-left: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .detail-header {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-2);
  }

  .detail-title {
    flex: 1;
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    margin: 0;
    line-height: 1.3;
    word-break: break-word;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    display: flex;
  }

  .badges-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--spacing-2);
    align-items: center;
  }

  .recurring-badge {
    background: var(--color-primary-light);
    color: var(--color-primary);
  }

  .description-row {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-1);
  }

  .detail-description {
    flex: 1 1 auto;
    font-size: var(--font-size-sm);
    color: var(--color-text);
    line-height: 1.5;
    margin: 0;
  }

  .detail-description--empty {
    color: var(--color-text-tertiary);
    font-style: italic;
  }

  .description-empty-btn {
    display: block;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    width: 100%;
  }

  .description-empty-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
    border-radius: 2px;
  }

  .description-textarea {
    width: 100%;
    min-height: 80px;
    resize: vertical;
    font-size: var(--font-size-sm);
    font-family: inherit;
    padding: var(--spacing-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: var(--color-text);
    box-sizing: border-box;
  }

  .description-textarea:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .description-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-2);
    margin-top: var(--spacing-2);
  }

  .metadata-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--spacing-2) var(--spacing-4);
    margin: 0;
  }

  .meta-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .meta-value {
    font-size: var(--font-size-sm);
    color: var(--color-text);
    margin: 0;
  }

  /* The picker popover needs the full panel width. */
  .meta-item--deadline {
    grid-column: 1 / -1;
  }

  .meta-item--deadline .meta-value {
    display: flex;
    align-items: center;
    gap: var(--spacing-1);
  }

  .quick-edit-btn {
    width: 22px;
    height: 22px;
    font-size: var(--font-size-xs);
  }

  .priority-select {
    padding: 2px var(--spacing-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--font-size-sm);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .priority-select:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .chunks-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .chunk-duration {
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .actions-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--spacing-2);
    padding-top: var(--spacing-2);
    border-top: 1px solid var(--color-border-light);
    margin-top: auto;
  }

  .btn-template {
    border-color: var(--color-primary);
    background: transparent;
    color: var(--color-primary);
  }

  .btn-template:hover {
    background: var(--color-primary-light);
  }

  .btn-complete {
    padding: var(--spacing-2) var(--spacing-4);
    border: 1px solid var(--color-success);
    border-radius: var(--radius-md);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    background: var(--color-success);
    color: var(--color-text-inverse);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      opacity var(--transition-fast);
  }

  .btn-complete:hover {
    opacity: 0.9;
  }

  .btn-complete:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-complete:focus-visible {
    outline: 2px solid var(--color-success);
    outline-offset: 2px;
  }

  .btn-delete {
    border-color: var(--color-error);
    background: transparent;
    color: var(--color-error);
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
    margin-left: auto;
  }

  .btn-delete:hover {
    background: var(--color-error);
    color: #ffffff;
  }

  .btn-delete:focus-visible {
    outline: 2px solid var(--color-error);
    outline-offset: 2px;
  }
</style>
