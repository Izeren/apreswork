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
  import {
    defaultSettingsViewApi,
    GOOGLE_CLIENT_ID_SUFFIX,
    POLL_INTERVAL_MS,
    POLL_MAX_TICKS,
    type SettingsViewApi,
  } from './settingsViewShared';
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

  let credentialsSaved: boolean | null = $state(null);
  let showCredForm: boolean = $state(false);
  let credClientId: string = $state('');
  let credClientSecret: string = $state('');
  let credIdError: string | null = $state(null);
  let credSecretError: string | null = $state(null);
  let savingCreds: boolean = $state(false);

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

  function validateCredForm(): boolean {
    credIdError = null;
    credSecretError = null;
    if (!credClientId.trim()) {
      credIdError = 'Client ID is required.';
      return false;
    }
    if (!credClientId.trim().endsWith(GOOGLE_CLIENT_ID_SUFFIX)) {
      credIdError = `Client ID must end with "${GOOGLE_CLIENT_ID_SUFFIX}".`;
      return false;
    }
    if (!credClientSecret.trim()) {
      credSecretError = 'Client Secret is required.';
      return false;
    }
    return true;
  }

  function handleSaveCreds(e: SubmitEvent): void {
    e.preventDefault();
    if (!validateCredForm()) return;
    withBusy(
      (v) => (savingCreds = v),
      () => apiClient.saveGoogleClientCredentials(credClientId, credClientSecret),
      () => {
        showCredForm = false;
        credentialsSaved = true;
        credClientId = '';
        credClientSecret = '';
        credIdError = null;
        credSecretError = null;
        syncStatus = null;
        calendarsError = null;
        if (status?.type === 'connected') loadPicker();
        toastState.success('OAuth credentials saved.');
      },
      'Could not save credentials.',
    );
  }

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
      if (pollTicks > POLL_MAX_TICKS) {
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
    }, POLL_INTERVAL_MS);
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
        // see startPolling
      });
  }

  function onSyncError(e: unknown, fallback: string, endBusy: () => void): void {
    toastState.error(apiClient.syncErrorMessage(e, fallback));
    endBusy();
  }

  function withBusy<T>(
    setBusy: (v: boolean) => void,
    call: () => Promise<T>,
    onSuccess: (r: T) => void,
    errorMsg: string,
    onErrorExtra?: () => void,
  ): void {
    setBusy(true);
    call()
      .then((r) => {
        setBusy(false);
        onSuccess(r);
      })
      .catch((err: unknown) => {
        onSyncError(err, errorMsg, () => {
          setBusy(false);
          onErrorExtra?.();
        });
      });
  }

  function handleConnect(): void {
    stopPolling();
    withBusy(
      (v) => (connecting = v),
      () => apiClient.beginGoogleAuth(),
      (url) => {
        apiClient.openExternalUrl(url).catch(() => {
          // The consent flow is still live backend-side; polling continues
          // so a manually opened browser can complete it.
          toastState.error('Could not open the browser for Google sign-in.');
        });
        status = { type: 'pending' };
        startPolling();
      },
      'Could not start Google sign-in.',
    );
  }

  function handleDisconnectConfirm(): void {
    withBusy(
      (v) => (disconnecting = v),
      () => apiClient.googleDisconnect(),
      () => {
        confirmDisconnect = false;
        calendars = null;
        selectedIds = [];
        calendarsError = null;
        status = { type: 'not_connected' };
        toastState.success('Google Calendar disconnected.');
      },
      'Could not disconnect.',
      () => {
        confirmDisconnect = false;
      },
    );
  }

  function handleDisconnectCancel(): void {
    confirmDisconnect = false;
  }

  function handleCheckboxToggle(id: string, checked: boolean): void {
    const next = checked ? [...selectedIds, id] : selectedIds.filter((x) => x !== id);
    selectedIds = next;
    withBusy(
      (v) => (savingSelection = v),
      () => apiClient.setPullCalendars(next),
      () => {},
      'Could not save calendar selection.',
      () => {
        apiClient
          .getPullCalendars()
          .then((ids) => {
            selectedIds = ids;
          })
          .catch(() => {
            // Refetch failed after a toasted save error; keep the optimistic value.
          });
      },
    );
  }

  function handleSyncNow(): void {
    // A failed sync records an error server-side too — refresh on both paths
    // so the status banner reflects it either way.
    runSync((busy) => (syncing = busy), loadSyncStatus, apiClient, loadSyncStatus);
  }

  $effect(() => {
    apiClient
      .googleClientCredentialsSaved()
      .then((saved) => {
        credentialsSaved = saved;
        showCredForm = !saved;
      })
      .catch(() => {
        credentialsSaved = false;
        showCredForm = true;
      });
  });

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

    {#if credentialsSaved === true && !showCredForm}
      <p class="cred-status">
        OAuth app credentials configured.
        <button class="btn-link" onclick={() => (showCredForm = true)}>Change</button>
      </p>
    {/if}

    {#if showCredForm}
      <form class="cred-form" onsubmit={handleSaveCreds}>
        <div class="cred-field">
          <label class="cred-label" for="cred-client-id">Client ID</label>
          <input
            id="cred-client-id"
            class="cred-input"
            type="text"
            autocomplete="off"
            bind:value={credClientId}
            disabled={savingCreds}
            oninput={() => (credIdError = null)}
          />
          {#if credIdError}
            <span class="field-error" role="alert">{credIdError}</span>
          {/if}
        </div>
        <div class="cred-field">
          <label class="cred-label" for="cred-client-secret">Client Secret</label>
          <input
            id="cred-client-secret"
            class="cred-input"
            type="password"
            autocomplete="off"
            bind:value={credClientSecret}
            disabled={savingCreds}
            oninput={() => (credSecretError = null)}
          />
          {#if credSecretError}
            <span class="field-error" role="alert">{credSecretError}</span>
          {/if}
        </div>
        <button class="btn-primary btn-sm" type="submit" disabled={savingCreds}>
          {savingCreds ? 'Saving…' : 'Save'}
        </button>
        {#if credentialsSaved === true}
          <button type="button" class="btn-sm" onclick={() => (showCredForm = false)}>
            Cancel
          </button>
        {/if}
      </form>
    {/if}

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
      <!-- Testing-status refresh tokens die weekly -->
      {#if syncStatus?.last_sync_error && !(showCredForm && credentialsSaved === true)}
        <div class="reconnect-banner" role="alert">
          <p class="error-text">{syncStatus.last_sync_error}</p>
          <p class="muted">
            The last sync failed. Your Google sign-in may have expired, or your OAuth app
            credentials may be invalid.
          </p>
          <div class="button-row">
            <button class="btn-primary btn-sm" onclick={handleConnect} disabled={connecting}>
              {connecting ? 'Connecting…' : 'Reconnect now'}
            </button>
            {#if credentialsSaved === true}
              <button type="button" class="btn-sm" onclick={() => (showCredForm = true)}>
                Change credentials
              </button>
            {/if}
          </div>
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
    --color-primary-fallback: #6366f1;
    --color-danger-fallback: #dc2626;
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

  .cred-status {
    font-size: var(--font-size-sm);
    color: var(--color-text-muted);
    margin: 0 0 var(--spacing-3) 0;
  }

  .cred-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-3);
    margin-bottom: var(--spacing-4);
  }

  .cred-field {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
  }

  .cred-label {
    font-size: var(--font-size-sm);
    color: var(--color-text);
  }

  .cred-input {
    padding: var(--spacing-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    color: var(--color-text);
    font-size: var(--font-size-sm);
  }

  .field-error {
    font-size: var(--font-size-sm);
    color: var(--color-danger, var(--color-danger-fallback));
  }

  .btn-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--color-primary, var(--color-primary-fallback));
    cursor: pointer;
    font-size: inherit;
    text-decoration: underline;
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
    border: 1px solid var(--color-danger, var(--color-danger-fallback));
    border-radius: var(--radius-md);
  }
</style>
