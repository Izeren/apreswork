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

const ROUTE_CALENDAR = 'calendar';
const ROUTE_TASKS = 'tasks';
const ROUTE_SETTINGS = 'settings';
const ROUTE_STATUS = 'status';
const ROUTE_PROFILES = 'profiles';
const SHORTCUT_KEY_CALENDAR = '1';
const SHORTCUT_KEY_TASKS = '2';
const SHORTCUT_KEY_SETTINGS = '3';
const SHORTCUT_KEY_STATUS = '4';
const SHORTCUT_KEY_HELP = '?';
const FLUSH_DIALOG_CYCLES = 2;
const TEST_DEADLINE_TIMESTAMP = '2026-06-01T00:00:00Z';
const TEST_EARLIEST_COMPLETION_TIMESTAMP = '2026-06-03T00:00:00Z';
const TEST_PROFILE_CREATION_TIMESTAMP = '2026-07-01T00:00:00Z';
const TEST_BACKUP_EXPORT_TIMESTAMP = '2026-07-12T10:00:00Z';
const TEST_BACKUP_RESTORE_TIMESTAMP = '2026-07-12T09:30:00Z';
const TEST_TASK_EPOCH = '2026-01-01T00:00:00Z';

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
    googleClientCredentialsSaved: vi.fn().mockResolvedValue(false),
    saveGoogleClientCredentials: vi.fn(),
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
  router.current = ROUTE_SETTINGS;
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
      deadline: TEST_DEADLINE_TIMESTAMP,
      earliest_completion: TEST_EARLIEST_COMPLETION_TIMESTAMP,
    },
  },
};
const DEADLINE_WARNING_2: ScheduleWarning = {
  task_id: 'task-3',
  task_title: 'Gamma task',
  kind: {
    DeadlineViolation: {
      deadline: TEST_DEADLINE_TIMESTAMP,
      earliest_completion: TEST_EARLIEST_COMPLETION_TIMESTAMP,
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
      label: 'danger-colored badge for a single blocking warning',
      warnings: [BLOCKING_WARNING],
      badgeLabel: /1 warning/i,
      expectBlocking: true,
    },
    {
      label: 'warning-colored badge when no warning is blocking',
      warnings: [DEADLINE_WARNING],
      badgeLabel: /1 warning/i,
      expectBlocking: false,
    },
    {
      label: 'warning-colored badge for multiple non-blocking warnings',
      warnings: [DEADLINE_WARNING, DEADLINE_WARNING_2],
      badgeLabel: /2 warnings/i,
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
      expect(badge!.tagName).toBe('BUTTON');
      expect(getByRole('button', { name: 'Status' }).contains(badge)).toBe(false);
      expect(badge!.classList.contains('warning-badge--blocking')).toBe(expectBlocking);
    }
  });

  it('renders the status view when the route is "status"', async () => {
    router.current = ROUTE_STATUS;

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

  // Regression: the toast host was once never mounted anywhere, so every pushed toast was invisible.
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
    { key: SHORTCUT_KEY_CALENDAR, route: ROUTE_CALENDAR },
    { key: SHORTCUT_KEY_TASKS, route: ROUTE_TASKS },
    { key: SHORTCUT_KEY_SETTINGS, route: ROUTE_SETTINGS },
    { key: SHORTCUT_KEY_STATUS, route: ROUTE_STATUS },
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

    await fireEvent.keyDown(window, {
      key: SHORTCUT_KEY_HELP,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    await tick();

    expect(queryByRole('dialog')).toBeTruthy();
  });
});

describe('Shell — profiles route', () => {
  it('renders the profiles view when the route is "profiles"', async () => {
    router.current = ROUTE_PROFILES;

    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockResolvedValue({
      active: { id: 'p-1', name: 'Default' },
      profiles: [{ id: 'p-1', name: 'Default', created_at: TEST_PROFILE_CREATION_TIMESTAMP }],
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
          last_export_at: TEST_BACKUP_EXPORT_TIMESTAMP,
          last_backup_error: null,
          restored_this_run: TEST_BACKUP_RESTORE_TIMESTAMP,
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
    await flush(FLUSH_DIALOG_CYCLES);
    check({ getByText, queryByText });
  });
});

describe('Shell — status warnings modal', () => {
  const WARNING = {
    task_id: 'task-1',
    task_title: 'Alpha task',
    kind: {
      DeadlineViolation: {
        deadline: TEST_DEADLINE_TIMESTAMP,
        earliest_completion: TEST_EARLIEST_COMPLETION_TIMESTAMP,
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
    deadline: TEST_DEADLINE_TIMESTAMP,
    schedule_id: 'sched-1',
    min_chunk_minutes: 15,
    no_split: false,
    recurring_template_id: null,
    labels: [],
    created_at: TEST_TASK_EPOCH,
    updated_at: TEST_TASK_EPOCH,
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
    await flush(FLUSH_DIALOG_CYCLES);

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
    await flush(FLUSH_DIALOG_CYCLES);

    await fireEvent.click(getByRole('button', { name: 'Close dialog' }));
    await tick();

    expect(queryByRole('dialog')).toBeNull();
  });

  it('Escape in the nested task editor closes only the editor', async () => {
    const { getByLabelText, getByRole, getAllByRole } = render(Shell, { props: makeShellProps() });
    await flush();

    await fireEvent.click(getByLabelText(/1 warning/i));
    await flush(FLUSH_DIALOG_CYCLES);

    await fireEvent.click(getByRole('button', { name: 'Alpha task' }));
    await flush(FLUSH_DIALOG_CYCLES);

    expect(vi.mocked(fakeTaskFormApi.listComments)).toHaveBeenCalledWith('task-1');

    const dialogs = getAllByRole('dialog');
    expect(dialogs).toHaveLength(2);

    await fireEvent.keyDown(dialogs[1], { key: 'Escape' });
    await tick();

    expect(getAllByRole('dialog')).toHaveLength(1);
    expect(getByRole('heading', { name: '1 task needs attention' })).toBeTruthy();
  });
});
