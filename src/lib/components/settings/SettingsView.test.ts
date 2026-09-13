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
import {
  GOOGLE_CLIENT_ID_SUFFIX,
  POLL_INTERVAL_MS,
  POLL_MAX_TICKS,
  type SettingsViewApi,
} from './settingsViewShared';
import type { SchedulingSectionApi } from './schedulingSectionShared';
import type { BackupSectionApi } from './backupSectionShared';

const INITIAL_POLL_CHECK_INTERVALS = 2;
const STOP_VERIFICATION_INTERVALS = 5;
const EXTENDED_TIMEOUT_INTERVALS = 10;
const INITIAL_CALL_COUNT = 1;
const INITIAL_CALL_PLUS_ONE = 2;
const FLUSHES_SINGLE_LAYER = 1;
const DEFAULT_PLANNING_HORIZON_DAYS = 30;
const DEFAULT_MAX_CONTINUOUS_MINUTES = 120;
const DEFAULT_MIN_BREAK_MINUTES = 5;
const FLUSHES_CONNECTED = 2;

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
  planning_horizon_days: DEFAULT_PLANNING_HORIZON_DAYS,
  timezone: 'UTC',
  max_continuous_minutes: DEFAULT_MAX_CONTINUOUS_MINUTES,
  min_break_minutes: DEFAULT_MIN_BREAK_MINUTES,
  last_reschedule: null,
  last_mutation: null,
  last_sync: null,
  last_busy_sync: null,
};

let fakeApi: {
  googleClientCredentialsSaved: MockInstance<() => Promise<boolean>>;
  saveGoogleClientCredentials: MockInstance<(id: string, secret: string) => Promise<void>>;
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
    googleClientCredentialsSaved: vi.fn<() => Promise<boolean>>().mockResolvedValue(false),
    saveGoogleClientCredentials: vi
      .fn<(id: string, secret: string) => Promise<void>>()
      .mockResolvedValue(undefined),
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

/** Call twice when effects chain two async layers (for example, loadPicker inside the then of googleAuthStatus). */
async function flush() {
  await Promise.resolve();
  await tick();
}

/** Callers override individual fakeApi methods after this to exercise failure branches. */
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
 * Pass `flushes: 1` for the not_connected / pending / connect paths (single async layer);
 * the default 2 resolves googleAuthStatus.then and the loadPicker it chains.
 */
async function mountAndFlush(flushes = FLUSHES_CONNECTED) {
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

async function fillAndSubmitCredForm(utils: Awaited<ReturnType<typeof mountAndFlush>>) {
  const { getByLabelText, getByText } = utils;
  await fireEvent.input(getByLabelText('Client ID'), {
    target: { value: 'my-id.apps.googleusercontent.com' },
  });
  await fireEvent.input(getByLabelText('Client Secret'), { target: { value: 'GOCSPX-secret' } });
  const form = getByText('Save').closest('form')!;
  await fireEvent.submit(form);
  await flush();
}

describe('SettingsView — not_connected on mount', () => {
  beforeEach(() => {
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'not_connected' });
  });

  it('shows "Not connected" and Connect button; picker and Sync absent; googleListCalendars not called', async () => {
    const { getByText, queryByText } = await mountAndFlush(FLUSHES_SINGLE_LAYER);

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
    mockConnected({ calendars: [PRIMARY_CAL, SECONDARY_CAL], pull: [PRIMARY_CAL.id] });

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

    await mountAndFlush(FLUSHES_SINGLE_LAYER);

    await clickConnect();

    expect(fakeApi.beginGoogleAuth).toHaveBeenCalledOnce();
    expect(fakeApi.openExternalUrl).toHaveBeenCalledWith(consentUrl);

    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS);
    await flush();

    expect(fakeApi.googleAuthStatus).toHaveBeenCalledTimes(INITIAL_CALL_PLUS_ONE);
    expect(toastState.items.some((t) => t.text.includes('connected'))).toBe(true);

    const countAfterConnect = fakeApi.googleAuthStatus.mock.calls.length;
    await vi.advanceTimersByTimeAsync(STOP_VERIFICATION_INTERVALS * POLL_INTERVAL_MS);
    await flush();
    expect(fakeApi.googleAuthStatus.mock.calls).toHaveLength(countAfterConnect);
  });
});

describe('SettingsView — pending on mount', () => {
  it('resumes status polling without starting a new auth flow', async () => {
    vi.useFakeTimers();
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'pending' });

    const { getByText } = await mountAndFlush(FLUSHES_SINGLE_LAYER);

    expect(getByText(/Waiting for you to finish signing in/)).toBeTruthy();

    await vi.advanceTimersByTimeAsync(INITIAL_POLL_CHECK_INTERVALS * POLL_INTERVAL_MS);
    await flush();

    expect(fakeApi.googleAuthStatus).toHaveBeenCalledTimes(
      INITIAL_CALL_COUNT + INITIAL_POLL_CHECK_INTERVALS,
    );
    expect(fakeApi.beginGoogleAuth).not.toHaveBeenCalled();
    expect(fakeApi.openExternalUrl).not.toHaveBeenCalled();
  });

  it('stops polling silently after 150 ticks (~5 minutes)', async () => {
    vi.useFakeTimers();
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'pending' });

    const { getByText } = await mountAndFlush(FLUSHES_SINGLE_LAYER);

    await vi.advanceTimersByTimeAsync((POLL_MAX_TICKS + INITIAL_CALL_COUNT) * POLL_INTERVAL_MS);
    await flush();

    const countAtTimeout = fakeApi.googleAuthStatus.mock.calls.length;
    expect(countAtTimeout).toBe(POLL_MAX_TICKS + INITIAL_CALL_COUNT);

    await vi.advanceTimersByTimeAsync(EXTENDED_TIMEOUT_INTERVALS * POLL_INTERVAL_MS);
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

    await mountAndFlush(FLUSHES_SINGLE_LAYER);
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

    const { getByText } = await mountAndFlush(FLUSHES_SINGLE_LAYER);
    await clickConnect();

    expect(toastState.items.some((t) => t.text.includes('Could not open the browser'))).toBe(true);
    expect(getByText(/Waiting for you to finish signing in/)).toBeTruthy();
  });
});

describe('SettingsView — Disconnect flow', () => {
  beforeEach(() => {
    mockConnected({ email: 'x@example.com', calendars: [PRIMARY_CAL], pull: [PRIMARY_CAL.id] });
  });

  it.each([
    {
      label: 'confirm',
      setupMocks: () => {
        fakeApi.googleDisconnect.mockResolvedValue(undefined);
        fakeApi.googleAuthStatus
          .mockResolvedValueOnce({ type: 'connected', email: 'x@example.com' })
          .mockResolvedValue({ type: 'not_connected' });
      },
      checkDialogBefore: true,
      checkDialogAfter: true,
      confirmButtonText: 'Disconnect',
      checkOutcome: (
        _getByText: (t: string) => HTMLElement,
        _queryByRole: (r: string) => HTMLElement | null,
      ) => {
        expect(fakeApi.googleDisconnect).toHaveBeenCalledOnce();
        expect(toastState.items.some((t) => t.text.includes('disconnected'))).toBe(true);
      },
    },
    {
      label: 'failure',
      setupMocks: () => {
        fakeApi.googleDisconnect.mockRejectedValue({
          error: 'calendar_sync',
          message: 'Calendar sync error: HTTP 500',
        });
      },
      checkDialogBefore: false,
      checkDialogAfter: false,
      confirmButtonText: 'Disconnect',
      checkOutcome: (
        getByText: (t: string) => HTMLElement,
        queryByRole: (r: string) => HTMLElement | null,
      ) => {
        expect(toastState.items.some((t) => t.text.includes('Calendar sync error: HTTP 500'))).toBe(
          true,
        );
        expect(queryByRole('alertdialog')).toBeNull();
        expect((getByText('Disconnect…') as HTMLButtonElement).disabled).toBe(false);
      },
    },
    {
      label: 'cancel',
      setupMocks: () => {},
      checkDialogBefore: true,
      checkDialogAfter: true,
      confirmButtonText: 'Cancel',
      checkOutcome: (
        _getByText: (t: string) => HTMLElement,
        _queryByRole: (r: string) => HTMLElement | null,
      ) => {
        expect(fakeApi.googleDisconnect).not.toHaveBeenCalled();
      },
    },
  ])(
    'Disconnect: $label',
    async ({
      setupMocks,
      checkDialogBefore,
      checkDialogAfter,
      confirmButtonText,
      checkOutcome,
    }) => {
      setupMocks();
      const { getByText, queryByRole } = await mountAndFlush();

      if (checkDialogBefore) expect(queryByRole('alertdialog')).toBeNull();

      await fireEvent.click(getByText('Disconnect…'));
      await flush();

      if (checkDialogAfter) expect(queryByRole('alertdialog')).toBeTruthy();

      await fireEvent.click(getByText(confirmButtonText));
      await flush();

      checkOutcome(getByText, queryByRole);
    },
  );
});

describe('SettingsView — checkbox toggle', () => {
  beforeEach(() => {
    mockConnected({ calendars: [PRIMARY_CAL, SECONDARY_CAL], pull: [PRIMARY_CAL.id] });
  });

  it.each([
    {
      label: 'happy path',
      setupMocks: () => fakeApi.setPullCalendars.mockResolvedValue(undefined),
      flushCount: 1,
      checkOutcome: () => {
        expect(fakeApi.setPullCalendars).toHaveBeenCalledWith(
          expect.arrayContaining([PRIMARY_CAL.id, SECONDARY_CAL.id]),
        );
      },
    },
    {
      label: 'failure',
      setupMocks: () => {
        fakeApi.setPullCalendars.mockRejectedValue({
          error: 'calendar_sync',
          message: 'Calendar sync error: save failed',
        });
        fakeApi.getPullCalendars
          .mockResolvedValueOnce(['cal-primary'])
          .mockResolvedValue(['cal-primary']);
      },
      flushCount: 2,
      checkOutcome: () => {
        expect(toastState.items.some((t) => t.text.includes('Calendar sync error'))).toBe(true);
        expect(fakeApi.getPullCalendars).toHaveBeenCalledTimes(INITIAL_CALL_PLUS_ONE);
      },
    },
  ])('checkbox toggle: $label', async ({ setupMocks, flushCount, checkOutcome }) => {
    setupMocks();
    await mountAndFlush();
    const secondaryBox = calendarCheckbox('Work')!;
    await fireEvent.click(secondaryBox);
    for (let i = 0; i < flushCount; i++) await flush();
    checkOutcome();
  });
});

describe('SettingsView — Sync now', () => {
  beforeEach(() => {
    mockConnected();
  });

  it.each<
    [number | null, string, { created: number; updated: number; deleted: number } | null, boolean]
  >([
    [
      1,
      '1 chunk scheduled, 0 Google events updated',
      { created: 0, updated: 0, deleted: 0 },
      false,
    ],
    [
      2,
      '2 chunks scheduled, 3 Google events updated',
      { created: 1, updated: 1, deleted: 1 },
      false,
    ],
    [null, 'Calendar sync error: HTTP 503', null, true],
  ])('syncNow count=%s: %s', async (count, expectedText, pushed, shouldFail) => {
    if (shouldFail) {
      fakeApi.syncNow.mockRejectedValue({
        error: 'calendar_sync',
        message: 'Calendar sync error: HTTP 503',
      });
    } else {
      fakeApi.syncNow.mockResolvedValue({
        schedule: {
          placed_chunks: Array.from({ length: count! }, (_, i) => makeChunk(`c${i}`)),
          warnings: [
            {
              task_id: 'task-warn',
              task_title: 'Overdue Task',
              kind: { Unschedulable: { reason: 'no windows' } },
            },
          ],
        },
        pushed: pushed!,
      });
    }

    const { getByText } = await mountAndFlush();

    await fireEvent.click(getByText('Sync now'));
    await flush();
    await flush();

    if (shouldFail) {
      expect(toastState.items.some((t) => t.text.includes('Calendar sync error: HTTP 503'))).toBe(
        true,
      );
      expect((getByText('Sync now') as HTMLButtonElement).disabled).toBe(false);
      expect(warningState.items).toHaveLength(0);
    } else {
      expect(fakeApi.syncNow).toHaveBeenCalledOnce();
      expect(warningState.items).toHaveLength(1);
      expect(warningState.items[0].task_id).toBe('task-warn');
      expect(toastState.items.some((t) => t.text.includes(expectedText))).toBe(true);
    }
    expect(fakeApi.getSyncStatus).toHaveBeenCalledTimes(INITIAL_CALL_PLUS_ONE);
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

    expect(fakeApi.googleListCalendars).toHaveBeenCalledTimes(INITIAL_CALL_PLUS_ONE);
  });
});

describe('SettingsView — OAuth client credentials', () => {
  beforeEach(() => {
    fakeApi.googleAuthStatus.mockResolvedValue({ type: 'not_connected' });
  });

  it.each([
    {
      credentialsSaved: false,
      checkUI: (utils: Awaited<ReturnType<typeof mountAndFlush>>) => {
        expect(utils.getByLabelText('Client ID')).toBeTruthy();
        expect(utils.getByLabelText('Client Secret')).toBeTruthy();
        expect(utils.getByText('Save')).toBeTruthy();
      },
    },
    {
      credentialsSaved: true,
      checkUI: (utils: Awaited<ReturnType<typeof mountAndFlush>>) => {
        expect(utils.getByText('OAuth app credentials configured.')).toBeTruthy();
        expect(utils.getByText('Change')).toBeTruthy();
        expect(utils.queryByLabelText('Client ID')).toBeNull();
      },
    },
  ])(
    'credential form visibility: saved=$credentialsSaved',
    async ({ credentialsSaved, checkUI }) => {
      fakeApi.googleClientCredentialsSaved.mockResolvedValue(credentialsSaved);
      const utils = await mountAndFlush(FLUSHES_SINGLE_LAYER);
      checkUI(utils);
    },
  );

  it('shows the form after clicking Change', async () => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(true);

    const { getByText, getByLabelText } = await mountAndFlush(FLUSHES_SINGLE_LAYER);

    await fireEvent.click(getByText('Change'));
    await tick();

    expect(getByLabelText('Client ID')).toBeTruthy();
    expect(getByText('Cancel')).toBeTruthy();
  });

  it.each<{
    label: string;
    setupMocks: () => void;
    checkOutcome: (utils: Awaited<ReturnType<typeof mountAndFlush>>) => void;
  }>([
    {
      label: 'success',
      setupMocks: () => {},
      checkOutcome: (utils) => {
        expect(fakeApi.saveGoogleClientCredentials).toHaveBeenCalledWith(
          'my-id.apps.googleusercontent.com',
          'GOCSPX-secret',
        );
        expect(utils.queryByLabelText('Client ID')).toBeNull();
        expect(utils.getByText('OAuth app credentials configured.')).toBeTruthy();
        expect(toastState.items.some((t) => t.text.includes('OAuth credentials saved.'))).toBe(
          true,
        );
      },
    },
    {
      label: 'failure',
      setupMocks: () => {
        fakeApi.saveGoogleClientCredentials.mockRejectedValue({
          error: 'validation',
          message: 'client_id must not be empty',
        });
      },
      checkOutcome: (utils) => {
        expect(toastState.items.some((t) => t.text.includes('client_id must not be empty'))).toBe(
          true,
        );
        expect((utils.getByText('Save') as HTMLButtonElement).disabled).toBe(false);
      },
    },
  ])('credential save: $label', async ({ setupMocks, checkOutcome }) => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(false);
    setupMocks();
    const utils = await mountAndFlush(FLUSHES_SINGLE_LAYER);
    await fillAndSubmitCredForm(utils);
    checkOutcome(utils);
  });

  it.each([
    {
      label: 'empty client_id',
      clientId: '',
      clientSecret: 'GOCSPX-secret',
      expectedError: 'Client ID is required.',
    },
    {
      label: 'invalid client_id format',
      clientId: 'not-a-google-client-id',
      clientSecret: 'GOCSPX-secret',
      expectedError: GOOGLE_CLIENT_ID_SUFFIX,
    },
    {
      label: 'empty client_secret',
      clientId: 'my-id.apps.googleusercontent.com',
      clientSecret: '',
      expectedError: 'Client Secret is required.',
    },
  ])('form validation blocks submit: $label', async ({ clientId, clientSecret, expectedError }) => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(false);

    const { getByLabelText, getByText, getByRole } = await mountAndFlush(FLUSHES_SINGLE_LAYER);

    await fireEvent.input(getByLabelText('Client ID'), { target: { value: clientId } });
    await fireEvent.input(getByLabelText('Client Secret'), { target: { value: clientSecret } });
    const form = getByText('Save').closest('form')!;
    await fireEvent.submit(form);
    await tick();

    expect(getByRole('alert')).toBeTruthy();
    expect(
      getByText(new RegExp(expectedError.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))),
    ).toBeTruthy();
    expect(fakeApi.saveGoogleClientCredentials).not.toHaveBeenCalled();
  });

  it('typing in Client ID field clears its error', async () => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(false);

    const { getByLabelText, getByText, queryByRole } = await mountAndFlush(FLUSHES_SINGLE_LAYER);

    const form = getByText('Save').closest('form')!;
    await fireEvent.submit(form);
    await tick();
    expect(queryByRole('alert')).toBeTruthy();

    await fireEvent.input(getByLabelText('Client ID'), { target: { value: 'x' } });
    await tick();
    expect(queryByRole('alert')).toBeNull();
  });

  it('reconnect banner shows "Change credentials" button when credentials are saved', async () => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(true);
    mockConnected({
      syncStatus: { last_sync_at: null, last_sync_error: 'Calendar sync error: HTTP 401' },
    });

    const { getByText } = await mountAndFlush();

    expect(getByText('Change credentials')).toBeTruthy();
    expect(getByText(/OAuth app credentials may be invalid/)).toBeTruthy();
  });

  it('clicking Change credentials opens the credential form', async () => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(true);
    mockConnected({
      syncStatus: { last_sync_at: null, last_sync_error: 'Calendar sync error: HTTP 401' },
    });

    const { getByText, getByLabelText } = await mountAndFlush();

    await fireEvent.click(getByText('Change credentials'));
    await tick();

    expect(getByLabelText('Client ID')).toBeTruthy();
    expect(getByText('Cancel')).toBeTruthy();
  });

  it('reconnect banner hides while the credential form is open', async () => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(true);
    mockConnected({
      syncStatus: { last_sync_at: null, last_sync_error: 'Calendar sync error: HTTP 401' },
    });

    const { queryByRole, getByText, getByLabelText } = await mountAndFlush();

    expect(queryByRole('alert')).toBeTruthy();

    await fireEvent.click(getByText('Change credentials'));
    await tick();

    expect(queryByRole('alert')).toBeNull();
    expect(getByLabelText('Client ID')).toBeTruthy();
  });

  it('a successful save clears the stale sync error and reloads calendars', async () => {
    fakeApi.googleClientCredentialsSaved.mockResolvedValue(true);
    fakeApi.googleListCalendars
      .mockRejectedValueOnce({
        error: 'calendar_sync',
        message: 'Calendar sync error: token refresh failed with HTTP 401',
      })
      .mockResolvedValue([]);
    fakeApi.getPullCalendars.mockResolvedValue([]);
    mockConnected({
      syncStatus: { last_sync_at: null, last_sync_error: 'Calendar sync error: HTTP 401' },
    });

    const utils = await mountAndFlush();
    const { queryByRole, queryByText, getByText } = utils;

    expect(queryByText('Calendar sync error: token refresh failed with HTTP 401')).toBeTruthy();

    await fireEvent.click(getByText('Change credentials'));
    await tick();

    await fillAndSubmitCredForm(utils);
    await flush();

    expect(queryByRole('alert')).toBeNull();
    expect(queryByText('Calendar sync error: token refresh failed with HTTP 401')).toBeNull();
    expect(fakeApi.googleListCalendars).toHaveBeenCalledTimes(INITIAL_CALL_PLUS_ONE);
  });
});
