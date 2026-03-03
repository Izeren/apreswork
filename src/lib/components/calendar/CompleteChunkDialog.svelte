<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import Modal from '../shared/Modal.svelte';

  export type CompletionTarget = 'chunk' | 'task';
  interface Props {
    open: boolean;
    taskTitle: string;
    selectedTarget: CompletionTarget;
    busy?: boolean;
    onselecttarget: (target: CompletionTarget) => void;
    onconfirm: () => void;
    onclose: () => void;
  }

  const {
    open,
    taskTitle,
    selectedTarget,
    busy = false,
    onselecttarget,
    onconfirm,
    onclose,
  }: Props = $props();
</script>

<Modal {open} title="Complete Work" {onclose}>
  <div class="dialog-body">
    <p class="dialog-copy">Choose what to mark complete for <strong>{taskTitle}</strong>.</p>

    <div class="option-list" role="radiogroup" aria-label="Completion target">
      <label class="option-card">
        <input
          type="radio"
          name="completion-target"
          value="chunk"
          checked={selectedTarget === 'chunk'}
          onchange={() => onselecttarget('chunk')}
        />
        <div class="option-copy">
          <span class="option-title">This chunk</span>
          <span class="option-description">Mark only the selected calendar block complete.</span>
        </div>
      </label>

      <label class="option-card">
        <input
          type="radio"
          name="completion-target"
          value="task"
          checked={selectedTarget === 'task'}
          onchange={() => onselecttarget('task')}
        />
        <div class="option-copy">
          <span class="option-title">Whole task</span>
          <span class="option-description"
            >Complete all remaining scheduled chunks for this task.</span
          >
        </div>
      </label>
    </div>

    <div class="dialog-actions">
      <button class="btn-cancel" onclick={onclose} disabled={busy}>Cancel</button>
      <button class="btn-primary" onclick={onconfirm} disabled={busy}>
        {busy ? 'Completing…' : 'Complete'}
      </button>
    </div>
  </div>
</Modal>

<style>
  .dialog-body {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-5);
  }

  .dialog-copy {
    margin: 0;
    color: var(--color-text);
    line-height: var(--line-height);
  }

  .option-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-3);
  }

  .option-card {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    cursor: pointer;
  }

  .option-card:has(input:checked) {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, var(--color-bg-secondary));
  }

  .option-card input {
    margin-top: 2px;
  }

  .option-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .option-title {
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .option-description {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    line-height: var(--line-height);
  }
</style>
