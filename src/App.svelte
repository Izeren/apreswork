<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import Shell from './lib/components/layout/Shell.svelte';
  import ProfileGate from './lib/components/profile/ProfileGate.svelte';
  import { profileState } from './lib/stores/profile.svelte';

  // The Shell (and everything behind it) mounts only while a profile is
  // active — the backend auto-activates the last-used profile at startup,
  // so the gate is a fallback (activation failure or empty registry) where
  // the user unlocks or creates one. The {#key} block remounts the Shell on
  // an in-place profile switch so every view refetches for the new profile.
  // load() traps its own errors into `loadError`.
  $effect(() => {
    void profileState.load();
  });
</script>

{#if profileState.active !== null}
  {#key profileState.active.id}
    <Shell />
  {/key}
{:else if profileState.status !== null}
  <ProfileGate status={profileState.status} onUnlocked={(p) => profileState.setActive(p)} />
{:else if profileState.loadError !== null}
  <div class="gate-error" role="alert">
    <p>{profileState.loadError}</p>
    <button onclick={() => void profileState.load()}>Retry</button>
  </div>
{:else}
  <!-- Usually sub-millisecond (local IPC), but visible on first-run migration. -->
  <div class="gate-loading">
    <p>Loading…</p>
  </div>
{/if}

<style>
  .gate-loading {
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg);
    color: var(--color-text-secondary);
  }

  .gate-error {
    height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-4);
    background: var(--color-bg);
    color: var(--color-text);
  }

  .gate-error button {
    background: var(--color-primary);
    color: var(--color-text-inverse);
    border: none;
    border-radius: var(--radius-md);
    padding: var(--spacing-2) var(--spacing-4);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
  }
</style>
