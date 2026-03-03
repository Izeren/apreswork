<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import * as api from '../../api';
  import type { ActiveProfile, ProfileInfo, ProfileStatus } from '../../types';
  import CreateProfileForm from './CreateProfileForm.svelte';

  interface Props {
    status: ProfileStatus;
    onUnlocked: (profile: ActiveProfile) => void;
    unlockProfile?: (id: string) => Promise<ActiveProfile>;
    createProfile?: (name: string) => Promise<ProfileInfo>;
  }

  const {
    status,
    onUnlocked,
    unlockProfile = api.unlockProfile,
    createProfile = api.createProfile,
  }: Props = $props();

  let busy: boolean = $state(false);
  let error: string | null = $state(null);

  let showCreate: boolean = $state(false);
  let newName: string = $state('');
  let createError: string | null = $state(null);

  /** Profiles created from this gate (the status prop is a mount-time snapshot). */
  let createdProfiles: ProfileInfo[] = $state([]);

  /** All profiles, last-used first, then oldest-first. */
  const sortedProfiles: ProfileInfo[] = $derived.by(() => {
    const all = [...status.profiles, ...createdProfiles];
    return all.sort((a, b) => {
      if (a.id === status.last_used) return -1;
      if (b.id === status.last_used) return 1;
      return a.created_at.localeCompare(b.created_at);
    });
  });

  function unlock(id: string): void {
    busy = true;
    error = null;
    unlockProfile(id)
      .then((active) => {
        busy = false;
        onUnlocked(active);
      })
      .catch((e) => {
        busy = false;
        error = api.apiErrorMessage(e, 'Could not unlock the profile.');
      });
  }

  function handleSelect(profile: ProfileInfo): void {
    if (busy) return;
    unlock(profile.id);
  }

  function handleCreateSubmit(event: SubmitEvent): void {
    event.preventDefault();
    if (busy) return;
    createError = null;
    const name = newName.trim();
    if (name.length === 0) {
      createError = 'Profile name is required.';
      return;
    }
    busy = true;
    createProfile(name)
      .then((profile) => {
        createdProfiles = [...createdProfiles, profile];
        newName = '';
        showCreate = false;
        // Enter the new profile directly — the gate exists to pick one.
        // An unlock failure lands on the picker, so surface it there, not in
        // the (now closed) create form.
        unlock(profile.id);
      })
      .catch((e) => {
        busy = false;
        createError = api.apiErrorMessage(e, 'Could not create the profile.');
      });
  }
</script>

<div class="gate">
  <div class="gate-card">
    <h1 class="gate-title">Après Work</h1>

    <p class="gate-subtitle">Who's using the app?</p>
    {#if error}
      <p class="error-text" role="alert">{error}</p>
    {/if}
    <ul class="profile-list">
      {#each sortedProfiles as profile (profile.id)}
        <li>
          <button class="profile-button" onclick={() => handleSelect(profile)} disabled={busy}>
            <span class="profile-name">{profile.name}</span>
          </button>
        </li>
      {/each}
    </ul>

    {#if showCreate}
      <CreateProfileForm
        bind:name={newName}
        {busy}
        error={createError}
        onSubmit={handleCreateSubmit}
        onCancel={() => {
          showCreate = false;
          createError = null;
        }}
        class="create-form"
        autofocus={true}
      />
    {:else}
      <button class="add-button" onclick={() => (showCreate = true)} disabled={busy}>
        Add profile
      </button>
    {/if}
  </div>
</div>

<style>
  .gate {
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg);
  }

  .gate-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--spacing-6);
    width: 320px;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .gate-title {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    margin: 0;
    text-align: center;
  }

  .gate-subtitle {
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    margin: 0;
    text-align: center;
  }

  .profile-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .profile-button {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-2);
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--spacing-3) var(--spacing-4);
    font-size: var(--font-size-base);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .profile-button:hover {
    background: var(--color-surface-hover);
  }

  .profile-button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .profile-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .add-button {
    align-self: center;
  }

  .error-text {
    text-align: center;
  }
</style>
