<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts" module>
  import type { Priority } from '../../types';

  export interface SharedFieldValues {
    title: string;
    description: string;
    durationMinutes: number;
    priority: Priority;
    scheduleId: string;
    labels: string[];
  }

  export interface SharedFormController {
    validate(): boolean;
    getValues(): SharedFieldValues;
    resetErrors(): void;
  }
</script>

<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { Snippet } from 'svelte';
  import DurationInput from '../shared/DurationInput.svelte';
  import LabelChip from '../shared/LabelChip.svelte';
  import MarkdownView from '../shared/MarkdownView.svelte';
  import { scheduleState } from '../../stores/schedules.svelte';

  interface Props {
    open: boolean;
    initial: SharedFieldValues;
    idPrefix: string;
    scheduleRequired?: boolean;
    scheduleNullable?: boolean;
    extraFields?: Snippet;
    onready?: (controller: SharedFormController) => void;
    onvalueschange?: (values: SharedFieldValues) => void;
  }

  const {
    open,
    initial,
    idPrefix,
    scheduleRequired = false,
    scheduleNullable = false,
    extraFields,
    onready,
    onvalueschange,
  }: Props = $props();

  let titleValue = $state('');
  let descriptionValue = $state('');
  let durationMinutes = $state(60);
  let priority = $state<Priority>('Medium');
  let scheduleId = $state('');
  let labels = $state<string[]>([]);
  let labelInput = $state('');

  let showDescriptionPreview = $state(false);

  let titleError = $state('');
  let durationError = $state('');
  let scheduleError = $state('');

  let titleInputEl: HTMLInputElement | null = $state(null);

  // Schedule options (with optional stub for unknown schedule_id in edit mode)

  const scheduleOptions = $derived.by(() => {
    const options = scheduleState.items.map((s) => ({ id: s.id, name: s.name }));

    if (scheduleId && !options.some((o) => o.id === scheduleId)) {
      return [{ id: scheduleId, name: scheduleId }, ...options];
    }

    return options;
  });

  $effect(() => {
    if (open) {
      titleValue = initial.title;
      descriptionValue = initial.description;
      durationMinutes = initial.durationMinutes;
      priority = initial.priority;
      scheduleId = initial.scheduleId;
      labels = [...initial.labels];
      labelInput = '';
      showDescriptionPreview = false;

      titleError = '';
      durationError = '';
      scheduleError = '';

      untrack(() => {
        if (!scheduleState.loaded && !scheduleState.loading && scheduleState.items.length === 0) {
          scheduleState.load().catch(() => undefined);
        }
      });

      tick().then(() => {
        titleInputEl?.focus();
      });
    }
  });

  function addLabel(raw: string): void {
    const trimmed = raw.trim();
    if (!trimmed || labels.includes(trimmed)) {
      labelInput = '';
      return;
    }
    labels = [...labels, trimmed];
    labelInput = '';
  }

  function removeLabel(index: number): void {
    labels = labels.filter((_, i) => i !== index);
  }

  function handleLabelKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      addLabel(labelInput);
    }
  }

  function validate(): boolean {
    let valid = true;

    titleError = '';
    durationError = '';
    scheduleError = '';

    if (!titleValue.trim()) {
      titleError = 'Title is required';
      valid = false;
    }

    if (durationMinutes <= 0) {
      durationError = 'Duration must be greater than 0';
      valid = false;
    }

    if (scheduleRequired && !scheduleId) {
      scheduleError = 'Schedule is required';
      valid = false;
    }

    return valid;
  }

  function getValues(): SharedFieldValues {
    return {
      title: titleValue,
      description: descriptionValue,
      durationMinutes,
      priority,
      scheduleId,
      labels: [...labels],
    };
  }

  function resetErrors(): void {
    titleError = '';
    durationError = '';
    scheduleError = '';
  }

  const controller: SharedFormController = { validate, getValues, resetErrors };

  $effect(() => {
    onready?.(controller);
  });

  const currentValues = $derived.by(() => ({
    title: titleValue,
    description: descriptionValue,
    durationMinutes,
    priority,
    scheduleId,
    labels: [...labels],
  }));

  $effect(() => {
    onvalueschange?.(currentValues);
  });
</script>

<div class="form-field">
  <label class="field-label" for="{idPrefix}-title">Title <span class="required">*</span></label>
  <input
    id="{idPrefix}-title"
    bind:this={titleInputEl}
    class="field-input"
    class:field-error={!!titleError}
    type="text"
    placeholder="Task title"
    bind:value={titleValue}
    aria-describedby={titleError ? `${idPrefix}-title-error` : undefined}
    aria-invalid={!!titleError}
  />
  {#if titleError}
    <span id="{idPrefix}-title-error" class="error-text" role="alert">{titleError}</span>
  {/if}
</div>

<div class="form-field">
  <div class="description-label-row">
    <label class="field-label" for="{idPrefix}-description">Description</label>
    <button
      type="button"
      class="preview-toggle"
      aria-pressed={showDescriptionPreview}
      onclick={() => (showDescriptionPreview = !showDescriptionPreview)}
      >{showDescriptionPreview ? 'Edit' : 'Preview'}</button
    >
  </div>
  {#if showDescriptionPreview}
    <div class="description-preview">
      {#if descriptionValue.trim()}
        <MarkdownView source={descriptionValue} />
      {:else}
        <span class="preview-empty">Nothing to preview</span>
      {/if}
    </div>
  {:else}
    <textarea
      id="{idPrefix}-description"
      class="field-textarea"
      placeholder="Optional description"
      rows="3"
      bind:value={descriptionValue}
    ></textarea>
  {/if}
</div>

<div class="form-field">
  <DurationInput
    label="Duration *"
    value={durationMinutes}
    onchange={(v) => {
      durationMinutes = v;
      if (v > 0) durationError = '';
    }}
    min={5}
  />
  {#if durationError}
    <span id="{idPrefix}-duration-error" class="error-text" role="alert">{durationError}</span>
  {/if}
</div>

<div class="form-row">
  <div class="form-field">
    <label class="field-label" for="{idPrefix}-priority">Priority</label>
    <select id="{idPrefix}-priority" class="field-input" bind:value={priority}>
      <option value="Low">Low</option>
      <option value="Medium">Medium</option>
      <option value="High">High</option>
      <option value="Critical">Critical</option>
    </select>
  </div>

  <div class="form-field">
    <label class="field-label" for="{idPrefix}-schedule">
      Schedule{#if scheduleRequired}
        <span class="required">*</span>{/if}
    </label>
    <select
      id="{idPrefix}-schedule"
      class="field-input"
      class:field-error={!!scheduleError}
      bind:value={scheduleId}
      disabled={scheduleState.loading && scheduleOptions.length === 0}
      aria-describedby={scheduleError ? `${idPrefix}-schedule-error` : undefined}
      aria-invalid={!!scheduleError || undefined}
    >
      {#if scheduleNullable}
        <option value="">— None —</option>
      {/if}
      {#each scheduleOptions as schedule (schedule.id)}
        <option value={schedule.id}>{schedule.name}</option>
      {/each}
    </select>
    {#if scheduleError}
      <span id="{idPrefix}-schedule-error" class="error-text" role="alert">{scheduleError}</span>
    {/if}
  </div>
</div>

<!-- Extra fields slot (e.g. Deadline + Start date + Min chunk in TaskForm, or Cadence in RecurringListView) -->
{@render extraFields?.()}

<div class="form-field">
  <label class="field-label" for="{idPrefix}-labels">Labels</label>
  <div class="labels-section">
    {#if labels.length > 0}
      <div class="labels-chips">
        {#each labels as label, i (label)}
          <LabelChip
            {label}
            onremove={() => {
              removeLabel(i);
            }}
          />
        {/each}
      </div>
    {/if}
    <input
      id="{idPrefix}-labels"
      class="field-input"
      type="text"
      placeholder="Add label and press Enter"
      bind:value={labelInput}
      onkeydown={handleLabelKeydown}
    />
  </div>
</div>

<style>
  .error-text {
    font-size: var(--font-size-xs);
    color: var(--color-error);
  }

  .description-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-2);
  }

  .preview-toggle {
    padding: 2px var(--spacing-2);
    border: 1px solid var(--color-border);
  }

  .preview-toggle:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .description-preview {
    min-height: 4.5em;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--spacing-2) var(--spacing-3);
    overflow-y: auto;
    background: var(--color-bg);
  }

  .preview-empty {
    font-size: var(--font-size-sm);
    color: var(--color-text-tertiary);
    font-style: italic;
  }
</style>
