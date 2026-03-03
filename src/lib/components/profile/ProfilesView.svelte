<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import * as api from '../../api';
  import { toastState } from '../../stores/toast.svelte';
  import { profileState, ProfileState } from '../../stores/profile.svelte';
  import type { ProfileInfo } from '../../types';
  import CreateProfileForm from './CreateProfileForm.svelte';

  interface Props {
    store?: ProfileState;
    createProfile?: (name: string) => Promise<ProfileInfo>;
    renameProfile?: (id: string, name: string) => Promise<ProfileInfo>;
    deleteProfile?: (id: string) => Promise<void>;
  }

  const {
    store = profileState,
    createProfile = api.createProfile,
    renameProfile = api.renameProfile,
    deleteProfile = api.deleteProfile,
  }: Props = $props();

  // State (list state lives in profileState; the forms are local)

  let showCreate: boolean = $state(false);
  let newName: string = $state('');
  let createError: string | null = $state(null);
  let creating: boolean = $state(false);

  let renameTarget: ProfileInfo | null = $state(null);
  let renameName: string = $state('');
  let renameError: string | null = $state(null);
  let renaming: boolean = $state(false);

  let deleteTarget: ProfileInfo | null = $state(null);
  let deleteError: string | null = $state(null);
  let deleting: boolean = $state(false);

  /** Registry entry of the active profile (for the rename affordance). */
  const activeInfo: ProfileInfo | null = $derived.by(() => {
    const status = store.status;
    if (status === null || status.active === null) return null;
    const activeId = status.active.id;
    return status.profiles.find((p) => p.id === activeId) ?? null;
  });

  function handleCreateSubmit(event: SubmitEvent): void {
    event.preventDefault();
    if (creating) return;
    createError = null;
    const name = newName.trim();
    if (name.length === 0) {
      createError = 'Profile name is required.';
      return;
    }
    creating = true;
    createProfile(name)
      .then((profile) => {
        creating = false;
        newName = '';
        showCreate = false;
        toastState.success(`Profile "${profile.name}" created.`);
        void store.load();
      })
      .catch((e) => {
        creating = false;
        createError = api.apiErrorMessage(e, 'Could not create the profile.');
      });
  }

  function startRename(profile: ProfileInfo): void {
    renameTarget = profile;
    renameName = profile.name;
    renameError = null;
    deleteTarget = null;
  }

  function handleRenameSubmit(event: SubmitEvent): void {
    event.preventDefault();
    if (renaming || renameTarget === null) return;
    renameError = null;
    const name = renameName.trim();
    if (name.length === 0) {
      renameError = 'Profile name is required.';
      return;
    }
    renaming = true;
    renameProfile(renameTarget.id, name)
      .then((profile) => {
        renaming = false;
        renameTarget = null;
        toastState.success(`Profile renamed to "${profile.name}".`);
        void store.load();
      })
      .catch((e) => {
        renaming = false;
        renameError = api.apiErrorMessage(e, 'Could not rename the profile.');
      });
  }

  function startDelete(profile: ProfileInfo): void {
    deleteTarget = profile;
    deleteError = null;
    renameTarget = null;
  }

  function handleDeleteSubmit(event: SubmitEvent): void {
    event.preventDefault();
    if (deleting || deleteTarget === null) return;
    const target = deleteTarget;
    deleteError = null;
    deleting = true;
    deleteProfile(target.id)
      .then(() => {
        deleting = false;
        deleteTarget = null;
        toastState.success(`Profile "${target.name}" deleted.`);
        void store.load();
      })
      .catch((e) => {
        deleting = false;
        deleteError = api.apiErrorMessage(e, 'Could not delete the profile.');
      });
  }

  // Mount effect — load() traps its own errors into store.loadError.

  $effect(() => {
    void store.load();
  });
</script>

<section class="profiles-view">
  <h2>Profiles</h2>

  {#if store.loadError !== null}
    <p class="error-text" role="alert">{store.loadError}</p>
    <button class="btn-sm" onclick={() => void store.load()}>Retry</button>
  {:else if store.status === null}
    <p class="muted">Loading…</p>
  {:else}
    <div class="settings-card">
      <h3 class="card-title">Profiles</h3>

      <div class="profile-row">
        <p class="status-line">
          Current profile: <strong>{store.active?.name ?? 'Unknown'}</strong>
        </p>
        {#if activeInfo !== null}
          {@const active = activeInfo}
          <button class="btn-sm" onclick={() => startRename(active)}>Rename…</button>
        {/if}
      </div>

      <!-- Other profiles: rename / direct switch (no confirmation) -->
      {#if store.others.length > 0}
        <ul class="profile-list">
          {#each store.others as profile (profile.id)}
            <li class="profile-row">
              <span class="profile-name">{profile.name}</span>
              <div class="row-actions">
                <button class="btn-sm" onclick={() => startRename(profile)}>Rename…</button>
                <button
                  class="btn-sm"
                  onclick={() => void store.switchTo(profile.id)}
                  disabled={store.switching}
                >
                  Switch
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <!-- Rename form (one at a time, whichever Rename… was clicked) -->
      {#if renameTarget !== null}
        <form class="stack-form" onsubmit={handleRenameSubmit}>
          <h4 class="section-heading">Rename "{renameTarget.name}"</h4>
          <input
            class="text-input"
            type="text"
            placeholder="New name"
            aria-label="New profile name"
            bind:value={renameName}
            disabled={renaming}
          />
          {#if renameError}
            <p class="error-text" role="alert">{renameError}</p>
          {/if}
          <div class="button-row">
            <button
              type="button"
              class="btn-sm"
              onclick={() => {
                renameTarget = null;
                renameError = null;
              }}
              disabled={renaming}
            >
              Cancel
            </button>
            <button type="submit" class="btn-primary btn-sm" disabled={renaming}>
              {renaming ? 'Renaming…' : 'Rename'}
            </button>
          </div>
        </form>
      {/if}

      {#if showCreate}
        <CreateProfileForm
          bind:name={newName}
          busy={creating}
          error={createError}
          onSubmit={handleCreateSubmit}
          onCancel={() => {
            showCreate = false;
            createError = null;
          }}
          class="stack-form"
          smButtons={true}
        />
      {:else}
        <div class="button-row-start">
          <button class="btn-sm" onclick={() => (showCreate = true)}>Add profile</button>
        </div>
      {/if}
    </div>

    <!-- Destructive actions live apart from the everyday ones. -->
    <div class="settings-card danger-card">
      <h3 class="card-title">Danger zone</h3>

      {#if store.others.length === 0}
        <p class="muted">
          Nothing to delete — only the active profile exists, and the active profile cannot be
          deleted.
        </p>
      {:else}
        <p class="muted">
          Deleting a profile permanently removes it and all of its tasks. The active profile cannot
          be deleted.
        </p>
        <ul class="profile-list">
          {#each store.others as profile (profile.id)}
            <li class="profile-row">
              <span class="profile-name">{profile.name}</span>
              <button class="btn-danger btn-sm" onclick={() => startDelete(profile)}>
                Delete…
              </button>
            </li>
          {/each}
        </ul>

        <!-- Delete confirmation (inline, destructive) -->
        {#if deleteTarget !== null}
          <form class="stack-form" onsubmit={handleDeleteSubmit}>
            <h4 class="section-heading">Delete "{deleteTarget.name}"?</h4>
            <p class="muted">
              This permanently deletes the profile and all of its tasks. It cannot be undone.
            </p>
            {#if deleteError}
              <p class="error-text" role="alert">{deleteError}</p>
            {/if}
            <div class="button-row">
              <button
                type="button"
                class="btn-sm"
                onclick={() => {
                  deleteTarget = null;
                  deleteError = null;
                }}
                disabled={deleting}
              >
                Cancel
              </button>
              <button type="submit" class="btn-danger btn-sm" disabled={deleting}>
                {deleting ? 'Deleting…' : 'Delete profile'}
              </button>
            </div>
          </form>
        {/if}
      {/if}
    </div>
  {/if}
</section>

<style>
  .profiles-view {
    padding: var(--spacing-6);
    overflow-y: auto;
    height: 100%;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-6);
    align-items: flex-start;
  }

  .profiles-view h2 {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    margin: 0;
  }

  .settings-card {
    width: 100%;
  }

  .danger-card {
    border-color: var(--color-danger);
  }

  .danger-card .card-title {
    color: var(--color-danger);
  }

  .profile-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .profile-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-3);
  }

  .profile-name {
    font-size: var(--font-size-sm);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-actions {
    display: flex;
    gap: var(--spacing-2);
    flex-shrink: 0;
  }

  .stack-form {
    padding-top: var(--spacing-2);
    border-top: 1px solid var(--color-border-light);
  }

  .text-input {
    font-size: var(--font-size-sm);
  }

  .button-row-start {
    display: flex;
    justify-content: flex-start;
  }
</style>
