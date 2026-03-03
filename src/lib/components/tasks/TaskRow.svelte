<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { Task, TaskStatus } from '../../types';
  import PriorityBadge from '../shared/PriorityBadge.svelte';
  import StatusBadge from '../shared/StatusBadge.svelte';
  import LabelChip from '../shared/LabelChip.svelte';
  import { formatShortDate, formatDuration } from '../../utils';

  interface Props {
    task: Task;
    selected?: boolean;
    onselect: () => void;
    onedit?: () => void;
    ontoggleschedule?: ((task: Task, nextStatus: TaskStatus) => void) | null;
    scheduletogglebusy?: boolean;
    onmenu?: ((task: Task, x: number, y: number) => void) | null;
  }

  const {
    task,
    selected = false,
    onselect,
    onedit,
    ontoggleschedule = null,
    scheduletogglebusy = false,
    onmenu = null,
  }: Props = $props();

  const progressPercent = $derived(
    task.duration_minutes > 0
      ? Math.min(100, Math.round((task.time_logged_minutes / task.duration_minutes) * 100))
      : 0,
  );

  const loggedLabel = $derived(formatDuration(task.time_logged_minutes));
  const totalLabel = $derived(formatDuration(task.duration_minutes));
  const deadlineLabel = $derived(formatShortDate(task.deadline));
  const scheduleToggleVisible = $derived(
    task.status === 'backlog' || task.status === 'pending' || task.status === 'scheduled',
  );
  const scheduleToggleChecked = $derived(task.status === 'pending' || task.status === 'scheduled');
  const scheduleToggleLabel = $derived(
    scheduleToggleChecked ? 'Remove from scheduling' : 'Add to scheduling',
  );

  function handleScheduleToggle(event: MouseEvent | KeyboardEvent): void {
    event.stopPropagation();
    if (!ontoggleschedule || scheduletogglebusy || !scheduleToggleVisible) return;

    const nextStatus: TaskStatus = scheduleToggleChecked ? 'backlog' : 'pending';
    ontoggleschedule(task, nextStatus);
  }

  /** Open the verb menu anchored to an element (kebab click, keyboard path). */
  function openMenuAt(element: Element): void {
    if (!onmenu) return;
    const rect = element.getBoundingClientRect();
    onmenu(task, rect.left, rect.bottom);
  }

  function handleKebabClick(event: MouseEvent): void {
    event.stopPropagation();
    openMenuAt(event.currentTarget as Element);
  }

  function handleContextMenu(event: MouseEvent): void {
    if (!onmenu) return;
    event.preventDefault();
    onmenu(task, event.clientX, event.clientY);
  }

  function handleRowKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onselect();
      return;
    }
    if (onmenu && (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))) {
      event.preventDefault();
      openMenuAt(event.currentTarget as Element);
    }
  }
</script>

<div
  class="task-row"
  class:selected
  role="option"
  tabindex="0"
  aria-selected={selected}
  onclick={onselect}
  ondblclick={onedit}
  oncontextmenu={handleContextMenu}
  onkeydown={handleRowKeydown}
>
  {#if scheduleToggleVisible}
    <button
      class="schedule-toggle"
      class:schedule-toggle--checked={scheduleToggleChecked}
      type="button"
      role="switch"
      aria-checked={scheduleToggleChecked}
      aria-label={scheduleToggleLabel}
      disabled={scheduletogglebusy}
      onclick={handleScheduleToggle}
      ondblclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
    >
      <span class="schedule-toggle__thumb"></span>
    </button>
  {:else}
    <span class="schedule-toggle-placeholder" aria-hidden="true"></span>
  {/if}

  <div class="row-badges">
    <PriorityBadge priority={task.priority} />
    <StatusBadge status={task.status} />
  </div>

  <span class="row-title">{task.title}</span>

  {#if task.labels.length > 0}
    <div class="row-labels">
      {#each task.labels as label (label)}
        <LabelChip {label} />
      {/each}
    </div>
  {/if}

  <span class="row-deadline">{deadlineLabel}</span>

  <div class="row-progress" aria-label="Progress: {loggedLabel} of {totalLabel}">
    <div class="progress-bar">
      <div class="progress-fill" style="width: {progressPercent}%"></div>
    </div>
    <span class="progress-text">{loggedLabel} / {totalLabel}</span>
  </div>

  {#if onmenu}
    <button
      class="row-menu"
      type="button"
      aria-label="Task actions"
      aria-haspopup="menu"
      onclick={handleKebabClick}
      ondblclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
    >
      ⋯
    </button>
  {/if}
</div>

<style>
  .task-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-3);
    padding: var(--spacing-2) var(--spacing-4);
    min-height: 44px;
    background: var(--color-bg);
    border-bottom: 1px solid var(--color-border-light);
    border-left: 3px solid transparent;
    cursor: pointer;
    transition:
      background var(--transition-fast),
      border-color var(--transition-fast);
    user-select: none;
  }

  .task-row:hover {
    background: var(--color-surface-hover);
  }

  .task-row:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .task-row.selected {
    background: var(--color-primary-light);
    border-left-color: var(--color-primary);
  }

  .schedule-toggle,
  .schedule-toggle-placeholder {
    width: 36px;
    height: 20px;
    flex-shrink: 0;
  }

  .schedule-toggle {
    position: relative;
    border: none;
    border-radius: 999px;
    padding: 0;
    background: var(--color-border);
    transition:
      background var(--transition-fast),
      opacity var(--transition-fast);
  }

  .schedule-toggle:hover {
    background: var(--color-text-tertiary);
  }

  .schedule-toggle:disabled {
    opacity: 0.6;
    cursor: wait;
  }

  .schedule-toggle--checked {
    background: var(--color-primary);
  }

  .schedule-toggle--checked:hover {
    background: var(--color-primary-hover);
  }

  .schedule-toggle__thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--color-surface);
    box-shadow: var(--shadow-sm);
    transition: transform var(--transition-fast);
  }

  .schedule-toggle--checked .schedule-toggle__thumb {
    transform: translateX(16px);
  }

  .row-menu {
    width: 24px;
    height: 24px;
    font-size: var(--font-size-md);
    line-height: 1;
    opacity: 0;
    transition:
      opacity var(--transition-fast),
      background var(--transition-fast);
  }

  .task-row:hover .row-menu,
  .task-row:focus-within .row-menu,
  .row-menu:focus-visible {
    opacity: 1;
  }

  .row-menu:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text);
  }

  .row-badges {
    display: flex;
    gap: var(--spacing-1);
    flex-shrink: 0;
  }

  .row-title {
    flex: 1;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .row-labels {
    display: flex;
    gap: var(--spacing-1);
    flex-shrink: 0;
    flex-wrap: nowrap;
    overflow: hidden;
    max-width: 160px;
  }

  .row-deadline {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    white-space: nowrap;
    flex-shrink: 0;
    min-width: 52px;
    text-align: right;
  }

  .row-progress {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    flex-shrink: 0;
    width: 140px;
  }

  .progress-bar {
    flex: 1;
    height: 4px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-primary);
    border-radius: var(--radius-sm);
    transition: width var(--transition-fast);
  }

  .progress-text {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    white-space: nowrap;
    min-width: 72px;
    text-align: right;
  }
</style>
