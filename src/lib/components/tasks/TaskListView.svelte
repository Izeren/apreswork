<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import { taskState, TaskState } from '../../stores/tasks.svelte';
  import { templateState, TemplateState } from '../../stores/templates.svelte';
  import { registerShortcuts } from '../../shortcuts.svelte';
  import type {
    Task,
    TaskStatus,
    Priority,
    CreateTaskInput,
    UpdateTaskInput,
    LabelCount,
  } from '../../types';
  import { PRIORITIES, TASK_STATUSES } from '../../types';
  import type { SortField, SortKey } from './taskSort';
  import type { SharedFieldValues } from './SharedFormFields.svelte';
  import { DEFAULT_SORT_STACK, clickSortField, sortTasks } from './taskSort';
  import {
    DEFAULT_PRIORITY_FILTER,
    DEFAULT_STATUS_FILTER,
    loadPriorityFilter,
    loadSortStack,
    loadStatusFilter,
    savePriorityFilter,
    saveSortStack,
    saveStatusFilter,
  } from './taskListPrefs';
  import type { LabelChipState, LabelSelection } from './labelFilter';
  import {
    ARIA_PRESSED,
    CHIP_TITLE,
    EMPTY_LABEL_SELECTION,
    UNLABELED_FILTER,
    clickLabelChip,
    compareLabelChips,
    labelChipState,
    nextChipState,
  } from './labelFilter';
  import { TaskActions, taskContextMenuItems } from '../../actions/taskActions';
  import { createConfirmHost } from '../../actions/confirmHost.svelte';
  import { runReschedule } from '../../actions/rescheduleTrigger';
  import { type TaskListViewApi, defaultTaskListViewApi } from './taskListViewShared';
  import FilterDropdown from './FilterDropdown.svelte';
  import TaskRow from './TaskRow.svelte';
  import TaskForm from './TaskForm.svelte';
  import TaskDetail from './TaskDetail.svelte';
  import RecurringListView from './RecurringListView.svelte';
  import ContextMenu from '../shared/ContextMenu.svelte';
  import ConfirmHostDialog from '../shared/ConfirmHostDialog.svelte';
  import RescheduleButton from '../shared/RescheduleButton.svelte';

  interface Props {
    apiClient?: TaskListViewApi;
    getNow: () => Date;
    taskStore?: TaskState;
    templateStore?: TemplateState;
  }

  const {
    apiClient = defaultTaskListViewApi,
    getNow,
    taskStore = taskState,
    templateStore = templateState,
  }: Props = $props();

  type TaskListTab = 'tasks' | 'recurring';

  interface TemplateEditRequest {
    id: string;
    nonce: number;
  }

  interface TemplateCreateRequest {
    seed: SharedFieldValues;
    nonce: number;
  }

  const SORT_FIELDS: { field: SortField; label: string }[] = [
    { field: 'status', label: 'Status' },
    { field: 'priority', label: 'Priority' },
    { field: 'deadline', label: 'Deadline' },
    { field: 'title', label: 'Title' },
    { field: 'logged', label: 'Logged' },
  ];

  let searchText = $state('');
  let statusFilter = $state<TaskStatus[]>(loadStatusFilter(window.localStorage));
  let priorityFilter = $state<Priority[]>(loadPriorityFilter(window.localStorage));
  let labelSelection = $state<LabelSelection>(EMPTY_LABEL_SELECTION);
  let unlabeledState = $state<LabelChipState>('neutral');

  // Sort state — the whole key stack persists per machine; stack[0] is the
  // primary key (the only one the sort bar shows an indicator for).
  let sortStack = $state<SortKey[]>(loadSortStack(window.localStorage) ?? [...DEFAULT_SORT_STACK]);
  const primarySort = $derived(sortStack[0]);

  let formOpen = $state(false);
  let editingTask = $state<Task | null>(null);
  let activeTab = $state<TaskListTab>('tasks');
  let templateEditRequest = $state<TemplateEditRequest | null>(null);
  let templateEditNonce = 0;
  let templateCreateRequest = $state<TemplateCreateRequest | null>(null);
  let templateCreateNonce = 0;
  let lastHandledTemplateEditNonce = $state(0);

  function openCreate() {
    editingTask = null;
    formOpen = true;
  }

  function openDetailEdit(task: Task) {
    editingTask = task;
    formOpen = true;
  }

  function closeForm() {
    formOpen = false;
    editingTask = null;
  }

  function selectTab(tab: TaskListTab) {
    activeTab = tab;
  }

  function openTemplateEditor(templateId: string) {
    activeTab = 'recurring';
    templateEditNonce += 1;
    templateEditRequest = { id: templateId, nonce: templateEditNonce };
  }

  function clearTemplateEditRequest() {
    templateEditRequest = null;
  }

  function handleMakeRecurring(seed: SharedFieldValues) {
    closeForm();
    activeTab = 'recurring';
    templateCreateNonce += 1;
    templateCreateRequest = { seed, nonce: templateCreateNonce };
  }

  function clearTemplateCreateRequest() {
    templateCreateRequest = null;
  }

  async function handleFormSubmit(input: CreateTaskInput | UpdateTaskInput) {
    if (editingTask) {
      await taskStore.update(editingTask.id, input as UpdateTaskInput);
    } else {
      await taskStore.create(input as CreateTaskInput);
    }
    closeForm();
  }

  $effect(() => {
    if (
      taskStore.templateEditRequestId !== null &&
      taskStore.templateEditRequestNonce !== lastHandledTemplateEditNonce
    ) {
      const templateId = taskStore.templateEditRequestId;
      lastHandledTemplateEditNonce = taskStore.templateEditRequestNonce;
      openTemplateEditor(templateId);
      taskStore.clearTemplateEditRequest();
    }
  });

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  function onSearchInput() {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      applyFilters();
    }, 300);
  }

  function applyFilters() {
    taskStore.setFilter({
      search_text: searchText || null,
      statuses: statusFilter.length > 0 ? [...statusFilter] : null,
      priorities: priorityFilter.length > 0 ? [...priorityFilter] : null,
      labels: labelSelection.included.length > 0 ? [...labelSelection.included] : null,
      excluded_labels: labelSelection.excluded.length > 0 ? [...labelSelection.excluded] : null,
      unlabeled: UNLABELED_FILTER[unlabeledState],
    });
    void taskStore.load();
  }

  function makeFilterHandler<T>(set: (v: T[]) => void, save: (v: T[], s: Storage) => void) {
    return (next: T[]) => {
      set(next);
      save(next, window.localStorage);
      applyFilters();
    };
  }

  const handleStatusFilterChange = makeFilterHandler<TaskStatus>((v) => {
    statusFilter = v;
  }, saveStatusFilter);
  const handlePriorityFilterChange = makeFilterHandler<Priority>((v) => {
    priorityFilter = v;
  }, savePriorityFilter);

  // See labelFilter.ts for the tri-state transition table.
  function handleLabelChipClick(label: string, exclude: boolean) {
    labelSelection = clickLabelChip(labelSelection, label, exclude);
    applyFilters();
  }

  function handleUnlabeledChipClick(exclude: boolean) {
    unlabeledState = nextChipState(unlabeledState, exclude);
    applyFilters();
  }

  function clearFilters() {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    searchText = '';
    statusFilter = [...DEFAULT_STATUS_FILTER];
    priorityFilter = [...DEFAULT_PRIORITY_FILTER];
    labelSelection = EMPTY_LABEL_SELECTION;
    unlabeledState = 'neutral';
    saveStatusFilter(statusFilter, window.localStorage);
    savePriorityFilter(priorityFilter, window.localStorage);
    taskStore.setFilter({ statuses: [...DEFAULT_STATUS_FILTER] });
    void taskStore.load();
  }

  const statusFilterIsDefault = $derived(
    statusFilter.length === DEFAULT_STATUS_FILTER.length &&
      DEFAULT_STATUS_FILTER.every((s) => statusFilter.includes(s)),
  );

  const hasActiveFilter = $derived(
    searchText !== '' ||
      !statusFilterIsDefault ||
      priorityFilter.length > 0 ||
      labelSelection.included.length > 0 ||
      labelSelection.excluded.length > 0 ||
      unlabeledState !== 'neutral',
  );

  const LABEL_CHIP_LIMIT = 12;
  let labelsExpanded = $state(false);

  const isActiveLabel = (label: string) => labelChipState(labelSelection, label) !== 'neutral';

  const facetLabels = $derived.by((): LabelCount[] => {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- transient accumulator inside the derived computation, never escapes as state
    const counts = new Map<string, number>();
    for (const task of taskStore.items) {
      for (const label of task.labels) {
        counts.set(label, (counts.get(label) ?? 0) + 1);
      }
    }
    for (const label of [...labelSelection.included, ...labelSelection.excluded]) {
      if (!counts.has(label)) counts.set(label, 0);
    }
    return [...counts.entries()].map(([label, task_count]) => ({ label, task_count }));
  });

  const sortedLabels = $derived(
    [...facetLabels].sort((a, b) => compareLabelChips(labelSelection, a, b)),
  );

  const visibleLabels = $derived.by(() => {
    if (labelsExpanded || sortedLabels.length <= LABEL_CHIP_LIMIT) return sortedLabels;
    return [
      ...sortedLabels.slice(0, LABEL_CHIP_LIMIT),
      ...sortedLabels.slice(LABEL_CHIP_LIMIT).filter((l) => isActiveLabel(l.label)),
    ];
  });

  const hiddenLabelCount = $derived(sortedLabels.length - visibleLabels.length);

  // Facet count for the "unlabeled" pseudo-chip: visible tasks with no labels.
  const unlabeledCount = $derived(
    taskStore.items.filter((task) => task.labels.length === 0).length,
  );

  const sortedTasks = $derived(sortTasks(taskStore.items, sortStack));

  // Load on mount with the restored/default filters applied — untrack prevents
  // the state reads and load()'s mutations from re-triggering the effect
  $effect(() => {
    untrack(() => {
      applyFilters();
    });
  });

  $effect(() => {
    return () => {
      if (debounceTimer !== null) clearTimeout(debounceTimer);
    };
  });

  function toggleSort(field: SortField) {
    sortStack = clickSortField(sortStack, field);
    saveSortStack(sortStack, window.localStorage);
  }

  let rescheduling = $state(false);
  let scheduleToggleBusyIds = $state<string[]>([]);

  function reschedule(): void {
    runReschedule(
      (busy) => (rescheduling = busy),
      () => void taskStore.load(),
      apiClient,
    );
  }

  function sortLabel(field: SortField): string {
    if (primarySort.field !== field) return '';
    return primarySort.direction === 'asc' ? ' ▲' : ' ▼';
  }

  async function handleScheduleToggle(task: Task, nextStatus: TaskStatus): Promise<void> {
    scheduleToggleBusyIds = [...scheduleToggleBusyIds, task.id];
    try {
      await taskStore.update(task.id, { status: nextStatus });
    } finally {
      scheduleToggleBusyIds = scheduleToggleBusyIds.filter((id) => id !== task.id);
    }
  }

  let menuOpen = $state(false);
  let menuTask = $state<Task | null>(null);
  let menuNow = $state<Date | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);

  const confirmHost = createConfirmHost();

  const actions = $derived.by(
    () =>
      new TaskActions(
        {
          refresh: () => void taskStore.load(),
          confirm: confirmHost.request,
          openTaskEditor: (taskId) => {
            const task = taskStore.items.find((t) => t.id === taskId);
            if (task) openDetailEdit(task);
          },
          openTemplateEditor,
        },
        apiClient,
      ),
  );

  const menuItems = $derived(
    menuTask && menuNow ? taskContextMenuItems(menuTask, actions, menuNow) : [],
  );

  function openTaskMenu(task: Task, x: number, y: number): void {
    menuTask = task;
    menuNow = getNow();
    menuX = x;
    menuY = y;
    menuOpen = true;
  }

  function closeTaskMenu(): void {
    menuOpen = false;
  }

  $effect(() => {
    return registerShortcuts([
      { key: 'n', description: 'New task', group: 'Tasks', handler: openCreate },
    ]);
  });
</script>

<div class="task-list-view">
  <div class="task-list-pane">
    <div class="view-tabs" role="tablist" aria-label="Task sections">
      <button
        id="tab-tasks"
        class="view-tab"
        class:active={activeTab === 'tasks'}
        role="tab"
        aria-selected={activeTab === 'tasks'}
        aria-controls="panel-tasks"
        onclick={() => selectTab('tasks')}
      >
        Tasks
      </button>
      <button
        id="tab-recurring"
        class="view-tab"
        class:active={activeTab === 'recurring'}
        role="tab"
        aria-selected={activeTab === 'recurring'}
        aria-controls="panel-recurring"
        onclick={() => selectTab('recurring')}
      >
        Recurring
      </button>
    </div>

    {#if activeTab === 'tasks'}
      <div id="panel-tasks" role="tabpanel" aria-labelledby="tab-tasks">
        <div class="filter-bar">
          <input
            class="search-input"
            type="search"
            placeholder="Search tasks…"
            bind:value={searchText}
            oninput={onSearchInput}
            aria-label="Search tasks"
          />

          <FilterDropdown
            label="Status"
            options={TASK_STATUSES}
            selected={statusFilter}
            onchange={handleStatusFilterChange}
          />

          <FilterDropdown
            label="Priority"
            options={PRIORITIES}
            selected={priorityFilter}
            onchange={handlePriorityFilterChange}
          />

          {#if hasActiveFilter}
            <button class="clear-btn" onclick={clearFilters}>Clear filters</button>
          {/if}

          <RescheduleButton {rescheduling} onclick={reschedule} />
          <button class="create-btn" onclick={openCreate}>+ New Task</button>
        </div>

        {#if sortedLabels.length > 0 || unlabeledState !== 'neutral'}
          <div class="label-filter-bar" role="group" aria-label="Filter by label">
            <button
              class="label-filter-chip meta"
              class:selected={unlabeledState === 'included'}
              class:excluded={unlabeledState === 'excluded'}
              aria-pressed={ARIA_PRESSED[unlabeledState]}
              title={CHIP_TITLE[unlabeledState]}
              onclick={(e) => handleUnlabeledChipClick(e.ctrlKey)}
              oncontextmenu={(e) => {
                e.preventDefault();
                handleUnlabeledChipClick(true);
              }}
            >
              unlabeled
              <span class="label-filter-count">{unlabeledCount}</span>
            </button>
            {#each visibleLabels as { label, task_count } (label)}
              {@const chipState = labelChipState(labelSelection, label)}
              <button
                class="label-filter-chip"
                class:selected={chipState === 'included'}
                class:excluded={chipState === 'excluded'}
                aria-pressed={ARIA_PRESSED[chipState]}
                title={CHIP_TITLE[chipState]}
                onclick={(e) => handleLabelChipClick(label, e.ctrlKey)}
                oncontextmenu={(e) => {
                  e.preventDefault();
                  handleLabelChipClick(label, true);
                }}
              >
                {label}
                <span class="label-filter-count">{task_count}</span>
              </button>
            {/each}
            {#if sortedLabels.length > LABEL_CHIP_LIMIT && (labelsExpanded || hiddenLabelCount > 0)}
              <button
                class="label-expand-btn"
                aria-expanded={labelsExpanded}
                onclick={() => (labelsExpanded = !labelsExpanded)}
              >
                {labelsExpanded ? 'Show less' : `+${hiddenLabelCount} more`}
              </button>
            {/if}
          </div>
        {/if}

        <div class="sort-bar" role="toolbar" aria-label="Sort controls">
          {#each SORT_FIELDS as { field, label } (field)}
            <button
              class="sort-btn"
              class:active={primarySort.field === field}
              onclick={() => toggleSort(field)}
            >
              {label}{sortLabel(field)}
            </button>
          {/each}
        </div>

        <div class="task-list-content">
          {#if taskStore.loading}
            <div class="state-message" aria-live="polite" aria-busy="true">Loading tasks…</div>
          {:else if sortedTasks.length === 0}
            <div class="state-message empty-state" aria-live="polite">
              No tasks found. Try adjusting your filters.
            </div>
          {:else}
            <ul class="task-list" role="listbox" aria-label="Tasks">
              {#each sortedTasks as task (task.id)}
                <li class="task-list-item" role="presentation">
                  <TaskRow
                    {task}
                    selected={taskStore.selectedId === task.id}
                    onselect={() => taskStore.select(task.id)}
                    onedit={() => openDetailEdit(task)}
                    ontoggleschedule={handleScheduleToggle}
                    scheduletogglebusy={scheduleToggleBusyIds.includes(task.id)}
                    onmenu={openTaskMenu}
                  />
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </div>
    {:else}
      <div id="panel-recurring" role="tabpanel" aria-labelledby="tab-recurring">
        <div class="task-list-content">
          <RecurringListView
            editRequest={templateEditRequest}
            oneditrequesthandled={clearTemplateEditRequest}
            createRequest={templateCreateRequest}
            oncreaterequesthandled={clearTemplateCreateRequest}
            {templateStore}
            {taskStore}
            {getNow}
          />
        </div>
      </div>
    {/if}
  </div>

  {#if activeTab === 'tasks' && taskStore.selected}
    <TaskDetail
      task={taskStore.selected}
      onclose={() => taskStore.select(null)}
      onedit={openDetailEdit}
      onedittemplate={openTemplateEditor}
      {taskStore}
    />
  {/if}
</div>

<TaskForm
  open={formOpen}
  task={editingTask}
  {getNow}
  onsubmit={handleFormSubmit}
  onclose={closeForm}
  onmakerecurring={handleMakeRecurring}
  onchunkschange={() => void taskStore.load()}
/>

<ContextMenu open={menuOpen} x={menuX} y={menuY} items={menuItems} onclose={closeTaskMenu} />

<ConfirmHostDialog host={confirmHost} />

<style>
  .task-list-view {
    display: flex;
    flex-direction: row;
    height: 100%;
    background: var(--color-bg);
    overflow: hidden;
  }

  .task-list-pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .view-tabs {
    display: flex;
    gap: var(--spacing-1);
    padding: var(--spacing-3) var(--spacing-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
  }

  .view-tab {
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    padding: var(--spacing-2) var(--spacing-3);
    background: transparent;
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .view-tab:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .view-tab.active {
    background: var(--color-surface);
    color: var(--color-primary);
    border-color: var(--color-primary);
  }

  .view-tab:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .filter-bar {
    display: flex;
    align-items: center;
    gap: var(--spacing-3);
    padding: var(--spacing-4);
    border-bottom: 1px solid var(--color-border);
    flex-wrap: wrap;
  }

  .search-input {
    flex: 1;
    min-width: 160px;
    background: var(--color-bg);
    color: var(--color-text);
    transition: border-color var(--transition-fast);
  }

  .search-input:focus {
    outline: none;
    border-color: var(--color-primary);
  }

  .search-input:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .clear-btn {
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }

  .clear-btn:hover {
    background: var(--color-surface-active);
    color: var(--color-text);
  }

  .clear-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .filter-bar :global(.reschedule-btn) {
    margin-left: auto;
    padding: var(--spacing-2) var(--spacing-3);
  }

  .create-btn {
    padding: var(--spacing-2) var(--spacing-3);
    margin-left: 0;
    border: none;
    border-radius: var(--radius-md);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    background: var(--color-primary);
    color: #ffffff;
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }

  .create-btn:hover {
    background: var(--color-primary-hover);
  }

  .create-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .label-filter-bar {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    padding: var(--spacing-2) var(--spacing-4);
    border-bottom: 1px solid var(--color-border-light);
    flex-wrap: wrap;
  }

  .label-filter-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--spacing-1);
    padding: var(--spacing-1) var(--spacing-3);
    border: 1px solid var(--color-border);
    border-radius: 999px;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    background: var(--color-bg);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .label-filter-chip:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .label-filter-chip:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .label-filter-chip.selected {
    background: var(--color-primary-light);
    color: var(--color-primary);
    border-color: var(--color-primary);
  }

  /* Meta pseudo-chip ("unlabeled"): italic marks it as a presence filter,
     not a real label. */
  .label-filter-chip.meta {
    font-style: italic;
  }

  /* Excluded: faded + struck through so the active exclusion reads at a
     glance (theme-safe: derived from the same custom properties). */
  .label-filter-chip.excluded {
    background: var(--color-bg-secondary);
    color: var(--color-text-tertiary);
    border-style: dashed;
    text-decoration: line-through;
    opacity: 0.65;
  }

  .label-filter-count {
    color: var(--color-text-tertiary);
    font-weight: var(--font-weight-normal);
  }

  .label-filter-chip.selected .label-filter-count {
    color: var(--color-primary);
  }

  .label-expand-btn {
    padding: var(--spacing-1) var(--spacing-3);
    border: 1px dashed var(--color-border);
    border-radius: 999px;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
    transition:
      color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .label-expand-btn:hover {
    color: var(--color-text);
    border-color: var(--color-text-tertiary);
  }

  .label-expand-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .sort-bar {
    display: flex;
    gap: var(--spacing-1);
  }

  .sort-btn {
    padding: var(--spacing-1) var(--spacing-3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .sort-btn:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .sort-btn.active {
    background: var(--color-primary-light);
    color: var(--color-primary);
    border-color: var(--color-primary);
  }

  /* Tab panels — fill remaining pane height and allow inner content to scroll */
  [role='tabpanel'] {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .task-list-content {
    flex: 1;
    overflow-y: auto;
  }

  .state-message {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--spacing-12);
    font-size: var(--font-size-base);
    color: var(--color-text-secondary);
  }

  .empty-state {
    color: var(--color-text-tertiary);
  }

  .task-list {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .task-list-item {
    display: block;
  }
</style>
