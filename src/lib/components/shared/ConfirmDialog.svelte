<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import Modal from './Modal.svelte';

  interface Props {
    open: boolean;
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    destructive?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  const {
    open,
    title,
    message,
    confirmLabel = 'Confirm',
    cancelLabel = 'Cancel',
    destructive = false,
    onconfirm,
    oncancel,
  }: Props = $props();
</script>

<Modal {open} {title} role="alertdialog" onclose={oncancel}>
  <div class="confirm-body">
    <p class="confirm-message">{message}</p>
    <div class="confirm-actions">
      <button class="btn-cancel" onclick={oncancel}>{cancelLabel}</button>
      <button class:btn-primary={!destructive} class:btn-danger={destructive} onclick={onconfirm}>
        {confirmLabel}
      </button>
    </div>
  </div>
</Modal>

<style>
  .confirm-body {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-6);
  }

  .confirm-message {
    font-size: var(--font-size-base);
    color: var(--color-text);
    line-height: var(--line-height);
  }
</style>
