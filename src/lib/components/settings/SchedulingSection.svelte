<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import {
    defaultSchedulingSectionApi,
    type SchedulingSectionApi,
  } from './schedulingSectionShared';
  import type { AppConfig } from '../../types';
  import type { QuickDateAnchor } from '../../utils';
  import { toastState } from '../../stores/toast.svelte';
  import { loadQuickDateAnchor, saveQuickDateAnchor } from '../../quickDateAnchorPref';

  const { apiClient = defaultSchedulingSectionApi }: { apiClient?: SchedulingSectionApi } =
    $props();

  let config: AppConfig | null = $state(null);
  let loadError: string | null = $state(null);
  let saving: boolean = $state(false);

  let horizonDraft: number | null = $state(null);
  let maxContinuousDraft: number | null = $state(null);
  let minBreakDraft: number | null = $state(null);
  let timezoneDraft: string = $state('');

  let anchor: QuickDateAnchor = $state(loadQuickDateAnchor(window.localStorage));

  const ANCHOR_OPTIONS: { value: QuickDateAnchor; label: string }[] = [
    { value: 'auto', label: 'Auto' },
    { value: 'fri', label: 'Friday' },
    { value: 'sat', label: 'Saturday' },
    { value: 'sun', label: 'Sunday' },
  ];

  const dirty = $derived.by(() => {
    if (config === null) return false;
    return (
      horizonDraft !== config.planning_horizon_days ||
      maxContinuousDraft !== config.max_continuous_minutes ||
      minBreakDraft !== config.min_break_minutes ||
      timezoneDraft !== config.timezone
    );
  });

  /**
   * IANA zones from the runtime, with the stored value prepended when the
   * runtime does not know it — the select must never silently rewrite a
   * timezone it cannot represent. Guarded: older webviews may lack
   * `Intl.supportedValuesOf` (the list is then just the stored value).
   */
  const timezoneOptions = $derived.by(() => {
    let zones: string[];
    try {
      zones = Intl.supportedValuesOf('timeZone');
    } catch {
      zones = [];
    }
    if (timezoneDraft && !zones.includes(timezoneDraft)) {
      return [timezoneDraft, ...zones];
    }
    return zones;
  });

  function syncDrafts(next: AppConfig): void {
    horizonDraft = next.planning_horizon_days;
    maxContinuousDraft = next.max_continuous_minutes;
    minBreakDraft = next.min_break_minutes;
    timezoneDraft = next.timezone;
  }

  function load(): void {
    apiClient
      .getConfig()
      .then((c) => {
        config = c;
        loadError = null;
        syncDrafts(c);
      })
      .catch((e) => {
        loadError = apiClient.apiErrorMessage(e, 'Could not load scheduling settings.');
      });
  }

  /** Light client check for UX; the backend range validation is the trust boundary. */
  function requireInteger(value: number | null): number | null {
    return value !== null && Number.isInteger(value) ? value : null;
  }

  function validateAndToastInteger(value: number | null, fieldName: string): number | null {
    const result = requireInteger(value);
    if (result === null) toastState.error(`${fieldName} must be a whole number.`);
    return result;
  }

  function handleSave(): void {
    const horizon = validateAndToastInteger(horizonDraft, 'Planning horizon');
    if (horizon === null) return;
    const maxContinuous = validateAndToastInteger(maxContinuousDraft, 'Max continuous work');
    if (maxContinuous === null) return;
    const minBreak = validateAndToastInteger(minBreakDraft, 'Break between work blocks');
    if (minBreak === null) return;

    saving = true;
    apiClient
      .updateConfig({
        planning_horizon_days: horizon,
        max_continuous_minutes: maxContinuous,
        min_break_minutes: minBreak,
        timezone: timezoneDraft,
      })
      .then((c) => {
        config = c;
        syncDrafts(c);
        toastState.success('Scheduling settings saved.');
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Could not save scheduling settings.'));
      })
      .finally(() => {
        saving = false;
      });
  }

  function setAnchor(next: QuickDateAnchor): void {
    if (next === anchor) return;
    anchor = next;
    saveQuickDateAnchor(next, window.localStorage);
  }

  // Mount-only: load() has no tracked reactive reads — keep it that way.
  $effect(() => {
    load();
  });
</script>

<div class="settings-card scheduling-card">
  <h3 class="card-title">Scheduling</h3>

  {#if loadError}
    <p class="error-text" role="alert">{loadError}</p>
    <button class="btn-sm" onclick={load}>Retry</button>
  {:else if config === null}
    <p class="muted">Loading…</p>
  {:else}
    <div class="field-list">
      <label class="field">
        <span class="field-label">Planning horizon (days)</span>
        <input type="number" min="1" max="365" bind:value={horizonDraft} />
      </label>
      <label class="field">
        <span class="field-label">Max continuous work (minutes)</span>
        <input type="number" min="15" max="1440" step="5" bind:value={maxContinuousDraft} />
      </label>
      <label class="field">
        <span class="field-label">Break between work blocks (minutes)</span>
        <input type="number" min="0" max="480" step="5" bind:value={minBreakDraft} />
      </label>
      <label class="field">
        <span class="field-label">Timezone</span>
        <select bind:value={timezoneDraft}>
          {#each timezoneOptions as zone (zone)}
            <option value={zone}>{zone}</option>
          {/each}
        </select>
      </label>
    </div>

    <div class="button-row-start">
      <button class="btn-primary" onclick={handleSave} disabled={!dirty || saving}>
        {saving ? 'Saving…' : 'Save scheduling settings'}
      </button>
    </div>
    <p class="muted">Saving replans the schedule with the new settings.</p>

    <div class="anchor-section">
      <span class="field-label" id="quick-date-anchor-label">“This week” lands on</span>
      <div class="anchor-toggle" role="group" aria-labelledby="quick-date-anchor-label">
        {#each ANCHOR_OPTIONS as option (option.value)}
          <button
            type="button"
            class="anchor-btn"
            class:anchor-btn--active={anchor === option.value}
            aria-pressed={anchor === option.value}
            onclick={() => setAnchor(option.value)}>{option.label}</button
          >
        {/each}
      </div>
      <p class="muted">
        Where the date picker’s week quick dates point. Auto follows the picker’s week start.
        Applies immediately.
      </p>
    </div>
  {/if}
</div>

<style>
  .scheduling-card {
    margin-top: var(--spacing-6);
  }

  .field-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-3);
  }

  .field {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 200px;
    align-items: center;
    gap: var(--spacing-3);
    font-size: var(--font-size-sm);
    color: var(--color-text);
  }

  .field input,
  .field select {
    width: 100%;
    box-sizing: border-box;
    padding: var(--spacing-2);
    font-size: var(--font-size-sm);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
  }

  .anchor-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
    padding-top: var(--spacing-3);
    border-top: 1px solid var(--color-border);
  }

  .anchor-toggle {
    display: flex;
    gap: 2px;
  }

  .anchor-btn {
    flex: 1 1 0;
    padding: 5px 8px;
    font-size: var(--font-size-xs);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: transparent;
    box-shadow: none;
    text-align: center;
    color: var(--color-text);
    cursor: pointer;
  }

  .anchor-btn:hover,
  .anchor-btn:focus-visible {
    border-color: var(--color-border-light);
    background: var(--color-surface-hover);
  }

  .anchor-btn--active {
    border-color: var(--color-primary);
    background: var(--color-primary-light);
  }
</style>
