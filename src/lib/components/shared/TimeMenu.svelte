<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { tick } from 'svelte';
  import {
    STICKY_TIME_OPTIONS,
    PICKER_MARGIN,
    PICKER_GAP,
    buildTimeOptions,
    type TimeOption,
  } from './dateTimePickerShared';
  import { repositionOnViewportChange } from './viewportReposition.svelte';
  import { computePositioningStyle } from './popoverPosition';

  interface Props {
    value: string;
    onselect: (time: string) => void;
    open?: boolean;
  }

  let { value, onselect, open = $bindable(false) }: Props = $props();

  const TIME_MENU_MIN_WIDTH = 220;
  const TIME_MENU_MAX_HEIGHT = 320;

  const dropdownTimeOptions = buildTimeOptions().filter(
    (option) => !STICKY_TIME_OPTIONS.some((sticky) => sticky.value === option.value),
  );

  let timeTriggerEl: HTMLButtonElement | null = $state(null);
  let timeMenuListEl: HTMLDivElement | null = $state(null);
  let timeMenuStyle = $state('');

  function updateTimeMenuPosition() {
    if (!open || !timeTriggerEl) return;

    const rect = timeTriggerEl.getBoundingClientRect();
    const width = Math.max(TIME_MENU_MIN_WIDTH, Math.round(rect.width));
    const availableBelow = window.innerHeight - rect.bottom - PICKER_MARGIN - PICKER_GAP;
    const availableAbove = rect.top - PICKER_MARGIN - PICKER_GAP;
    const openAbove = availableBelow < 240 && availableAbove > availableBelow;
    const maxHeight = Math.max(
      160,
      Math.min(TIME_MENU_MAX_HEIGHT, openAbove ? availableAbove : availableBelow),
    );

    let left = rect.left;
    if (left + width > window.innerWidth - PICKER_MARGIN) {
      left = window.innerWidth - width - PICKER_MARGIN;
    }
    left = Math.max(PICKER_MARGIN, left);

    const top = openAbove ? rect.top - maxHeight - PICKER_GAP : rect.bottom + PICKER_GAP;

    const maxHeightStyle = `${Math.round(maxHeight)}px`;
    timeMenuStyle = computePositioningStyle(top, left, width, maxHeightStyle);
  }

  function scrollSelectedTimeIntoView() {
    if (!open || !timeMenuListEl) return;
    if (STICKY_TIME_OPTIONS.some((option) => option.value === value)) return;

    const activeOption = timeMenuListEl.querySelector<HTMLButtonElement>('.option-btn--active');
    if (!activeOption) return;

    const listRect = timeMenuListEl.getBoundingClientRect();
    const optionRect = activeOption.getBoundingClientRect();
    const relativeTop = optionRect.top - listRect.top + timeMenuListEl.scrollTop;
    const targetScrollTop = Math.max(
      0,
      relativeTop - (timeMenuListEl.clientHeight - optionRect.height) / 2,
    );
    timeMenuListEl.scrollTop = targetScrollTop;
  }

  async function toggleTimeMenu() {
    open = !open;
    if (!open) return;

    await tick();
    updateTimeMenuPosition();
    scrollSelectedTimeIntoView();
  }

  function handleTimeOptionSelect(time: string) {
    onselect(time);
    open = false;
  }

  repositionOnViewportChange(() => open, updateTimeMenuPosition);
</script>

<div class="time-field">
  <button
    bind:this={timeTriggerEl}
    type="button"
    class="time-select"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label="Time"
    onclick={toggleTimeMenu}
  >
    <span>{value}</span>
    <span class="time-select-caret">&#x25BE;</span>
  </button>

  {#snippet timeOptionButton(option: TimeOption, showValue: boolean)}
    <button
      type="button"
      class="option-btn option-row time-menu-option"
      class:option-btn--active={value === option.value}
      data-time={option.value}
      onclick={() => {
        handleTimeOptionSelect(option.value);
      }}
    >
      <span>{option.label}</span>
      {#if showValue}<span>{option.value}</span>{/if}
    </button>
  {/snippet}

  {#if open}
    <div class="time-menu" style={timeMenuStyle} role="listbox" aria-label="Time options">
      <div class="time-menu-section">
        <span class="time-menu-label">Quick times</span>

        {#each STICKY_TIME_OPTIONS as option (option.value)}
          <!-- eslint-disable-next-line sonarjs/no-use-of-empty-return-value -- false positive: snippet render, not a value use -->
          {@render timeOptionButton(option, true)}
        {/each}
      </div>

      <div class="time-menu-divider"></div>

      <div class="time-menu-section time-menu-section--scrollable">
        <span class="time-menu-label">All times</span>

        <div class="time-menu-list" bind:this={timeMenuListEl}>
          {#each dropdownTimeOptions as option (option.value)}
            <!-- eslint-disable-next-line sonarjs/no-use-of-empty-return-value -- false positive: snippet render, not a value use -->
            {@render timeOptionButton(option, false)}
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .time-field {
    position: relative;
  }

  .time-select {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-2);
    width: 100%;
    padding: 6px 8px;
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
  }

  .time-select-caret {
    color: var(--color-text-secondary);
  }

  .time-menu {
    position: fixed;
    z-index: 1200;
    padding: var(--spacing-2);
    border: 1px solid var(--color-border-light);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    box-shadow: var(--shadow-md);
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: var(--spacing-2);
    overflow: hidden;
  }

  .time-menu-section {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-height: 0;
  }

  .time-menu-section--scrollable {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    overflow: hidden;
  }

  .time-menu-label {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    color: var(--color-text-tertiary);
  }

  .time-menu-divider {
    height: 1px;
    background: var(--color-border-light);
  }

  .time-menu-list {
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .time-menu-option {
    font-size: var(--font-size-sm);
    font-variant-numeric: tabular-nums;
  }

  @media (max-width: 720px) {
    .time-select {
      width: 100%;
    }
  }
</style>
