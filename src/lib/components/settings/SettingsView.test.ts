// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockInstance } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { toastState } from '../../stores/toast.svelte';
import { warningState } from '../../stores/warnings.svelte';
import { formatDateTime } from '../../utils';
import { syncErrorMessage } from '../../api';
import type {
  AppConfig,
  AuthStatus,
  BackupStatus,
  Chunk,
  ExternalCalendar,
  SyncOutcome,
  SyncStatus,
  UpdateConfigInput,
} from '../../types';
import SettingsView from './SettingsView.svelte';
import { type SettingsViewApi } from './settingsViewShared';
import type { SchedulingSectionApi } from './schedulingSectionShared';
import type { BackupSectionApi } from './backupSectionShared';

const PRIMARY_CAL: ExternalCalendar = { id: 'cal-primary', title: 'My Calendar', primary: true };
const SECONDARY_CAL: ExternalCalendar = { id: 'cal-secondary', title: 'Work', primary: false };

const NEVER_SYNCED: SyncStatus = { last_sync_at: null, last_sync_error: null };

const QUIET_BACKUP_STATUS: BackupStatus = {
  enabled: false,
  connected: false,
  last_export_at: null,
  last_backup_error: null,
  restored_this_run: null,
};

const QUIET_CONFIG: AppConfig = {
  planning_horizon_days: 30,
  timezone: 'UTC',
  max_continuous_minutes: 120,
  min_break_minutes: 5,
  last_reschedule: null,
  last_mutation: null,
  last_sync: null,
  last_busy_sync: null,
};

let fakeApi: {
  googleAuthStatus: MockInstance<() => Promise<AuthStatus>>;
  beginGoogleAuth: MockInstance<() => Promise<string>>;
  openExternalUrl: MockInstance<(url: string) => Promise<void>>;
  googleListCalendars: MockInstance<() => Promise<ExternalCalendar[]>>;
  getPullCalendars: MockInstance<() => Promise<string[]>>;
  setPullCalendars: MockInstance<(calendarIds: string[]) => Promise<void>>;
  googleDisconnect: MockInstance<() => Promise<void>>;
  getSyncStatus: MockInstance<() => Promise<SyncStatus>>;
  syncNow: MockInstance<() => Promise<SyncOutcome>>;
  syncErrorMessage: (e: unknown, fallback: string) => string;
} & SettingsViewApi;

let fakeSchedulingApi: SchedulingSectionApi;
let fakeBackupApi: BackupSectionApi;

beforeEach(() => {
  fakeApi = {
    googleAuthStatus: vi.fn<() => Promise<AuthStatus>>(),
    beginGoogleAuth: vi.fn<() => Promise<string>>(),
    openExternalUrl: vi.fn<(url: string) => Promise<void>>(),
    googleListCalendars: vi.fn<() => Promise<ExternalCalendar[]>>(),
    getPullCalendars: vi.fn<() => Promise<string[]>>(),
    setPullCalendars: vi.fn<(calendarIds: string[]) => Promise<void>>(),
    googleDisconnect: vi.fn<() => Promise<void>>(),
    getSyncStatus: vi.fn<() => Promise<SyncStatus>>(),
    syncNow: vi.fn<() => Promise<SyncOutcome>>(),
    syncErrorMessage: (e, fallback) => syncErrorMessage(e, fallback),
  };

  fakeSchedulingApi = {
    getConfig: vi.fn<() => Promise<AppConfig>>().mockResolvedValue(QUIET_CONFIG),
    updateConfig: vi
      .fn<(c: UpdateConfigInput) => Promise<AppConfig>>()
      .mockResolvedValue(QUIET_CONFIG),
    apiErrorMessage: vi.fn().mockReturnValue(''),
  };

  fakeBackupApi = {
    getBackupStatus: vi.fn<() => Promise<BackupStatus>>().mockResolvedValue(QUIET_BACKUP_STATUS),
    setBackupEnabled: vi
      .fn<(enabled: boolean) => Promise<BackupStatus>>()
      .mockResolvedValue(QUIET_BACKUP_STATUS),
    backupNow: vi.fn<() => Promise<BackupStatus>>().mockResolvedValue(QUIET_BACKUP_STATUS),
    exportBackupToFile: vi.fn<(path: string) => Promise<void>>().mockResolvedValue(undefined),
    importBackupFromFile: vi.fn<(path: string) => Promise<void>>().mockResolvedValue(undefined),
    apiErrorMessage: vi.fn().mockReturnValue(''),
    backupErrorMessage: vi.fn().mockReturnValue(''),
  };
});

afterEach(async () => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
  toastState.items = [];
  warningState.items = [];
});

function makeChunk(id: string): Chunk {
  return {
    id,
    task_id: 't1',
    start_time: '2026-07-11T09:00:00Z',
    end_time: '2026-07-11T10:00:00Z',
    status: 'scheduled',
    is_fixed: false,
    logged_minutes: null,
    completed_at: null,
    google_event_id: null,
    created_at: '2026-07-11T00:00:00Z',
    updated_at: '2026-07-11T00:00:00Z',
  };
}

/**
 * Flush one level of microtask queue + Svelte reactivity.
 * Call twice when effects chain two async layers (e.g. loadPicker inside
 * the .then of googleAuthStatus).
 */
async function flush() {
  await Promise.resolve();
  await tick();
}

/**
 * Set fakeApi to the connected-account happy path: status + calendars + saved selection +
 * sync status. Callers override individual fakeApi methods after this to exercise failure branches.
 */
function mockConnected(
  opts: {
    email?: string | null;
    calendars?: ExternalCalendar[];
    pull?: string[];
    syncStatus?: SyncStatus;
  } = {},
) {
  const { email = 'user@example.com', calendars = [], pull = [], syncStatus = NEVER_SYNCED } = opts;
  fakeApi.googleAuthStatus.mockResolvedValue({ type: 'connected', email });
  fakeApi.googleListCalendars.mockResolvedValue(calendars);
  fakeApi.getPullCalendars.mockResolvedValue(pull);
  fakeApi.getSyncStatus.mockResolvedValue(syncStatus);
}

/**
 * Render SettingsView with injected fakes, then flush the mount effects.
 * Pass `flushes: 1` for the not_connected / pending / connect paths (single async layer);
 * the default 2 resolves googleAuthStatus.then and the loadPicker it chains.
 */
async function mountAndFlush(flushes = 2) {
  const utils = render(SettingsView, {
    props: {
      apiClient: fakeApi,
      schedulingApiClient: fakeSchedulingApi,
      backupApiClient: fakeBackupApi,
    },
  });
  await flush();
  if (flushes > 1) await flush();
  return utils;
}

function calendarCheckbox(labelText: string): HTMLInputElement | undefined {
  const checkboxes = screen.getAllByRole('checkbox') as HTMLInputElement[];
  return checkboxes.find((cb) => cb.closest('label')?.textContent?.includes(labelText));
}

async function clickConnect() {
  const btn = screen.getByText('Connect Google Calendar');
  await fireEvent.click(btn);
  await flush();
  return btn;
}

describe('SettingsView — not_connected on mount', () => {
  beforeEach(() => {
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'not_connected' });
  });

  it('shows "Not connected" and Connect button; picker and Sync absent; googleListCalendars not called', async () => {
    const { getByText, queryByText } = await mountAndFlush(1);

    expect(getByText('Not connected')).toBeTruthy();
    expect(getByText('Connect Google Calendar')).toBeTruthy();
    expect(queryByText('Sync now')).toBeNull();
    expect(queryByText('Calendars to import')).toBeNull();
    expect(fakeApi.googleListCalendars).not.toHaveBeenCalled();
    expect(fakeSchedulingApi.getConfig).toHaveBeenCalledOnce();
    expect(fakeBackupApi.getBackupStatus).toHaveBeenCalledOnce();
  });
});

describe('SettingsView — connected status line', () => {
  it.each([
    ['a@b.c', 'Connected as a@b.c'],
    [null, 'Connected'],
  ])('email %s renders %s', async (email, expectedText) => {
    mockConnected({ email });

    const { getByText } = await mountAndFlush();

    expect(getByText(expectedText)).toBeTruthy();
    expect(getByText('Reconnect')).toBeTruthy();
    expect(getByText('Disconnect…')).toBeTruthy();
  });
});

describe('SettingsView — connected mounts picker', () => {
  it('calls googleListCalendars + getPullCalendars; renders checkboxes, saved selection, and (primary) marker', async () => {
    mockConnected({ calendars: [PRIMARY_CAL, SECONDARY_CAL], pull: ['cal-primary'] });

    const { getByText } = await mountAndFlush();

    expect(fakeApi.googleListCalendars).toHaveBeenCalledOnce();
    expect(fakeApi.getPullCalendars).toHaveBeenCalledOnce();

    expect(getByText('My Calendar (primary)')).toBeTruthy();
    expect(getByText('Work')).toBeTruthy();

    expect(calendarCheckbox('primary')?.checked).toBe(true);
    expect(calendarCheckbox('Work')?.checked).toBe(false);
  });
});

describe('SettingsView — Connect click happy path', () => {
  it('calls beginGoogleAuth, opens URL, polls, shows toast when connected', async () => {
    vi.useFakeTimers();

    fakeApi.googleAuthStatus
      .mockResolvedValueOnce({ type: 'not_connected' })
      .mockResolvedValue({ type: 'connected', email: 'new@example.com' });
    const consentUrl = 'https://accounts.google.com/o/oauth2/auth?state=abc';
    fakeApi.beginGoogleAuth.mockResolvedValue(consentUrl);
    fakeApi.openExternalUrl.mockResolvedValue(undefined);
    fakeApi.googleListCalendars.mockResolvedValue([]);
    fakeApi.getPullCalendars.mockResolvedValue([]);
    fakeApi.getSyncStatus.mockResolvedValue(NEVER_SYNCED);

    await mountAndFlush(1);

    await clickConnect();

    expect(fakeApi.beginGoogleAuth).toHaveBeenCalledOnce();
    expect(fakeApi.openExternalUrl).toHaveBeenCalledWith(consentUrl);

    await vi.advanceTimersByTimeAsync(2000);
    await flush();

    expect(fakeApi.googleAuthStatus).toHaveBeenCalledTimes(2);
    expect(toastState.items.some((t) => t.text.includes('connected'))).toBe(true);

    const countAfterConnect = fakeApi.googleAuthStatus.mock.calls.length;
    await vi.advanceTimersByTimeAsync(10000);
    await flush();
    expect(fakeApi.googleAuthStatus.mock.calls).toHaveLength(countAfterConnect);
  });
});

describe('SettingsView — pending on mount', () => {
  it('resumes status polling without starting a new auth flow', async () => {
    vi.useFakeTimers();
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'pending' });

    const { getByText } = await mountAndFlush(1);

    expect(getByText(/Waiting for you to finish signing in/)).toBeTruthy();

    await vi.advanceTimersByTimeAsync(4000);
    await flush();

    expect(fakeApi.googleAuthStatus).toHaveBeenCalledTimes(3);
    expect(fakeApi.beginGoogleAuth).not.toHaveBeenCalled();
    expect(fakeApi.openExternalUrl).not.toHaveBeenCalled();
  });

  it('stops polling silently after 150 ticks (~5 minutes)', async () => {
    vi.useFakeTimers();
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'pending' });

    const { getByText } = await mountAndFlush(1);

    await vi.advanceTimersByTimeAsync(302_000);
    await flush();

    const countAtTimeout = fakeApi.googleAuthStatus.mock.calls.length;
    expect(countAtTimeout).toBe(151);

    await vi.advanceTimersByTimeAsync(20_000);
    await flush();

    expect(fakeApi.googleAuthStatus.mock.calls).toHaveLength(countAtTimeout);
    expect(getByText(/Waiting for you to finish signing in/)).toBeTruthy();
  });
});

describe('SettingsView — Connect click failure', () => {
  it('shows error toast with sanitized message; openExternalUrl not called; button re-enabled', async () => {
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'not_connected' });
    fakeApi.beginGoogleAuth.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: network error',
    });

    await mountAndFlush(1);
    const connectBtn = await clickConnect();

    expect(fakeApi.openExternalUrl).not.toHaveBeenCalled();
    expect(
      toastState.items.some((t) => t.text.includes('Calendar sync error: network error')),
    ).toBe(true);
    expect((connectBtn as HTMLButtonElement).disabled).toBe(false);
  });

  it('browser open failure: error toast shown, consent flow keeps waiting', async () => {
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'not_connected' });
    fakeApi.beginGoogleAuth.mockResolvedValue('https://example.invalid/consent');
    fakeApi.openExternalUrl.mockRejectedValue(new Error('no browser'));

    const { getByText } = await mountAndFlush(1);
    await clickConnect();

    expect(toastState.items.some((t) => t.text.includes('Could not open the browser'))).toBe(true);
    expect(getByText(/Waiting for you to finish signing in/)).toBeTruthy();
  });
});

describe('SettingsView — Disconnect flow', () => {
  beforeEach(() => {
    mockConnected({ email: 'x@example.com', calendars: [PRIMARY_CAL], pull: ['cal-primary'] });
  });

  it('confirm: googleDisconnect called, status refetched, success toast', async () => {
    fakeApi.googleDisconnect.mockResolvedValue(undefined);
    fakeApi.googleAuthStatus
      .mockResolvedValueOnce({ type: 'connected', email: 'x@example.com' })
      .mockResolvedValue({ type: 'not_connected' });

    const { getByText, queryByRole } = await mountAndFlush();

    expect(queryByRole('alertdialog')).toBeNull();

    await fireEvent.click(getByText('Disconnect…'));
    await flush();

    expect(queryByRole('alertdialog')).toBeTruthy();

    await fireEvent.click(getByText('Disconnect'));
    await flush();

    expect(fakeApi.googleDisconnect).toHaveBeenCalledOnce();
    expect(toastState.items.some((t) => t.text.includes('disconnected'))).toBe(true);
  });

  it('cancel: googleDisconnect NOT called', async () => {
    const { getByText, queryByRole } = await mountAndFlush();

    await fireEvent.click(getByText('Disconnect…'));
    await flush();

    expect(queryByRole('alertdialog')).toBeTruthy();

    await fireEvent.click(getByText('Cancel'));
    await flush();

    expect(fakeApi.googleDisconnect).not.toHaveBeenCalled();
  });

  it('failure: error toast shown, dialog closed, button re-enabled', async () => {
    fakeApi.googleDisconnect.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: HTTP 500',
    });

    const { getByText, queryByRole } = await mountAndFlush();

    await fireEvent.click(getByText('Disconnect…'));
    await flush();
    await fireEvent.click(getByText('Disconnect'));
    await flush();

    expect(toastState.items.some((t) => t.text.includes('Calendar sync error: HTTP 500'))).toBe(
      true,
    );
    expect(queryByRole('alertdialog')).toBeNull();
    expect((getByText('Disconnect…') as HTMLButtonElement).disabled).toBe(false);
  });
});

describe('SettingsView — checkbox toggle', () => {
  beforeEach(() => {
    mockConnected({ calendars: [PRIMARY_CAL, SECONDARY_CAL], pull: ['cal-primary'] });
  });

  it('happy path: setPullCalendars called with updated id array', async () => {
    fakeApi.setPullCalendars.mockResolvedValue(undefined);

    await mountAndFlush();

    const secondaryBox = calendarCheckbox('Work')!;
    await fireEvent.click(secondaryBox);
    await flush();

    expect(fakeApi.setPullCalendars).toHaveBeenCalledWith(
      expect.arrayContaining(['cal-primary', 'cal-secondary']),
    );
  });

  it('failure: error toast shown and getPullCalendars re-called to revert selection', async () => {
    fakeApi.setPullCalendars.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: save failed',
    });
    fakeApi.getPullCalendars
      .mockResolvedValueOnce(['cal-primary'])
      .mockResolvedValue(['cal-primary']);

    await mountAndFlush();

    const secondaryBox = calendarCheckbox('Work')!;
    await fireEvent.click(secondaryBox);
    await flush();
    await flush();

    expect(toastState.items.some((t) => t.text.includes('Calendar sync error'))).toBe(true);
    expect(fakeApi.getPullCalendars).toHaveBeenCalledTimes(2);
  });
});

describe('SettingsView — Sync now', () => {
  beforeEach(() => {
    mockConnected();
  });

  it.each([
    [1, '1 chunk scheduled, 0 Google events updated'],
    [2, '2 chunks scheduled, 3 Google events updated'],
  ])(
    'syncNow with %i placed chunks: warningState updated; toast says "%s"; sync status refreshed',
    async (count, expectedText) => {
      fakeApi.syncNow.mockResolvedValue({
        schedule: {
          placed_chunks: Array.from({ length: count }, (_, i) => makeChunk(`c${i}`)),
          warnings: [
            {
              task_id: 'task-warn',
              task_title: 'Overdue Task',
              kind: { Unschedulable: { reason: 'no windows' } },
            },
          ],
        },
        pushed:
          count === 1
            ? { created: 0, updated: 0, deleted: 0 }
            : { created: 1, updated: 1, deleted: 1 },
      });

      const { getByText } = await mountAndFlush();

      await fireEvent.click(getByText('Sync now'));
      await flush();
      await flush();

      expect(fakeApi.syncNow).toHaveBeenCalledOnce();
      expect(warningState.items).toHaveLength(1);
      expect(warningState.items[0].task_id).toBe('task-warn');
      expect(toastState.items.some((t) => t.text.includes(expectedText))).toBe(true);
      expect(fakeApi.getSyncStatus).toHaveBeenCalledTimes(2);
    },
  );

  it('failure: error toast with sanitized message; button re-enabled; sync status still refreshed', async () => {
    fakeApi.syncNow.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: HTTP 503',
    });

    const { getByText } = await mountAndFlush();

    const syncBtn = getByText('Sync now');
    await fireEvent.click(syncBtn);
    await flush();
    await flush();

    expect(toastState.items.some((t) => t.text.includes('Calendar sync error: HTTP 503'))).toBe(
      true,
    );
    expect((getByText('Sync now') as HTMLButtonElement).disabled).toBe(false);
    expect(warningState.items).toHaveLength(0);
    expect(fakeApi.getSyncStatus).toHaveBeenCalledTimes(2);
  });
});

describe('SettingsView — last-sync display', () => {
  type CheckFns = {
    getByText: (text: string | RegExp) => HTMLElement;
    queryByText: (text: string | RegExp) => HTMLElement | null;
    queryByRole: (role: string) => HTMLElement | null;
  };

  beforeEach(() => {
    mockConnected();
  });

  it.each([
    {
      label: 'never synced shows "Not synced yet."',
      setup: () => fakeApi.getSyncStatus.mockResolvedValue(NEVER_SYNCED),
      check: ({ getByText, queryByText }: CheckFns) => {
        expect(getByText('Not synced yet.')).toBeTruthy();
        expect(queryByText(/Last synced:/)).toBeNull();
      },
    },
    {
      label: 'with timestamp shows formatted local time',
      setup: () =>
        fakeApi.getSyncStatus.mockResolvedValue({
          last_sync_at: '2026-07-12T15:00:00Z',
          last_sync_error: null,
        }),
      check: ({ getByText, queryByText }: CheckFns) => {
        expect(getByText(`Last synced: ${formatDateTime('2026-07-12T15:00:00Z')}`)).toBeTruthy();
        expect(queryByText('Not synced yet.')).toBeNull();
      },
    },
    {
      label: 'getSyncStatus failure is silent: no sync line, no banner, no toast',
      setup: () => fakeApi.getSyncStatus.mockRejectedValue(new Error('io error')),
      check: ({ getByText, queryByRole, queryByText }: CheckFns) => {
        expect(getByText('Sync now')).toBeTruthy();
        expect(queryByRole('alert')).toBeNull();
        expect(queryByText('Not synced yet.')).toBeNull();
        expect(queryByText(/Last synced:/)).toBeNull();
        expect(toastState.items).toHaveLength(0);
      },
    },
  ])('$label', async ({ setup, check }) => {
    setup();
    const { getByText, queryByText, queryByRole } = await mountAndFlush();
    check({ getByText, queryByText, queryByRole });
  });
});

describe('SettingsView — reconnect banner', () => {
  beforeEach(() => {
    mockConnected();
  });

  it('shown when the last sync failed: error message, expiry hint, Reconnect now button', async () => {
    fakeApi.getSyncStatus.mockResolvedValue({
      last_sync_at: '2026-07-10T15:00:00Z',
      last_sync_error: 'Calendar sync error: HTTP 401',
    });

    const { getByRole, getByText } = await mountAndFlush();

    const banner = getByRole('alert');
    expect(banner.textContent).toContain('Calendar sync error: HTTP 401');
    expect(banner.textContent).toContain('sign-in may have expired');
    expect(getByText('Reconnect now')).toBeTruthy();
  });

  it('Reconnect now click starts the consent flow (beginGoogleAuth + browser open)', async () => {
    fakeApi.getSyncStatus.mockResolvedValue({
      last_sync_at: null,
      last_sync_error: 'Calendar sync error: HTTP 401',
    });
    const consentUrl = 'https://accounts.google.com/o/oauth2/auth?state=xyz';
    fakeApi.beginGoogleAuth.mockResolvedValue(consentUrl);
    fakeApi.openExternalUrl.mockResolvedValue(undefined);

    const { getByText } = await mountAndFlush();

    await fireEvent.click(getByText('Reconnect now'));
    await flush();

    expect(fakeApi.beginGoogleAuth).toHaveBeenCalledOnce();
    expect(fakeApi.openExternalUrl).toHaveBeenCalledWith(consentUrl);
  });

  it.each([
    ['never synced', { last_sync_at: null, last_sync_error: null }],
    ['last sync succeeded', { last_sync_at: '2026-07-12T15:00:00Z', last_sync_error: null }],
  ])('absent when %s', async (_label, syncStatus) => {
    fakeApi.getSyncStatus.mockResolvedValue(syncStatus);

    const { queryByRole, queryByText } = await mountAndFlush();

    expect(queryByRole('alert')).toBeNull();
    expect(queryByText('Reconnect now')).toBeNull();
  });
});

describe('SettingsView — calendar load failure', () => {
  it('shows inline error text and Retry button; Retry re-calls googleListCalendars', async () => {
    mockConnected();
    fakeApi.googleListCalendars.mockRejectedValue({
      error: 'calendar_sync',
      message: 'Calendar sync error: could not reach server',
    });

    const { getByText } = await mountAndFlush();

    expect(getByText('Calendar sync error: could not reach server')).toBeTruthy();
    const retryBtn = getByText('Retry');
    expect(retryBtn).toBeTruthy();

    await fireEvent.click(retryBtn);
    await flush();

    expect(fakeApi.googleListCalendars).toHaveBeenCalledTimes(2);
  });
});
