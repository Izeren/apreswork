<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { router } from '../../router.svelte';
  import Sidebar from './Sidebar.svelte';
  import TaskListView from '../tasks/TaskListView.svelte';
  import CalendarView from '../calendar/CalendarView.svelte';
  import StatusView from '../status/StatusView.svelte';
  import SettingsView from '../settings/SettingsView.svelte';
  import ProfilesView from '../profile/ProfilesView.svelte';
  import ShortcutOverlay from './ShortcutOverlay.svelte';
  import Modal from '../shared/Modal.svelte';
  import Toast from '../shared/Toast.svelte';
  import { defaultShellApi, type ShellApi } from './shellShared';
  import type { SettingsViewApi } from '../settings/settingsViewShared';
  import type { SchedulingSectionApi } from '../settings/schedulingSectionShared';
  import type { BackupSectionApi } from '../settings/backupSectionShared';
  import type { StatusViewApi } from '../status/statusViewShared';
  import type { TaskFormApi } from '../tasks/taskFormShared';
  import type { ProfileState } from '../../stores/profile.svelte';
  import { appClock } from '../../app-clock';
  import { formatDateTime } from '../../utils';
  import { toastState } from '../../stores/toast.svelte';
  import { warningState } from '../../stores/warnings.svelte';
  import { handleShortcutKeydown, registerShortcuts } from '../../shortcuts.svelte';

  interface Props {
    apiClient?: ShellApi;
    settingsApiClient?: SettingsViewApi;
    schedulingApiClient?: SchedulingSectionApi;
    backupApiClient?: BackupSectionApi;
    statusApiClient?: StatusViewApi;
    taskFormApiClient?: TaskFormApi;
    profileStore?: ProfileState;
  }

  const {
    apiClient = defaultShellApi,
    settingsApiClient,
    schedulingApiClient,
    backupApiClient,
    statusApiClient,
    taskFormApiClient,
    profileStore,
  }: Props = $props();

  let shortcutOverlayOpen = $state(false);

  let statusWarningsOpen = $state(false);

  // One-time notice when this app run restored the profile from its Drive
  // backup (backup-wins restore). Longer-lived than the default toast: the
  // user should notice their local data was replaced. Mount-only: no
  // tracked reactive reads — keep it that way.
  $effect(() => {
    apiClient
      .getBackupStatus()
      .then((s) => {
        if (s.restored_this_run !== null) {
          const when =
            s.restored_this_run === ''
              ? ''
              : ` (last change ${formatDateTime(s.restored_this_run)})`;
          toastState.push('info', `Restored this profile from its Drive backup${when}.`, 10_000);
        }
      })
      .catch(() => {
        toastState.push('error', 'Failed to load backup status.');
      });
  });

  $effect(() => {
    return registerShortcuts([
      {
        key: '1',
        description: 'Go to calendar',
        group: 'Global',
        handler: () => router.navigate('calendar'),
      },
      {
        key: '2',
        description: 'Go to tasks',
        group: 'Global',
        handler: () => router.navigate('tasks'),
      },
      {
        key: '3',
        description: 'Go to settings',
        group: 'Global',
        handler: () => router.navigate('settings'),
      },
      {
        key: '4',
        description: 'Go to status',
        group: 'Global',
        handler: () => router.navigate('status'),
      },
      {
        key: '?',
        description: 'Show this help',
        group: 'Global',
        handler: () => {
          shortcutOverlayOpen = true;
        },
      },
    ]);
  });
</script>

<svelte:window onkeydown={handleShortcutKeydown} />

<div class="shell">
  <Sidebar
    warningCount={warningState.count}
    warningBlocking={warningState.hasBlocking}
    onwarningsclick={() => (statusWarningsOpen = true)}
  />

  <div class="main-column">
    <main class="content">
      {#if router.current === 'calendar'}
        <CalendarView getNow={appClock} />
      {:else if router.current === 'tasks'}
        <TaskListView getNow={appClock} />
      {:else if router.current === 'status'}
        <StatusView apiClient={statusApiClient} {taskFormApiClient} />
      {:else if router.current === 'settings'}
        <SettingsView apiClient={settingsApiClient} {schedulingApiClient} {backupApiClient} />
      {:else if router.current === 'profiles'}
        <ProfilesView store={profileStore} />
      {/if}
    </main>
  </div>
</div>

<Modal
  open={statusWarningsOpen}
  title="Scheduling status"
  size="lg"
  movable={true}
  resizable={true}
  onclose={() => (statusWarningsOpen = false)}
>
  <StatusView embedded={true} apiClient={statusApiClient} {taskFormApiClient} />
</Modal>

<ShortcutOverlay open={shortcutOverlayOpen} onclose={() => (shortcutOverlayOpen = false)} />
<Toast />

<style>
  .shell {
    display: flex;
    height: 100vh;
    width: 100%;
    overflow: hidden;
  }

  .content {
    flex: 1;
    background: var(--color-bg);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .main-column {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
