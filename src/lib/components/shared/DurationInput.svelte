<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import { formatDuration, parseDuration } from '../../utils';

  interface Props {
    value: number;
    onchange: (minutes: number) => void;
    label?: string;
    min?: number;
    disabled?: boolean;
  }

  interface DurationPreset {
    minutes: number;
    label: string;
  }

  const PRESETS: DurationPreset[] = [
    { minutes: 5, label: '5 min' },
    { minutes: 15, label: '15 min' },
    { minutes: 30, label: '30 min' },
    { minutes: 45, label: '45 min' },
    { minutes: 60, label: '1 hour' },
    { minutes: 120, label: '2 hours' },
    { minutes: 240, label: '4 hours' },
    { minutes: 480, label: '8 hours' },
  ];

  const { value, onchange, label = 'Duration', min = 5, disabled = false }: Props = $props();

  // Display text in the input; initialised from the numeric value prop.
  let inputText = $state(untrack(() => formatDuration(value)));
  // null = not yet committed; will be derived on blur/Enter
  let parsedValue = $state<number | null>(untrack(() => value));
  let parseError = $state(false);

  // Dropdown state
  let dropdownOpen = $state(false);
  let highlightedIndex = $state(-1);
  // Flag set on pointerdown of an option to avoid closing the dropdown on blur
  let selectingOption = $state(false);
  // True only after the user has typed something since the last focus; prevents
  // the formatted value from filtering the presets on initial focus.
  let userIsTyping = $state(false);

  // When the value prop changes externally, sync the display text.
  $effect(() => {
    inputText = formatDuration(value);
    parsedValue = value;
    parseError = false;
  });

  const belowMin = $derived(parsedValue !== null && parsedValue < min);
  const errorDescribedBy = $derived.by(() => {
    if (parseError) return 'duration-parse-error';
    if (belowMin) return 'duration-min-error';
    return undefined;
  });
  const preview = $derived(
    parsedValue !== null && !parseError ? formatDuration(parsedValue) : null,
  );

  /** The user's input parsed live (null if unparseable). */
  const liveParsed = $derived.by(() => {
    if (!userIsTyping) return null;
    const text = inputText.trim();
    if (text === '') return null;
    return parseDuration(text);
  });

  const filteredPresets = $derived.by(() => {
    // Show all presets when the dropdown first opens (before the user modifies
    // the text). Once the user starts typing we filter.
    if (!userIsTyping) return PRESETS;

    const text = inputText.trim();
    if (text === '') return PRESETS;

    const lower = text.toLowerCase();

    return PRESETS.filter((preset) => {
      if (liveParsed !== null && preset.minutes === liveParsed) return true;
      if (preset.label.toLowerCase().includes(lower)) return true;
      return false;
    });
  });

  /** A dynamic option showing what the user typed, formatted canonically.
   *  Only shown when the parsed value doesn't match any visible preset
   *  and is at least the minimum duration. */
  const customOption = $derived.by((): DurationPreset | null => {
    if (!userIsTyping || liveParsed === null || liveParsed < min) return null;
    // Don't show if it already matches a preset in the filtered list
    if (filteredPresets.some((p) => p.minutes === liveParsed)) return null;
    return { minutes: liveParsed, label: formatDuration(liveParsed) };
  });

  /** Total options visible in the dropdown (presets + optional custom). */
  const totalOptions = $derived(filteredPresets.length + (customOption ? 1 : 0));
  const hasDropdownContent = $derived(totalOptions > 0);

  /** Map a flat dropdown index to the corresponding option.
   *  Index 0 is the custom option (if present), then presets follow. */
  function getOptionAtIndex(index: number): DurationPreset | null {
    if (customOption) {
      if (index === 0) return customOption;
      return filteredPresets[index - 1] ?? null;
    }
    return filteredPresets[index] ?? null;
  }

  function selectPreset(preset: DurationPreset) {
    inputText = preset.label;
    parsedValue = preset.minutes;
    parseError = false;
    dropdownOpen = false;
    highlightedIndex = -1;
    userIsTyping = false;
    onchange(preset.minutes);
  }

  function commit() {
    const result = parseDuration(inputText);
    if (result === null) {
      parseError = true;
      parsedValue = null;
      return;
    }
    parseError = false;
    parsedValue = result;
    // Normalise the display text to canonical form after a successful parse.
    inputText = formatDuration(result);
    onchange(result);
  }

  function handleFocus(event: FocusEvent) {
    dropdownOpen = true;
    highlightedIndex = -1;
    userIsTyping = false;
    // Clear the field so the user can start typing immediately; the previous
    // value is still shown as a checkmarked preset in the dropdown.
    inputText = '';
    (event.target as HTMLInputElement).value = '';
  }

  function handleBlur() {
    if (selectingOption) {
      selectingOption = false;
      return;
    }
    dropdownOpen = false;
    highlightedIndex = -1;
    userIsTyping = false;
    // If the user cleared/focused but didn't type anything, restore the
    // previous value instead of committing an empty string.
    if (inputText.trim() === '') {
      inputText = formatDuration(value);
      parsedValue = value;
      parseError = false;
      return;
    }
    commit();
  }

  /** Arrow-key navigation inside the open dropdown; true when the key was handled. */
  function handleDropdownNav(event: KeyboardEvent): boolean {
    if (!dropdownOpen) return false;

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      highlightedIndex = totalOptions === 0 ? -1 : (highlightedIndex + 1) % totalOptions;
      return true;
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (totalOptions === 0) {
        highlightedIndex = -1;
      } else {
        highlightedIndex = highlightedIndex <= 0 ? totalOptions - 1 : highlightedIndex - 1;
      }
      return true;
    }

    return false;
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      dropdownOpen = false;
      highlightedIndex = -1;
      // Restore the previous value so the field isn't left blank
      inputText = formatDuration(value);
      userIsTyping = false;
      return;
    }

    if (handleDropdownNav(event)) return;

    if (event.key === 'Enter') {
      if (dropdownOpen && highlightedIndex >= 0) {
        const option = getOptionAtIndex(highlightedIndex);
        if (option) {
          selectPreset(option);
          return;
        }
      }
      commit();
    }
  }

  function handleInput(event: Event) {
    inputText = (event.target as HTMLInputElement).value;
    // Clear errors while typing so the user is not distracted mid-edit.
    parseError = false;
    parsedValue = null;
    dropdownOpen = true;
    highlightedIndex = -1;
    userIsTyping = true;
  }

  function handleOptionPointerDown() {
    selectingOption = true;
  }

  const listboxId = 'duration-listbox';

  function optionId(index: number): string {
    return `duration-option-${index.toString()}`;
  }
</script>

<div class="duration-input">
  <span class="duration-label">{label}</span>
  <div class="duration-field-wrapper">
    <input
      class="duration-field"
      type="text"
      role="combobox"
      aria-label={label}
      aria-invalid={parseError || belowMin}
      aria-expanded={dropdownOpen}
      aria-autocomplete="list"
      aria-controls={listboxId}
      aria-activedescendant={highlightedIndex >= 0 ? optionId(highlightedIndex) : undefined}
      aria-describedby={errorDescribedBy}
      {disabled}
      value={inputText}
      oninput={handleInput}
      onblur={handleBlur}
      onfocus={handleFocus}
      onkeydown={handleKeyDown}
    />

    {#if dropdownOpen && hasDropdownContent}
      <ul id={listboxId} class="duration-dropdown" role="listbox" aria-label="Duration presets">
        {#if customOption}
          {@const flatIndex = 0}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- Keyboard navigation is managed by the parent combobox input -->
          <li
            id={optionId(flatIndex)}
            class="duration-option duration-option--custom"
            class:duration-option--highlighted={flatIndex === highlightedIndex}
            role="option"
            aria-selected={customOption.minutes === value}
            data-minutes={customOption.minutes}
            onpointerdown={handleOptionPointerDown}
            onclick={() => selectPreset(customOption)}
          >
            <span class="duration-option-label">{customOption.label}</span>
            {#if customOption.minutes === value}
              <span class="duration-option-check" aria-hidden="true">✓</span>
            {/if}
          </li>
        {/if}
        {#each filteredPresets as preset, index (preset.minutes)}
          {@const flatIndex = customOption ? index + 1 : index}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- Keyboard navigation is managed by the parent combobox input -->
          <li
            id={optionId(flatIndex)}
            class="duration-option"
            class:duration-option--highlighted={flatIndex === highlightedIndex}
            role="option"
            aria-selected={preset.minutes === value}
            data-minutes={preset.minutes}
            onpointerdown={handleOptionPointerDown}
            onclick={() => selectPreset(preset)}
          >
            <span class="duration-option-label">{preset.label}</span>
            {#if preset.minutes === value}
              <span class="duration-option-check" aria-hidden="true">✓</span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if preview !== null}
    <span class="duration-preview">= {preview}</span>
  {/if}
  {#if parseError}
    <span id="duration-parse-error" class="validation-message" role="alert"
      >Invalid format — try "2h", "30m", "1h 30m", or "90"</span
    >
  {:else if belowMin}
    <span id="duration-min-error" class="validation-message" role="alert"
      >Minimum duration is {formatDuration(min)}</span
    >
  {/if}
</div>

<style>
  .duration-input {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
  }

  .duration-label {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-text);
  }

  .duration-field-wrapper {
    position: relative;
    width: 160px;
  }

  .duration-field {
    width: 100%;
  }

  .duration-preview {
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  .validation-message {
    font-size: var(--font-size-xs);
    color: var(--color-error);
  }

  .duration-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 1100;
    width: 100%;
    min-width: 140px;
    margin: 0;
    padding: var(--spacing-1) 0;
    list-style: none;
    background: var(--color-surface);
    border: 1px solid var(--color-border-light);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    overflow-y: auto;
    max-height: 240px;
  }

  .duration-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-2);
    padding: 6px 10px;
    cursor: pointer;
    font-size: var(--font-size-sm);
    color: var(--color-text);
    user-select: none;
  }

  .duration-option:hover,
  .duration-option--highlighted {
    background: var(--color-surface-hover);
  }

  .duration-option--custom {
    border-bottom: 1px solid var(--color-border-light);
    margin-bottom: var(--spacing-1);
    padding-bottom: 8px;
    font-weight: var(--font-weight-medium);
  }

  .duration-option-check {
    color: var(--color-primary);
    font-size: var(--font-size-xs);
    flex-shrink: 0;
  }
</style>
