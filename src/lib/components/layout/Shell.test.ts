// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import Shell from './Shell.svelte';
import type { BackupStatus, ScheduleResult, ScheduleWarning, Task } from '../../types';
import { warningState } from '../../stores/warnings.svelte';
import { taskState } from '../../stores/tasks.svelte';
import { toastState } from '../../stores/toast.svelte';
import { profileState } from '../../stores/profile.svelte';
import { resetShortcutsForTest } from '../../shortcuts.svelte';
import { apiErrorMessage, backupErrorMessage, syncErrorMessage } from '../../api';
import { configFixture } from '../../testFixtures';
import type { MockInstance } from 'vitest';
import type { ShellApi } from './shellShared';
import type { SettingsViewApi } from '../settings/settingsViewShared';
import type { SchedulingSectionApi } from '../settings/schedulingSectionShared';
import type { BackupSectionApi } from '../settings/backupSectionShared';
import type { StatusViewApi } from '../status/statusViewShared';
import type { TaskFormApi } from '../tasks/taskFormShared';
import { makeTestProfileState } from '../profile/profileTestSupport';

// router.svelte.ts exports a singleton constructed at import time, which reads
// window.location.hash and registers a hashchange listener. There is no seam to inject
// through until the router is made injectable the way the api clients were; until then
// this is the only module still stubbed here.
// eslint-disable-next-line no-restricted-syntax -- router singleton, no injection seam yet
vi.mock('../../router.svelte', () => ({
  router: {
    current: 'settings',
    navigate: vi.fn(),
  },
}));

const { router } = await import('../../router.svelte');

/** Quiet default — no restore this run. Shell AND the mounted BackupSection read it. */
const QUIET_BACKUP_STATUS: BackupStatus = {
  enabled: false,
  connected: false,
  last_export_at: null,
  last_backup_error: null,
  restored_this_run: null,
};

let fakeShellApi: {
  getBackupStatus: MockInstance<() => Promise<BackupStatus>>;
} & ShellApi;

let fakeSettingsApi: SettingsViewApi;
let fakeSchedulingApi: SchedulingSectionApi;
let fakeBackupApi: BackupSectionApi;

let fakeStatusApi: {
  triggerReschedule: MockInstance<() => Promise<ScheduleResult>>;
  getTask: MockInstance<(id: string) => Promise<Task>>;
} & StatusViewApi;

let fakeTaskFormApi: TaskFormApi;

function makeShellProps() {
  return {
    apiClient: fakeShellApi,
    settingsApiClient: fakeSettingsApi,
    schedulingApiClient: fakeSchedulingApi,
    backupApiClient: fakeBackupApi,
    statusApiClient: fakeStatusApi,
    taskFormApiClient: fakeTaskFormApi,
  };
}

beforeEach(() => {
  fakeShellApi = {
    getBackupStatus: vi.fn().mockResolvedValue(QUIET_BACKUP_STATUS),
  };

  fakeSettingsApi = {
    googleAuthStatus: vi.fn().mockResolvedValue({ type: 'not_connected' }),
    beginGoogleAuth: vi.fn(),
    openExternalUrl: vi.fn(),
    googleListCalendars: vi.fn(),
    getPullCalendars: vi.fn(),
    setPullCalendars: vi.fn(),
    googleDisconnect: vi.fn(),
    getSyncStatus: vi.fn(),
    syncNow: vi.fn(),
    syncErrorMessage,
  };

  fakeSchedulingApi = {
    getConfig: vi.fn().mockResolvedValue(configFixture()),
    updateConfig: vi.fn(),
    apiErrorMessage,
  };

  fakeBackupApi = {
    getBackupStatus: vi.fn().mockResolvedValue(QUIET_BACKUP_STATUS),
    setBackupEnabled: vi.fn(),
    backupNow: vi.fn(),
    exportBackupToFile: vi.fn(),
    importBackupFromFile: vi.fn(),
    apiErrorMessage,
    backupErrorMessage,
  };

  fakeStatusApi = {
    triggerReschedule: vi.fn().mockResolvedValue({ placed_chunks: [], warnings: [] }),
    getTask: vi.fn(),
    updateTask: vi.fn(),
    completeTask: vi.fn(),
    cancelTask: vi.fn(),
    listChunksForTask: vi.fn().mockResolvedValue([]),
    createFixedChunk: vi.fn(),
    apiErrorMessage,
  };

  fakeTaskFormApi = {
    listComments: vi.fn().mockResolvedValue([]),
    createComment: vi.fn(),
    updateComment: vi.fn(),
    deleteComment: vi.fn(),
    listChunksForTask: vi.fn().mockResolvedValue([]),
    unlockChunk: vi.fn(),
    deleteFixedChunk: vi.fn(),
  };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  warningState.clear();
  taskState.reset();
  toastState.reset();
  profileState.status = null;
  profileState.loadError = null;
  profileState.switching = false;
  router.current = 'settings';
  resetShortcutsForTest();
});

const BLOCKING_WARNING: ScheduleWarning = {
  task_id: 'task-1',
  task_title: 'Alpha task',
  kind: { Unschedulable: { reason: 'No schedule windows are available.' } },
};
const DEADLINE_WARNING: ScheduleWarning = {
  task_id: 'task-2',
  task_title: 'Beta task',
  kind: {
    DeadlineViolation: {
      deadline: '2026-06-01T00:00:00Z',
      earliest_completion: '2026-06-03T00:00:00Z',
    },
  },
};

describe('Shell', () => {
  it.each<{
    label: string;
    warnings: ScheduleWarning[];
    badgeLabel: RegExp | null;
    expectBlocking: boolean | null;
  }>([
    {
      label: 'danger-colored badge for blocking warnings',
      warnings: [BLOCKING_WARNING, DEADLINE_WARNING],
      badgeLabel: /2 warnings/i,
      expectBlocking: true,
    },
    {
      label: 'warning-colored badge when no warning is blocking',
      warnings: [DEADLINE_WARNING],
      badgeLabel: /1 warning/i,
      expectBlocking: false,
    },
    {
      label: 'no badge when there are no warnings',
      warnings: [],
      badgeLabel: null,
      expectBlocking: null,
    },
  ])('warning badge: $label', ({ warnings, badgeLabel, expectBlocking }) => {
    warningState.items = warnings;
    const { queryByLabelText, getByRole } = render(Shell, { props: makeShellProps() });
    if (badgeLabel === null) {
      expect(queryByLabelText(/warning/i)).toBeNull();
    } else {
      const badge = queryByLabelText(badgeLabel);
      expect(badge).toBeTruthy();
      // Badge is its own control, NOT nested inside the nav button.
      expect(badge!.tagName).toBe('BUTTON');
      expect(getByRole('button', { name: 'Status' }).contains(badge)).toBe(false);
      expect(badge!.classList.contains('warning-badge--blocking')).toBe(expectBlocking);
    }
  });

  it('renders the status view when the route is "status"', async () => {
    router.current = 'status';

    const { getByText } = render(Shell, { props: makeShellProps() });
    await flush();

    expect(getByText('Scheduling status')).toBeTruthy();
    expect(fakeStatusApi.triggerReschedule).toHaveBeenCalled();
  });

  it('forwards scheduling and backup api clients to SettingsView children', async () => {
    render(Shell, { props: makeShellProps() });
    await flush();

    expect(vi.mocked(fakeSchedulingApi.getConfig)).toHaveBeenCalled();
    expect(vi.mocked(fakeBackupApi.getBackupStatus)).toHaveBeenCalled();
  });

  // Regression: the toast host was once never mounted anywhere, so every pushed
  // toast was invisible. Assert against the rendered DOM, not the store.
  it('mounts the toast host so pushed toasts are visible', async () => {
    const { getByLabelText, getByText } = render(Shell, { props: makeShellProps() });

    toastState.error('Could not start Google sign-in.');
    await tick();

    expect(getByLabelText('Notifications')).toBeTruthy();
    expect(getByText('Could not start Google sign-in.')).toBeTruthy();
  });
});

describe('Shell — keyboard shortcuts', () => {
  it.each([
    { key: '1', route: 'calendar' },
    { key: '2', route: 'tasks' },
    { key: '3', route: 'settings' },
    { key: '4', route: 'status' },
  ])('pressing "$key" calls router.navigate with "$route"', async ({ key, route }) => {
    render(Shell, { props: makeShellProps() });
    await tick();

    await fireEvent.keyDown(window, { key, bubbles: true, cancelable: true });

    expect(router.navigate).toHaveBeenCalledWith(route);
  });

  it('pressing "?" opens the shortcut overlay dialog', async () => {
    const { queryByRole } = render(Shell, { props: makeShellProps() });
    await tick();

    expect(queryByRole('dialog')).toBeNull();

    await fireEvent.keyDown(window, { key: '?', shiftKey: true, bubbles: true, cancelable: true });
    await tick();

    expect(queryByRole('dialog')).toBeTruthy();
  });
});

describe('Shell — profiles route', () => {
  it('renders the profiles view when the route is "profiles"', async () => {
    router.current = 'profiles';

    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockResolvedValue({
      active: { id: 'p-1', name: 'Default' },
      profiles: [{ id: 'p-1', name: 'Default', created_at: '2026-07-01T00:00:00Z' }],
      last_used: 'p-1',
    });

    const { getByText } = render(Shell, { props: { ...makeShellProps(), profileStore: store } });
    await flush();

    expect(getByText('Danger zone')).toBeTruthy();
  });
});

async function flush(count = 1): Promise<void> {
  for (let i = 0; i < count; i++) {
    await Promise.resolve();
    await tick();
  }
}

describe('Shell — startup restore notice', () => {
  type CheckFns = {
    getByText: (text: string | RegExp) => HTMLElement;
    queryByText: (text: string | RegExp) => HTMLElement | null;
  };

  it.each([
    {
      label: 'with timestamp, shows dated restore notice',
      setup: () =>
        fakeShellApi.getBackupStatus.mockResolvedValue({
          enabled: true,
          connected: true,
          last_export_at: '2026-07-12T10:00:00Z',
          last_backup_error: null,
          restored_this_run: '2026-07-12T09:30:00Z',
        }),
      check: ({ getByText }: CheckFns) => {
        expect(
          getByText(/Restored this profile from its Drive backup \(last change /),
        ).toBeTruthy();
      },
    },
    {
      label: 'no last-change stamp, shows plain restore notice',
      setup: () =>
        fakeShellApi.getBackupStatus.mockResolvedValue({
          enabled: true,
          connected: true,
          last_export_at: null,
          last_backup_error: null,
          restored_this_run: '',
        }),
      check: ({ getByText }: CheckFns) => {
        expect(getByText('Restored this profile from its Drive backup.')).toBeTruthy();
      },
    },
    {
      label: 'no restore this run, stays silent',
      setup: () => {},
      check: ({ queryByText }: CheckFns) => {
        expect(queryByText(/Restored this profile/)).toBeNull();
      },
    },
    {
      label: 'status probe fails, shows error toast',
      setup: () => fakeShellApi.getBackupStatus.mockRejectedValue(new Error('io error')),
      check: ({ queryByText }: CheckFns) => {
        expect(queryByText(/Restored this profile/)).toBeNull();
        expect(toastState.items).toHaveLength(1);
        expect(toastState.items[0].level).toBe('error');
        expect(toastState.items[0].text).toMatch(/backup status/i);
      },
    },
  ])('$label', async ({ setup, check }) => {
    setup();
    const { getByText, queryByText } = render(Shell, { props: makeShellProps() });
    await flush(2);
    check({ getByText, queryByText });
  });
});

describe('Shell — status warnings modal', () => {
  const WARNING = {
    task_id: 'task-1',
    task_title: 'Alpha task',
    kind: {
      DeadlineViolation: {
        deadline: '2026-06-01T00:00:00Z',
        earliest_completion: '2026-06-03T00:00:00Z',
      },
    },
  };

  const TASK: Task = {
    id: 'task-1',
    title: 'Alpha task',
    description: null,
    duration_minutes: 60,
    time_logged_minutes: 0,
    priority: 'Medium',
    status: 'scheduled',
    start_date: null,
    deadline: '2026-06-01T00:00:00Z',
    schedule_id: 'sched-1',
    min_chunk_minutes: 15,
    no_split: false,
    recurring_template_id: null,
    labels: [],
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  };

  beforeEach(() => {
    warningState.items = [WARNING];
    // The modal-mounted StatusView refreshes on open — keep the warning alive.
    fakeStatusApi.triggerReschedule.mockResolvedValue({ placed_chunks: [], warnings: [WARNING] });
    fakeStatusApi.getTask.mockResolvedValue(TASK);
  });

  it('clicking the badge opens the warnings modal without navigating', async () => {
    const { getByLabelText, getByRole } = render(Shell, { props: makeShellProps() });
    await flush();

    await fireEvent.click(getByLabelText(/1 warning/i));
    await flush(2);

    expect(router.navigate).not.toHaveBeenCalled();
    expect(getByRole('dialog')).toBeTruthy();
    expect(getByRole('heading', { name: '1 task needs attention' })).toBeTruthy();
    expect(getByRole('button', { name: 'Alpha task' })).toBeTruthy();
    // Opening re-derives warnings, same as visiting the status page.
    expect(fakeStatusApi.triggerReschedule).toHaveBeenCalledTimes(1);
  });

  it('the close button dismisses the modal', async () => {
    const { getByLabelText, getByRole, queryByRole } = render(Shell, { props: makeShellProps() });
    await flush();

    await fireEvent.click(getByLabelText(/1 warning/i));
    await flush(2);

    await fireEvent.click(getByRole('button', { name: 'Close dialog' }));
    await tick();

    expect(queryByRole('dialog')).toBeNull();
  });

  it('Escape in the nested task editor closes only the editor', async () => {
    const { getByLabelText, getByRole, getAllByRole } = render(Shell, { props: makeShellProps() });
    await flush();

    await fireEvent.click(getByLabelText(/1 warning/i));
    await flush(2);

    await fireEvent.click(getByRole('button', { name: 'Alpha task' }));
    await flush(2);

    expect(vi.mocked(fakeTaskFormApi.listComments)).toHaveBeenCalledWith('task-1');

    const dialogs = getAllByRole('dialog');
    expect(dialogs).toHaveLength(2);

    await fireEvent.keyDown(dialogs[1], { key: 'Escape' });
    await tick();

    expect(getAllByRole('dialog')).toHaveLength(1);
    expect(getByRole('heading', { name: '1 task needs attention' })).toBeTruthy();
  });
});
