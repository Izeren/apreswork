<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { AuthStatus, ExternalCalendar, SyncStatus } from '../../types';
  import { formatDateTime } from '../../utils';
  import { toastState } from '../../stores/toast.svelte';
  import { runSync } from '../../actions/syncTrigger';
  import ConfirmDialog from '../shared/ConfirmDialog.svelte';
  import SchedulingSection from './SchedulingSection.svelte';
  import BackupSection from './BackupSection.svelte';
  import { defaultSettingsViewApi, type SettingsViewApi } from './settingsViewShared';
  import type { SchedulingSectionApi } from './schedulingSectionShared';
  import type { BackupSectionApi } from './backupSectionShared';

  interface Props {
    apiClient?: SettingsViewApi;
    schedulingApiClient?: SchedulingSectionApi;
    backupApiClient?: BackupSectionApi;
  }

  const {
    apiClient = defaultSettingsViewApi,
    schedulingApiClient,
    backupApiClient,
  }: Props = $props();

  let status: AuthStatus | null = $state(null);
  let connecting: boolean = $state(false);
  let pollTimer: ReturnType<typeof setInterval> | null = $state(null);
  let pollTicks: number = $state(0);

  let calendars: ExternalCalendar[] | null = $state(null);
  let calendarsError: string | null = $state(null);
  let selectedIds: string[] = $state([]);
  let savingSelection: boolean = $state(false);

  let syncing: boolean = $state(false);
  let syncStatus: SyncStatus | null = $state(null);
  let confirmDisconnect: boolean = $state(false);
  let disconnecting: boolean = $state(false);

  const sortedCalendars: ExternalCalendar[] = $derived.by(() => {
    if (!calendars) return [];
    return [...calendars].sort((a, b) => {
      if (a.primary !== b.primary) return a.primary ? -1 : 1;
      return a.title.localeCompare(b.title);
    });
  });

  function stopPolling(): void {
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
      pollTicks = 0;
    }
  }

  function startPolling(): void {
    stopPolling();
    pollTicks = 0;
    pollTimer = setInterval(() => {
      pollTicks += 1;
      if (pollTicks > 150) {
        // ~5 minutes: stop silently
        stopPolling();
        return;
      }
      apiClient
        .googleAuthStatus()
        .then((s) => {
          status = s;
          if (s.type === 'connected') {
            stopPolling();
            toastState.success('Google Calendar connected.');
            loadPicker();
            loadSyncStatus();
          }
        })
        .catch(() => {
          // Read-only status check — errors are intentionally silent.
        });
    }, 2000);
  }

  function loadPicker(): void {
    calendars = null;
    calendarsError = null;
    Promise.all([apiClient.googleListCalendars(), apiClient.getPullCalendars()])
      .then(([cals, ids]) => {
        calendars = cals;
        selectedIds = ids;
      })
      .catch((e) => {
        calendarsError = apiClient.syncErrorMessage(e, 'Could not load calendars.');
      });
  }

  function loadSyncStatus(): void {
    apiClient
      .getSyncStatus()
      .then((s) => {
        syncStatus = s;
      })
      .catch(() => {
        // Display-only bookkeeping — errors are intentionally silent.
      });
  }

  function onSyncError(e: unknown, fallback: string, endBusy: () => void): void {
    toastState.error(apiClient.syncErrorMessage(e, fallback));
    endBusy();
  }

  function handleConnect(): void {
    stopPolling();
    connecting = true;
    apiClient
      .beginGoogleAuth()
      .then((url) => {
        apiClient.openExternalUrl(url).catch(() => {
          // The consent flow is still live backend-side; polling continues
          // so a manually opened browser can complete it.
          toastState.error('Could not open the browser for Google sign-in.');
        });
        status = { type: 'pending' };
        connecting = false;
        startPolling();
      })
      .catch((e) => {
        onSyncError(e, 'Could not start Google sign-in.', () => {
          connecting = false;
        });
      });
  }

  function handleDisconnectConfirm(): void {
    disconnecting = true;
    apiClient
      .googleDisconnect()
      .then(() => {
        confirmDisconnect = false;
        disconnecting = false;
        calendars = null;
        selectedIds = [];
        calendarsError = null;
        status = { type: 'not_connected' };
        toastState.success('Google Calendar disconnected.');
      })
      .catch((e) => {
        onSyncError(e, 'Could not disconnect.', () => {
          confirmDisconnect = false;
          disconnecting = false;
        });
      });
  }

  function handleDisconnectCancel(): void {
    confirmDisconnect = false;
  }

  function handleCheckboxToggle(id: string, checked: boolean): void {
    const next = checked ? [...selectedIds, id] : selectedIds.filter((x) => x !== id);
    selectedIds = next;
    savingSelection = true;
    apiClient
      .setPullCalendars(next)
      .then(() => {
        savingSelection = false;
      })
      .catch((e) => {
        onSyncError(e, 'Could not save calendar selection.', () => {
          savingSelection = false;
          apiClient
            .getPullCalendars()
            .then((ids) => {
              selectedIds = ids;
            })
            .catch(() => {
              // Refetch failed after a toasted save error; keep the optimistic value.
            });
        });
      });
  }

  function handleSyncNow(): void {
    // A failed sync records an error server-side too — refresh on both paths
    // so the status banner reflects it either way.
    runSync((busy) => (syncing = busy), loadSyncStatus, apiClient, loadSyncStatus);
  }

  $effect(() => {
    apiClient
      .googleAuthStatus()
      .then((s) => {
        status = s;
        if (s.type === 'connected') {
          loadPicker();
          loadSyncStatus();
        } else if (s.type === 'pending') {
          startPolling();
        }
      })
      .catch((e) => {
        status = { type: 'not_connected' };
        toastState.error(apiClient.syncErrorMessage(e, 'Could not load Google Calendar status.'));
      });

    return () => {
      stopPolling();
    };
  });
</script>

<section class="settings-view">
  <h2>Settings</h2>

  <div class="settings-card">
    <h3 class="card-title">Google Calendar</h3>

    <p class="status-line">
      {#if status === null}
        Loading…
      {:else if status.type === 'not_connected'}
        Not connected
      {:else if status.type === 'pending'}
        Waiting for you to finish signing in in the browser…
      {:else if status.type === 'connected'}
        {status.email ? `Connected as ${status.email}` : 'Connected'}
      {/if}
    </p>

    <div class="button-row">
      {#if status?.type !== 'connected'}
        <button class="btn-primary" onclick={handleConnect} disabled={connecting}>
          {connecting ? 'Connecting…' : 'Connect Google Calendar'}
        </button>
      {:else}
        <button onclick={handleConnect} disabled={connecting}>
          {connecting ? 'Connecting…' : 'Reconnect'}
        </button>
        <button
          class="btn-danger"
          onclick={() => (confirmDisconnect = true)}
          disabled={disconnecting}
        >
          Disconnect…
        </button>
      {/if}
    </div>

    {#if status?.type === 'connected'}
      <!-- Reconnect-needed banner: a recorded sync failure usually means the
           Google sign-in expired (Testing-status refresh tokens die weekly). -->
      {#if syncStatus?.last_sync_error}
        <div class="reconnect-banner" role="alert">
          <p class="error-text">{syncStatus.last_sync_error}</p>
          <p class="muted">The last sync failed — your Google sign-in may have expired.</p>
          <button class="btn-primary btn-sm" onclick={handleConnect} disabled={connecting}>
            {connecting ? 'Connecting…' : 'Reconnect now'}
          </button>
        </div>
      {/if}

      <div class="picker-section">
        <h4 class="picker-heading">Calendars to import</h4>

        {#if calendarsError}
          <p class="error-text">{calendarsError}</p>
          <button class="btn-sm" onclick={loadPicker}>Retry</button>
        {:else if calendars === null}
          <p class="muted">Loading calendars…</p>
        {:else}
          <ul class="calendar-list">
            {#each sortedCalendars as cal (cal.id)}
              <li>
                <label class="calendar-label">
                  <input
                    type="checkbox"
                    checked={selectedIds.includes(cal.id)}
                    disabled={savingSelection}
                    onchange={(e) => handleCheckboxToggle(cal.id, e.currentTarget.checked)}
                  />
                  {cal.title}{cal.primary ? ' (primary)' : ''}
                </label>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="sync-row">
        <p class="muted">Import events from the selected calendars, reschedule, and push.</p>
        <button onclick={handleSyncNow} disabled={syncing}>
          {syncing ? 'Syncing…' : 'Sync now'}
        </button>
        {#if syncStatus !== null}
          <p class="muted">
            {syncStatus.last_sync_at
              ? `Last synced: ${formatDateTime(syncStatus.last_sync_at)}`
              : 'Not synced yet.'}
          </p>
        {/if}
      </div>
    {/if}
  </div>

  <SchedulingSection apiClient={schedulingApiClient} />

  <BackupSection apiClient={backupApiClient} />
</section>

<ConfirmDialog
  open={confirmDisconnect}
  title="Disconnect Google Calendar?"
  message="This removes the stored sign-in and imported events from this app. Your Google account and calendars are not changed."
  confirmLabel="Disconnect"
  destructive={true}
  onconfirm={handleDisconnectConfirm}
  oncancel={handleDisconnectCancel}
/>

<style>
  .settings-view {
    padding: var(--spacing-6);
    overflow-y: auto;
    height: 100%;
    box-sizing: border-box;
  }

  .settings-view h2 {
    font-size: var(--font-size-xl);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    margin: 0 0 var(--spacing-6) 0;
  }

  .button-row {
    display: flex;
    gap: var(--spacing-3);
    flex-wrap: wrap;
  }

  /* .btn-primary / .btn-danger / .btn-sm come from the app.css globals; secondary buttons
     use the plain base `button` style (no class). */

  .picker-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-3);
  }

  .calendar-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .calendar-label {
    display: flex;
  }

  .calendar-label input[type='checkbox'] {
    cursor: pointer;
  }

  .sync-row {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
    padding-top: var(--spacing-2);
    border-top: 1px solid var(--color-border);
  }

  .reconnect-banner {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--spacing-2);
    padding: var(--spacing-3);
    border: 1px solid var(--color-danger, #dc2626);
    border-radius: var(--radius-md);
  }
</style>
