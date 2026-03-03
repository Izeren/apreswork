<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import type {
    Cadence,
    CreateTemplateInput,
    RecurringTemplate,
    Schedule,
    UpdateTemplateInput,
  } from '../../types';
  import { formatDateTime, formatDuration } from '../../utils';
  import { appClock } from '../../app-clock';
  import { templateState, TemplateState } from '../../stores/templates.svelte';
  import { scheduleState } from '../../stores/schedules.svelte';
  import { taskState, TaskState } from '../../stores/tasks.svelte';
  import { toastState } from '../../stores/toast.svelte';
  import Modal from '../shared/Modal.svelte';
  import ConfirmDialog from '../shared/ConfirmDialog.svelte';
  import LabelChip from '../shared/LabelChip.svelte';
  import DateTimePicker from '../shared/DateTimePicker.svelte';
  import RecurringSection from './RecurringSection.svelte';
  import SharedFormFields from './SharedFormFields.svelte';
  import type { SharedFormController, SharedFieldValues } from './SharedFormFields.svelte';

  interface TemplateEditRequest {
    id: string;
    nonce: number;
  }

  interface TemplateCreateRequest {
    seed: SharedFieldValues;
    nonce: number;
  }

  interface Props {
    editRequest?: TemplateEditRequest | null;
    oneditrequesthandled?: () => void;
    createRequest?: TemplateCreateRequest | null;
    oncreaterequesthandled?: () => void;
    templateStore?: TemplateState;
    taskStore?: TaskState;
    getNow?: () => Date;
  }

  const {
    editRequest = null,
    oneditrequesthandled,
    createRequest = null,
    oncreaterequesthandled,
    templateStore = templateState,
    taskStore = taskState,
    getNow = appClock,
  }: Props = $props();

  let editorOpen = $state(false);
  let deleteDialogOpen = $state(false);
  let saving = $state(false);
  let busyTemplateId = $state<string | null>(null);
  let deleteTarget = $state<RecurringTemplate | null>(null);
  let handledRequestNonce = $state<number | null>(null);
  let pendingEditTemplateId = $state<string | null>(null);

  let editingTemplateId = $state<string | null>(null);
  let editorMode = $state<'create' | 'edit'>('edit');
  const editorSubmitLabel = $derived(editorMode === 'create' ? 'Create template' : 'Save changes');
  // Bumped on every open so the keyed form fully remounts (resetting shared fields).
  let editorInstanceKey = $state(0);
  let handledCreateNonce = $state<number | null>(null);
  let cadence = $state<Cadence>({ period: 'Weekly', interval: 1, windows: [{ start: 0, end: 0 }] });
  // Recurrence anchor (template.start_date). Always set when the editor opens.
  let startDate = $state<string | null>(null);

  function defaultSharedValues(): SharedFieldValues {
    return {
      title: '',
      description: '',
      durationMinutes: 60,
      priority: 'Medium',
      scheduleId: '',
      labels: [],
    };
  }

  let sharedInitial = $state<SharedFieldValues>(defaultSharedValues());
  let sharedValues = $state<SharedFieldValues>(defaultSharedValues());

  let sharedController = $state<SharedFormController | null>(null);

  const sortedTemplates = $derived.by(() =>
    [...templateStore.items].sort((a, b) => {
      if (a.is_active !== b.is_active) {
        return a.is_active ? -1 : 1;
      }
      return a.title.localeCompare(b.title);
    }),
  );

  function loadIfNeeded(state: {
    loaded: boolean;
    loading: boolean;
    load: () => Promise<void>;
  }): void {
    if (!state.loaded && !state.loading) {
      state.load().catch(() => undefined);
    }
  }

  $effect(() => {
    untrack(() => {
      loadIfNeeded(templateStore);
      loadIfNeeded(scheduleState);
    });
  });

  $effect(() => {
    if (!editRequest || editRequest.nonce === handledRequestNonce) {
      return;
    }

    handledRequestNonce = editRequest.nonce;
    pendingEditTemplateId = editRequest.id;
    templateStore.select(editRequest.id);

    loadIfNeeded(templateStore);
    loadIfNeeded(scheduleState);

    oneditrequesthandled?.();
  });

  $effect(() => {
    if (!createRequest || createRequest.nonce === handledCreateNonce) {
      return;
    }

    handledCreateNonce = createRequest.nonce;
    openCreate(createRequest.seed);
    oncreaterequesthandled?.();
  });

  $effect(() => {
    if (!pendingEditTemplateId) {
      return;
    }

    if (templateStore.loading) {
      return;
    }

    if (templateStore.selected?.id === pendingEditTemplateId) {
      openEditor(templateStore.selected);
      pendingEditTemplateId = null;
      return;
    }

    if (templateStore.loaded) {
      toastState.error('Recurring template not found');
      templateStore.select(null);
      pendingEditTemplateId = null;
    }
  });

  const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  function cloneCadence(value: Cadence): Cadence {
    return {
      period: value.period,
      interval: value.interval,
      windows: value.windows.map((w) => ({ ...w })),
    };
  }

  function initializeSharedValues(source: SharedFieldValues): void {
    sharedInitial = { ...source, labels: [...source.labels] };
    sharedValues = { ...sharedInitial, labels: [...sharedInitial.labels] };
  }

  function openEditor(template: RecurringTemplate): void {
    templateStore.select(template.id);
    editingTemplateId = template.id;
    editorMode = 'edit';
    editorInstanceKey += 1;
    initializeSharedValues({
      title: template.title,
      description: template.description ?? '',
      durationMinutes: template.duration_minutes,
      priority: template.priority,
      scheduleId: template.schedule_id,
      labels: template.labels,
    });
    cadence = cloneCadence(template.cadence);
    startDate = template.start_date;
    editorOpen = true;
  }

  // Open the shared editor in create mode, seeded from the task form's shared
  // fields. There is no template yet, so submit goes through templateStore.create.
  function openCreate(seed: SharedFieldValues): void {
    templateStore.select(null);
    editingTemplateId = null;
    editorMode = 'create';
    editorInstanceKey += 1;
    initializeSharedValues(seed);
    cadence = { period: 'Weekly', interval: 1, windows: [{ start: 0, end: 0 }] };
    startDate = getNow().toISOString();
    editorOpen = true;
  }

  function closeEditor(): void {
    editorOpen = false;
    editingTemplateId = null;
    templateStore.select(null);
    sharedController?.resetErrors();
  }

  /** Field mapping shared by create and update: both take the same shared-form shape. */
  function sharedTemplateFields() {
    return {
      title: sharedValues.title.trim(),
      description: sharedValues.description.trim() || null,
      duration_minutes: sharedValues.durationMinutes,
      priority: sharedValues.priority,
      schedule_id: sharedValues.scheduleId,
      labels: sharedValues.labels,
      cadence: cloneCadence(cadence),
      start_date: startDate ?? undefined,
    };
  }

  async function cleanupAfterMutation(): Promise<void> {
    closeEditor();
    await taskStore.load();
  }

  async function doMutation(mutate: () => Promise<unknown>): Promise<void> {
    const result = await mutate();
    saving = false;
    if (!result) return;
    await cleanupAfterMutation();
  }

  async function handleSubmit(): Promise<void> {
    const sharedValid = sharedController?.validate() ?? false;
    if (!sharedValid) {
      return;
    }

    saving = true;

    if (editorMode === 'create') {
      const input: CreateTemplateInput = sharedTemplateFields();
      await doMutation(() => templateStore.create(input));
      return;
    }

    if (!editingTemplateId) {
      saving = false;
      return;
    }

    const id = editingTemplateId;
    const input: UpdateTemplateInput = sharedTemplateFields();
    await doMutation(() => templateStore.update(id, input));
  }

  function handleFormSubmit(event: Event): void {
    event.preventDefault();
    void handleSubmit();
  }

  async function withBusyState(id: string, mutate: () => Promise<unknown>): Promise<boolean> {
    busyTemplateId = id;
    const result = await mutate();
    busyTemplateId = null;
    if (!result) return false;
    await taskStore.load();
    return true;
  }

  async function handleToggle(template: RecurringTemplate): Promise<void> {
    await withBusyState(template.id, () => templateStore.toggleActive(template.id));
  }

  function promptDelete(template: RecurringTemplate): void {
    deleteTarget = template;
    deleteDialogOpen = true;
  }

  async function confirmDelete(): Promise<void> {
    if (!deleteTarget) return;
    const deletingId = deleteTarget.id;
    deleteDialogOpen = false;
    deleteTarget = null;
    busyTemplateId = deletingId;
    const deleted = await templateStore.remove(deletingId);
    busyTemplateId = null;
    if (!deleted) return;
    if (editingTemplateId === deletingId) {
      closeEditor();
    }
    await taskStore.load();
  }

  function weeklyWindowLabel(window: Cadence['windows'][number]): string {
    return window.start === window.end
      ? WEEKDAYS[window.start]
      : `${WEEKDAYS[window.start]}–${WEEKDAYS[window.end]}`;
  }

  function cadenceSummary(value: Cadence): string {
    if (value.period === 'Monthly') {
      const days = value.windows.map((window) => window.start + 1).join(', ');
      const prefix = value.interval > 1 ? `Every ${value.interval} months` : 'Monthly';
      return `${prefix} on day ${days}`;
    }

    const spans = value.windows.map(weeklyWindowLabel).join(', ');
    const prefix = value.interval > 1 ? `Every ${value.interval} weeks` : 'Weekly';
    return `${prefix} on ${spans}`;
  }

  function resolveSchedule(scheduleIdToResolve: string): Pick<Schedule, 'id' | 'name'> | null {
    return scheduleState.items.find((schedule) => schedule.id === scheduleIdToResolve) ?? null;
  }
</script>

<div class="recurring-list-view">
  <header class="recurring-header">
    <div>
      <h2 class="recurring-title">Recurring templates</h2>
      <p class="recurring-subtitle">Manage future recurring work without leaving the task list.</p>
    </div>
  </header>

  {#if templateStore.loading && !templateStore.loaded}
    <div class="state-message" aria-live="polite" aria-busy="true">
      Loading recurring templates…
    </div>
  {:else if sortedTemplates.length === 0}
    <div class="state-message empty-state" aria-live="polite">No recurring templates yet.</div>
  {:else}
    <ul class="template-list" role="list" aria-label="Recurring templates">
      {#each sortedTemplates as template (template.id)}
        <li class="template-card">
          <div class="template-card__top">
            <div class="template-heading">
              <div class="template-title-row">
                <h3 class="template-title">{template.title}</h3>
                <span
                  class="status-badge"
                  class:status-badge--active={template.is_active}
                  class:status-badge--inactive={!template.is_active}
                >
                  {template.is_active ? 'Active' : 'Inactive'}
                </span>
              </div>

              {#if template.description}
                <p class="template-description">{template.description}</p>
              {:else}
                <p class="template-description template-description--empty">No description</p>
              {/if}
            </div>

            <div class="template-actions">
              <button
                class="secondary-btn"
                onclick={() => void handleToggle(template)}
                disabled={busyTemplateId === template.id}
              >
                {template.is_active ? 'Deactivate' : 'Activate'}
              </button>
              <button class="secondary-btn" onclick={() => openEditor(template)}>Edit</button>
              <button
                class="danger-btn"
                onclick={() => promptDelete(template)}
                disabled={busyTemplateId === template.id}
              >
                Delete
              </button>
            </div>
          </div>

          <dl class="template-meta">
            <div>
              <dt>Duration</dt>
              <dd>{formatDuration(template.duration_minutes)}</dd>
            </div>
            <div>
              <dt>Priority</dt>
              <dd>{template.priority}</dd>
            </div>
            <div>
              <dt>Schedule</dt>
              <dd>{resolveSchedule(template.schedule_id)?.name ?? template.schedule_id}</dd>
            </div>
            <div>
              <dt>Cadence</dt>
              <dd>{cadenceSummary(template.cadence)}</dd>
            </div>
            <div>
              <dt>Updated</dt>
              <dd>{formatDateTime(template.updated_at)}</dd>
            </div>
          </dl>

          {#if template.labels.length > 0}
            <div class="template-labels" aria-label="Template labels">
              {#each template.labels as label (label)}
                <LabelChip {label} />
              {/each}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<Modal
  open={editorOpen}
  title={editorMode === 'create' ? 'New recurring template' : 'Edit recurring template'}
  onclose={closeEditor}
>
  {#if editorOpen}
    {#key editorInstanceKey}
      <form class="template-form" onsubmit={handleFormSubmit} novalidate>
        <SharedFormFields
          open={editorOpen}
          initial={sharedInitial}
          idPrefix="template"
          scheduleRequired={true}
          onready={(ctrl) => {
            sharedController = ctrl;
          }}
          onvalueschange={(values) => {
            sharedValues = values;
          }}
        >
          {#snippet extraFields()}
            <!-- Start date (recurrence anchor) — day precision; the backend truncates to the start of the day. -->
            <div class="form-field">
              <DateTimePicker
                label="Start date"
                value={startDate}
                defaultTime="00:00"
                now={getNow()}
                onchange={(v) => {
                  startDate = v;
                }}
              />
            </div>

            <div class="form-field">
              <span class="field-label">Cadence</span>
              <RecurringSection
                {cadence}
                onchange={(nextCadence) => {
                  cadence = cloneCadence(nextCadence);
                }}
              />
            </div>
          {/snippet}
        </SharedFormFields>

        <div class="form-footer">
          <button type="button" onclick={closeEditor}>Cancel</button>
          <button type="submit" class="btn-primary" disabled={saving}>
            {saving ? 'Saving…' : editorSubmitLabel}
          </button>
        </div>
      </form>
    {/key}
  {/if}
</Modal>

<ConfirmDialog
  open={deleteDialogOpen}
  title="Delete recurring template"
  message={deleteTarget
    ? `Delete '${deleteTarget.title}'? Pending and scheduled instances will be removed, while completed and cancelled history stays intact.`
    : 'Delete this recurring template?'}
  confirmLabel="Delete"
  cancelLabel="Keep template"
  destructive={true}
  onconfirm={() => void confirmDelete()}
  oncancel={() => {
    deleteDialogOpen = false;
    deleteTarget = null;
  }}
/>

<style>
  .recurring-list-view {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .recurring-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--spacing-4);
  }

  .recurring-title {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .recurring-subtitle {
    margin-top: var(--spacing-1);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .state-message {
    padding: var(--spacing-8) var(--spacing-4);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    text-align: center;
  }

  .template-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-3);
    padding: 0;
    margin: 0;
  }

  .template-card {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
    padding: var(--spacing-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
  }

  .template-card__top {
    display: flex;
    justify-content: space-between;
    gap: var(--spacing-4);
  }

  .template-heading {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
    min-width: 0;
  }

  .template-title-row {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    flex-wrap: wrap;
  }

  .template-title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .template-description {
    color: var(--color-text-secondary);
    line-height: var(--line-height);
  }

  .template-description--empty {
    color: var(--color-text-tertiary);
  }

  .status-badge {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--spacing-2);
    border-radius: 999px;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
  }

  .status-badge--active {
    background: rgba(34, 197, 94, 0.12);
    color: #166534;
  }

  .status-badge--inactive {
    background: rgba(148, 163, 184, 0.16);
    color: var(--color-text-secondary);
  }

  .template-actions {
    display: flex;
    align-items: flex-start;
    gap: var(--spacing-2);
    flex-wrap: wrap;
  }

  .secondary-btn,
  .danger-btn {
    border-radius: var(--radius-md);
    padding: var(--spacing-2) var(--spacing-3);
    font-size: var(--font-size-sm);
  }

  .danger-btn {
    color: var(--color-error);
    border-color: rgba(239, 68, 68, 0.3);
  }

  .template-meta {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--spacing-3);
    margin: 0;
  }

  .template-meta dt {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
    margin-bottom: var(--spacing-1);
  }

  .template-meta dd {
    margin: 0;
    color: var(--color-text);
    font-size: var(--font-size-sm);
  }

  .template-labels {
    display: flex;
    flex-wrap: wrap;
    gap: var(--spacing-1);
  }

  .template-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .form-footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-3);
    padding-top: var(--spacing-2);
    border-top: 1px solid var(--color-border-light);
    margin-top: var(--spacing-2);
  }

  @media (max-width: 960px) {
    .template-card__top {
      flex-direction: column;
    }

    .template-meta {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 640px) {
    .template-meta {
      grid-template-columns: 1fr;
    }
  }
</style>
