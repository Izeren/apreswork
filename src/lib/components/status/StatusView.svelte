<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { Task, UpdateTaskInput, CreateTaskInput } from '../../types';
  import { warningState } from '../../stores/warnings.svelte';
  import { toastState } from '../../stores/toast.svelte';
  import { taskState } from '../../stores/tasks.svelte';
  import { router } from '../../router.svelte';
  import { formatDateTime } from '../../utils';
  import { TaskActions } from '../../actions/taskActions';
  import { createConfirmHost } from '../../actions/confirmHost.svelte';
  import TaskForm from '../tasks/TaskForm.svelte';
  import ConfirmHostDialog from '../shared/ConfirmHostDialog.svelte';
  import ResolutionDropdown from './ResolutionDropdown.svelte';
  import type { ScheduleWarning, WarningKind } from '../../types';
  import { defaultStatusViewApi, type StatusViewApi } from './statusViewShared';
  import { defaultTaskFormApi, type TaskFormApi } from '../tasks/taskFormShared';

  interface Props {
    /** True when hosted inside the shell's status modal: the modal header
        already carries the title and the modal body its own padding. */
    embedded?: boolean;
    apiClient?: StatusViewApi;
    taskFormApiClient?: TaskFormApi;
  }

  let {
    embedded = false,
    apiClient = defaultStatusViewApi,
    taskFormApiClient = defaultTaskFormApi,
  }: Props = $props();

  let refreshing = $state(false);

  function warningKindLabel(kind: WarningKind): string {
    return 'DeadlineViolation' in kind ? 'Deadline violation' : 'Unschedulable';
  }

  function warningMessage(warning: ScheduleWarning): string {
    if ('DeadlineViolation' in warning.kind) {
      const { deadline, earliest_completion } = warning.kind.DeadlineViolation;
      return `Deadline ${formatDateTime(deadline)} is earlier than the earliest completion ${formatDateTime(earliest_completion)}.`;
    }

    return warning.kind.Unschedulable.reason;
  }

  /**
   * Warnings are not persisted — they only exist in reschedule results, so a
   * fresh reschedule is the one way to re-derive them. Runs on mount and after
   * every resolution action so resolved rows disappear immediately.
   */
  function refreshWarnings(): void {
    refreshing = true;
    apiClient
      .triggerReschedule()
      .then((result) => {
        warningState.set(result.warnings);
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to refresh schedule warnings'));
      })
      .finally(() => {
        refreshing = false;
      });
  }

  // --- Task edit modal (same interaction model as calendar chunk clicks) ---
  let formOpen = $state(false);
  let editingTask = $state<Task | null>(null);

  function openTaskEditor(taskId: string): void {
    apiClient
      .getTask(taskId)
      .then((task) => {
        editingTask = task;
        formOpen = true;
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to load task'));
      });
  }

  function closeForm(): void {
    formOpen = false;
    editingTask = null;
  }

  function handleTaskSubmit(input: CreateTaskInput | UpdateTaskInput): void {
    if (!editingTask) return;
    apiClient
      .updateTask(editingTask.id, input as UpdateTaskInput)
      .then(() => {
        closeForm();
        toastState.success('Task updated');
        refreshWarnings();
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to update task'));
      });
  }

  const confirmHost = createConfirmHost();

  const actions = $derived(
    new TaskActions(
      {
        refresh: refreshWarnings,
        confirm: confirmHost.request,
        openTaskEditor,
        openTemplateEditor: (templateId) => {
          taskState.requestTemplateEdit(templateId);
          router.navigate('tasks');
        },
      },
      apiClient,
    ),
  );

  // Mount-only: refreshWarnings has no tracked reactive reads — keep it that way.
  $effect(() => {
    refreshWarnings();
  });
</script>

<section class="status-view" class:status-view--embedded={embedded} aria-label="Scheduling status">
  <div class="status-header">
    <div>
      {#if !embedded}
        <p class="status-kicker">Scheduling status</p>
      {/if}
      <h2 class="status-title" aria-busy={refreshing}>
        {#if warningState.count === 0}
          All tasks fit the schedule
        {:else}
          {warningState.count}
          {warningState.count === 1 ? 'task needs attention' : 'tasks need attention'}
        {/if}
      </h2>
    </div>
    <p class="status-summary">
      Click a task title to edit it, or resolve a warning directly from its row.
    </p>
  </div>

  {#if warningState.count === 0}
    <p class="status-empty">
      No scheduling warnings. Deadlines are met and every task has a valid placement.
    </p>
  {:else}
    <ul class="warning-list" role="list">
      {#each warningState.items as warning (warning.task_id)}
        <li class="warning-item">
          <div class="warning-item-body">
            <div class="warning-item-topline">
              <button
                class="warning-task-title"
                type="button"
                onclick={() => openTaskEditor(warning.task_id)}
              >
                {warning.task_title}
              </button>
              <span
                class="warning-kind"
                class:warning-kind--blocking={'Unschedulable' in warning.kind}
              >
                {warningKindLabel(warning.kind)}
              </span>
            </div>
            <p class="warning-message">{warningMessage(warning)}</p>
          </div>

          <ResolutionDropdown {warning} {actions} />
        </li>
      {/each}
    </ul>
  {/if}
</section>

<TaskForm
  open={formOpen}
  task={editingTask}
  onsubmit={handleTaskSubmit}
  onclose={closeForm}
  onchunkschange={refreshWarnings}
  apiClient={taskFormApiClient}
/>

<ConfirmHostDialog host={confirmHost} />

<style>
  .status-view {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--spacing-5);
  }

  /* The modal body pads and scrolls on its own. */
  .status-view--embedded {
    padding: 0;
  }

  .status-header {
    display: flex;
    justify-content: space-between;
    gap: var(--spacing-3);
    align-items: flex-start;
    margin-bottom: var(--spacing-4);
  }

  .status-kicker {
    margin: 0 0 var(--spacing-1);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--color-warning);
  }

  .status-title {
    margin: 0;
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-bold);
    color: var(--color-text);
  }

  .status-summary {
    margin: 0;
    max-width: 28rem;
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .status-empty {
    margin: 0;
    padding: var(--spacing-8) var(--spacing-4);
    text-align: center;
    color: var(--color-text-secondary);
    font-size: var(--font-size-base);
  }

  .warning-list {
    list-style: none;
    display: grid;
    gap: var(--spacing-3);
    margin: 0;
    padding: 0;
  }

  .warning-item {
    border: 1px solid var(--color-border-light);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
  }

  /* Fixed-basis text column (long titles wrap) so every row's Resolve button
     sits at the same x, right next to the task definition — not at the far
     right edge where it is easy to click the wrong row's button. */
  .warning-item-body {
    min-width: 0;
    flex: 0 1 36rem;
  }

  .warning-item-topline {
    display: flex;
    flex-wrap: wrap;
    gap: var(--spacing-2);
    align-items: center;
    margin-bottom: var(--spacing-1);
  }

  .warning-task-title {
    margin: 0;
    padding: 0;
    border: none;
    background: transparent;
    font-size: var(--font-size-base);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    text-align: left;
    overflow-wrap: anywhere;
    cursor: pointer;
  }

  .warning-task-title:hover {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .warning-task-title:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .warning-kind {
    display: inline-flex;
    align-items: center;
    padding: 0 var(--spacing-2);
    border-radius: var(--radius-full);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning, #d97706) 14%, transparent);
  }

  .warning-kind--blocking {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger, #dc2626) 14%, transparent);
  }

  .warning-message {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    line-height: 1.5;
  }
</style>
