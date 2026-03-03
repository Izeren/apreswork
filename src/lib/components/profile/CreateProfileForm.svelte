<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  interface Props {
    name: string;
    busy: boolean;
    error: string | null;
    onSubmit: (e: SubmitEvent) => void;
    onCancel: () => void;
    class?: string;
    smButtons?: boolean;
    autofocus?: boolean;
  }

  let {
    name = $bindable(''),
    busy,
    error,
    onSubmit,
    onCancel,
    class: formClass = '',
    smButtons = false,
    autofocus = false,
  }: Props = $props();

  function mountFocus(node: HTMLElement): void {
    if (autofocus) node.focus();
  }
</script>

<form class={formClass} onsubmit={onSubmit}>
  <input
    class="text-input"
    type="text"
    placeholder="Profile name"
    aria-label="Profile name"
    bind:value={name}
    disabled={busy}
    use:mountFocus
  />
  {#if error}
    <p class="error-text" role="alert">{error}</p>
  {/if}
  <div class="button-row">
    <button type="button" class:btn-sm={smButtons} onclick={onCancel} disabled={busy}>
      Cancel
    </button>
    <button type="submit" class="btn-primary" class:btn-sm={smButtons} disabled={busy}>
      {busy ? 'Creating…' : 'Create profile'}
    </button>
  </div>
</form>

<style>
  .text-input {
    font-size: var(--font-size-sm);
  }
</style>
