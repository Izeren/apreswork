<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { BackupStatus } from '../../types';
  import { formatDateTime } from '../../utils';
  import { toastState } from '../../stores/toast.svelte';
  import ConfirmDialog from '../shared/ConfirmDialog.svelte';
  import {
    defaultBackupSectionApi,
    defaultBackupSectionDialog,
    type BackupSectionApi,
    type BackupSectionDialog,
  } from './backupSectionShared';

  let {
    apiClient = defaultBackupSectionApi,
    dialogClient = defaultBackupSectionDialog,
  }: { apiClient?: BackupSectionApi; dialogClient?: BackupSectionDialog } = $props();

  let status: BackupStatus | null = $state(null);
  let loadError: string | null = $state(null);

  let toggling: boolean = $state(false);
  let backingUp: boolean = $state(false);
  let exporting: boolean = $state(false);
  let importing: boolean = $state(false);

  /** File picked for import, awaiting the replace-all-data confirmation. */
  let confirmImportPath: string | null = $state(null);

  function load(): void {
    apiClient
      .getBackupStatus()
      .then((s) => {
        status = s;
        loadError = null;
      })
      .catch((e) => {
        loadError = apiClient.apiErrorMessage(e, 'Could not load backup status.');
      });
  }

  function handleToggle(): void {
    if (status === null) return;
    toggling = true;
    apiClient
      .setBackupEnabled(!status.enabled)
      .then((s) => {
        status = s;
        toggling = false;
      })
      .catch((e) => {
        toggling = false;
        toastState.error(apiClient.backupErrorMessage(e, 'Could not update the backup setting.'));
        // Defensive refetch — the controlled checkbox never moved, but the
        // backend may have recorded a side effect before failing.
        load();
      });
  }

  function handleBackupNow(): void {
    backingUp = true;
    apiClient
      .backupNow()
      .then((s) => {
        status = s;
        backingUp = false;
        if (s.last_backup_error) {
          // The stale-writer guard skipped the upload; the banner explains.
          toastState.error(s.last_backup_error);
        } else {
          toastState.success('Backed up to Google Drive.');
        }
      })
      .catch((e) => {
        backingUp = false;
        toastState.error(apiClient.backupErrorMessage(e, 'Backup failed.'));
        // A failed export records an error server-side — refresh the banner.
        load();
      });
  }

  function handleExport(): void {
    dialogClient
      .save({
        defaultPath: 'apreswork-backup.zip',
        filters: [{ name: 'Zip archive', extensions: ['zip'] }],
      })
      .then((path) => {
        if (path === null) return;
        exporting = true;
        apiClient
          .exportBackupToFile(path)
          .then(() => {
            exporting = false;
            toastState.success('Backup exported.');
          })
          .catch((e) => {
            exporting = false;
            toastState.error(apiClient.backupErrorMessage(e, 'Could not export the backup file.'));
          });
      })
      .catch(() => {
        toastState.error('Could not open the save dialog.');
      });
  }

  function handleImportPick(): void {
    dialogClient
      .open({
        multiple: false,
        filters: [{ name: 'Zip archive', extensions: ['zip'] }],
      })
      .then((path) => {
        if (typeof path === 'string') {
          confirmImportPath = path;
        }
      })
      .catch(() => {
        toastState.error('Could not open the file dialog.');
      });
  }

  function handleImportConfirm(): void {
    if (confirmImportPath === null) return;
    const path = confirmImportPath;
    confirmImportPath = null;
    importing = true;
    apiClient.importBackupFromFile(path).catch((e) => {
      importing = false;
      toastState.error(apiClient.backupErrorMessage(e, 'Could not import the backup file.'));
    });
  }

  // Mount-only: load() has no tracked reactive reads — keep it that way.
  $effect(() => {
    load();
  });
</script>

<div class="settings-card backup-card">
  <h3 class="card-title">Backup</h3>

  {#if loadError}
    <p class="error-text" role="alert">{loadError}</p>
    <button type="button" class="btn-sm" onclick={load}>Retry</button>
  {:else if !status}
    <p class="muted">Loading…</p>
  {:else}
    <label class="toggle-label">
      <!-- Fully controlled: preventDefault keeps the DOM in sync with `status`
           even when the call fails (Svelte skips writes for unchanged values,
           so a user-flipped box would otherwise stick after a revert). -->
      <input
        type="checkbox"
        checked={status.enabled}
        disabled={toggling || !status.connected}
        onclick={(e) => {
          e.preventDefault();
          handleToggle();
        }}
      />
      Back up this profile to Google Drive automatically
    </label>
    {#if !status.connected}
      <p class="muted">Connect Google Calendar above to enable Drive backup.</p>
    {/if}

    <p class="status-line">
      {status.last_export_at
        ? `Last backed up: ${formatDateTime(status.last_export_at)}`
        : 'Not backed up yet.'}
    </p>

    {#if status.last_backup_error}
      <p class="error-text" role="alert">{status.last_backup_error}</p>
    {/if}

    <div class="button-row-start">
      <button class="btn-sm" onclick={handleBackupNow} disabled={backingUp || !status.connected}>
        {backingUp ? 'Backing up…' : 'Back up now'}
      </button>
      <button class="btn-sm" onclick={handleExport} disabled={exporting}>
        {exporting ? 'Exporting…' : 'Export to file…'}
      </button>
      <button class="btn-sm" onclick={handleImportPick} disabled={importing}>
        {importing ? 'Importing…' : 'Import from file…'}
      </button>
    </div>
  {/if}
</div>

<!-- Import confirmation dialog (destructive: replaces the profile's data) -->
<ConfirmDialog
  open={confirmImportPath !== null}
  title="Import backup?"
  message="This replaces all data in this profile with the backup and restarts the app. The replaced data is kept as a one-generation safety copy."
  confirmLabel="Import and restart"
  destructive={true}
  onconfirm={handleImportConfirm}
  oncancel={() => (confirmImportPath = null)}
/>

<style>
  /* Sits below the Google Calendar card inside .settings-view. */
  .backup-card {
    margin-top: var(--spacing-6);
  }

  .toggle-label {
    display: flex;
  }

  .toggle-label input[type='checkbox'] {
    cursor: pointer;
  }
</style>
