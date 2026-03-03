// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { toastState } from '../../stores/toast.svelte';
import { formatDateTime } from '../../utils';
import { apiErrorMessage, backupErrorMessage } from '../../api';
import type { BackupStatus } from '../../types';
import type { BackupSectionApi, BackupSectionDialog } from './backupSectionShared';

const DISCONNECTED: BackupStatus = {
  enabled: false,
  connected: false,
  last_export_at: null,
  last_backup_error: null,
  restored_this_run: null,
};

const CONNECTED: BackupStatus = { ...DISCONNECTED, connected: true };

const ENABLED: BackupStatus = {
  ...CONNECTED,
  enabled: true,
  last_export_at: '2026-07-12T10:00:00Z',
};

let mockApi: BackupSectionApi;
let mockDialog: BackupSectionDialog;

beforeEach(() => {
  // Fresh object per call (like real IPC) — reusing one instance would let
  // Svelte's referential-equality check skip re-renders on refetch.
  mockApi = {
    getBackupStatus: vi.fn().mockResolvedValue({ ...CONNECTED }),
    setBackupEnabled: vi.fn(),
    backupNow: vi.fn(),
    exportBackupToFile: vi.fn(),
    importBackupFromFile: vi.fn(),
    apiErrorMessage,
    backupErrorMessage,
  };
  mockDialog = {
    open: vi.fn(),
    save: vi.fn(),
  };
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  toastState.items = [];
});

// ---------------------------------------------------------------------------
// Dynamic component import (re-import per test to clear module-level state)
// ---------------------------------------------------------------------------

async function importBackupSection() {
  const mod = await import('./BackupSection.svelte');
  return mod.default;
}

/** Flush one level of microtask queue + Svelte reactivity. */
async function flush() {
  await Promise.resolve();
  await tick();
}

function toggleOf(container: HTMLElement): HTMLInputElement {
  const box = container.querySelector('input[type="checkbox"]');
  if (!(box instanceof HTMLInputElement)) throw new Error('toggle not rendered');
  return box;
}

/** Re-import BackupSection, render it with injected deps, flush the mount load. */
async function renderBackupSection() {
  const BackupSection = await importBackupSection();
  const result = render(BackupSection, { apiClient: mockApi, dialogClient: mockDialog });
  await flush();
  return result;
}

describe('BackupSection — mount', () => {
  it('renders the toggle and actions; never backed up shows "Not backed up yet."', async () => {
    const { container, getByText } = await renderBackupSection();

    const toggle = toggleOf(container);
    expect(toggle.checked).toBe(false);
    expect(toggle.disabled).toBe(false);
    expect(getByText('Not backed up yet.')).toBeTruthy();
    expect((getByText('Back up now') as HTMLButtonElement).disabled).toBe(false);
    expect(getByText('Export to file…')).toBeTruthy();
    expect(getByText('Import from file…')).toBeTruthy();
  });

  it('shows the last-backup time and a checked toggle when backup ran', async () => {
    mockApi.getBackupStatus = vi.fn().mockResolvedValue(ENABLED);

    const { container, getByText } = await renderBackupSection();

    expect(toggleOf(container).checked).toBe(true);
    expect(getByText(`Last backed up: ${formatDateTime('2026-07-12T10:00:00Z')}`)).toBeTruthy();
  });

  it('disconnected: toggle and "Back up now" disabled, connect hint shown', async () => {
    mockApi.getBackupStatus = vi.fn().mockResolvedValue(DISCONNECTED);

    const { container, getByText } = await renderBackupSection();

    expect(toggleOf(container).disabled).toBe(true);
    expect((getByText('Back up now') as HTMLButtonElement).disabled).toBe(true);
    expect(getByText('Connect Google Calendar above to enable Drive backup.')).toBeTruthy();
  });

  it('renders the persisted backup error as an alert', async () => {
    mockApi.getBackupStatus = vi.fn().mockResolvedValue({
      ...ENABLED,
      last_backup_error: 'Backup on Drive is newer — restart to pull it.',
    });

    const { getByRole } = await renderBackupSection();

    expect(getByRole('alert').textContent).toContain('restart to pull it');
  });

  it('load failure shows an alert; Retry re-fetches and recovers', async () => {
    mockApi.getBackupStatus = vi
      .fn()
      .mockRejectedValueOnce(new Error('io error'))
      .mockResolvedValue(CONNECTED);

    const { getByRole, getByText } = await renderBackupSection();

    expect(getByRole('alert').textContent).toContain('Could not load backup status.');

    await fireEvent.click(getByText('Retry'));
    await flush();

    expect(mockApi.getBackupStatus).toHaveBeenCalledTimes(2);
    expect(getByText('Not backed up yet.')).toBeTruthy();
  });
});

describe('BackupSection — enable toggle', () => {
  it('happy path: setBackupEnabled(true) called; toggle reflects the fresh status', async () => {
    mockApi.setBackupEnabled = vi.fn().mockResolvedValue({ ...CONNECTED, enabled: true });

    const { container } = await renderBackupSection();

    await fireEvent.click(toggleOf(container));
    await flush();

    expect(mockApi.setBackupEnabled).toHaveBeenCalledWith(true);
    expect(toggleOf(container).checked).toBe(true);
  });

  it('failure: backup message surfaces in a toast and the status is re-fetched', async () => {
    mockApi.setBackupEnabled = vi.fn().mockRejectedValue({
      error: 'backup',
      message: 'Backup error: Drive upload failed (HTTP 500)',
    });

    const { container } = await renderBackupSection();

    await fireEvent.click(toggleOf(container));
    // Two flushes: setBackupEnabled.catch, then the revert load().
    await flush();
    await flush();

    expect(toastState.items.some((t) => t.text.includes('Drive upload failed'))).toBe(true);
    // Mount load + revert load.
    expect(mockApi.getBackupStatus).toHaveBeenCalledTimes(2);
    expect(toggleOf(container).checked).toBe(false);
  });
});

describe('BackupSection — back up now', () => {
  it('success: backupNow called once, success toast shown', async () => {
    mockApi.backupNow = vi.fn().mockResolvedValue(ENABLED);

    const { getByText } = await renderBackupSection();

    await fireEvent.click(getByText('Back up now'));
    await flush();

    expect(mockApi.backupNow).toHaveBeenCalledOnce();
    expect(toastState.items.some((t) => t.text === 'Backed up to Google Drive.')).toBe(true);
  });

  it('stale-writer skip: the recorded error is toasted and rendered as an alert', async () => {
    const guard = 'Backup on Drive is newer — restart to pull it.';
    mockApi.backupNow = vi.fn().mockResolvedValue({ ...ENABLED, last_backup_error: guard });

    const { getByRole, getByText } = await renderBackupSection();

    await fireEvent.click(getByText('Back up now'));
    await flush();

    expect(toastState.items.some((t) => t.level === 'error' && t.text === guard)).toBe(true);
    expect(getByRole('alert').textContent).toContain(guard);
  });

  it('failure: backup message toasted, status refreshed, button re-enabled', async () => {
    mockApi.backupNow = vi.fn().mockRejectedValue({
      error: 'backup',
      message: 'Backup error: network error',
    });

    const { getByText } = await renderBackupSection();

    await fireEvent.click(getByText('Back up now'));
    // Two flushes: backupNow.catch, then the refresh load().
    await flush();
    await flush();

    expect(toastState.items.some((t) => t.text.includes('network error'))).toBe(true);
    expect(mockApi.getBackupStatus).toHaveBeenCalledTimes(2);
    expect((getByText('Back up now') as HTMLButtonElement).disabled).toBe(false);
  });
});

interface ExportCase {
  name: string;
  saveMock: () => Promise<string | null>;
  exportMock?: () => Promise<void>;
  expectExportCalledWith?: string;
  expectedToast?: string;
}

const exportCases: ExportCase[] = [
  {
    name: 'save dialog cancelled',
    saveMock: () => Promise.resolve(null),
  },
  {
    name: 'success',
    saveMock: () => Promise.resolve('/home/user/apreswork-backup.zip'),
    exportMock: () => Promise.resolve(undefined),
    expectExportCalledWith: '/home/user/apreswork-backup.zip',
    expectedToast: 'Backup exported.',
  },
  {
    name: 'export failure',
    saveMock: () => Promise.resolve('/home/user/apreswork-backup.zip'),
    exportMock: () =>
      Promise.reject({ error: 'backup', message: 'Backup error: could not write the archive' }),
    expectExportCalledWith: '/home/user/apreswork-backup.zip',
    expectedToast: 'could not write the archive',
  },
  {
    name: 'save dialog failure',
    saveMock: () => Promise.reject(new Error('no display')),
    expectedToast: 'Could not open the save dialog.',
  },
];

describe('BackupSection — export to file', () => {
  it.each(exportCases)(
    '$name',
    async ({ saveMock, exportMock, expectExportCalledWith, expectedToast }) => {
      mockDialog.save = vi.fn().mockImplementation(saveMock);
      if (exportMock !== undefined) {
        mockApi.exportBackupToFile = vi.fn().mockImplementation(exportMock);
      }

      const { getByText } = await renderBackupSection();

      await fireEvent.click(getByText('Export to file…'));
      await flush();
      if (exportMock !== undefined) await flush();

      if (expectExportCalledWith !== undefined) {
        expect(mockApi.exportBackupToFile).toHaveBeenCalledWith(expectExportCalledWith);
      } else {
        expect(mockApi.exportBackupToFile).not.toHaveBeenCalled();
      }

      if (expectedToast !== undefined) {
        expect(toastState.items.some((t) => t.text.includes(expectedToast))).toBe(true);
      } else {
        expect(toastState.items).toHaveLength(0);
      }
    },
  );
});

describe('BackupSection — import from file', () => {
  it('file dialog cancelled: no confirmation dialog', async () => {
    mockDialog.open = vi.fn().mockResolvedValue(null);

    const { getByText, queryByRole } = await renderBackupSection();

    await fireEvent.click(getByText('Import from file…'));
    await flush();

    expect(queryByRole('alertdialog')).toBeNull();
    expect(mockApi.importBackupFromFile).not.toHaveBeenCalled();
  });

  it('file dialog failure: dialog toast shown, no confirmation dialog', async () => {
    mockDialog.open = vi.fn().mockRejectedValue(new Error('no display'));

    const { getByText, queryByRole } = await renderBackupSection();

    await fireEvent.click(getByText('Import from file…'));
    await flush();

    expect(queryByRole('alertdialog')).toBeNull();
    expect(toastState.items.some((t) => t.text === 'Could not open the file dialog.')).toBe(true);
  });

  it('confirm: destructive dialog opens, then the picked file is imported', async () => {
    mockDialog.open = vi.fn().mockResolvedValue('/home/user/old-backup.zip');
    mockApi.importBackupFromFile = vi.fn().mockReturnValue(new Promise(() => {})); // restarts

    const { getByText, queryByRole } = await renderBackupSection();

    await fireEvent.click(getByText('Import from file…'));
    await flush();

    expect(queryByRole('alertdialog')).toBeTruthy();

    await fireEvent.click(getByText('Import and restart'));
    await flush();

    expect(mockApi.importBackupFromFile).toHaveBeenCalledWith('/home/user/old-backup.zip');
    expect(queryByRole('alertdialog')).toBeNull();
  });

  it('cancel: nothing imported, dialog closed', async () => {
    mockDialog.open = vi.fn().mockResolvedValue('/home/user/old-backup.zip');

    const { getByText, queryByRole } = await renderBackupSection();

    await fireEvent.click(getByText('Import from file…'));
    await flush();
    await fireEvent.click(getByText('Cancel'));
    await flush();

    expect(mockApi.importBackupFromFile).not.toHaveBeenCalled();
    expect(queryByRole('alertdialog')).toBeNull();
  });

  it('failure: backup message toasted, button re-enabled', async () => {
    mockDialog.open = vi.fn().mockResolvedValue('/home/user/old-backup.zip');
    mockApi.importBackupFromFile = vi.fn().mockRejectedValue({
      error: 'backup',
      message: 'Backup error: the archive has no database entry',
    });

    const { getByText } = await renderBackupSection();

    await fireEvent.click(getByText('Import from file…'));
    await flush();
    await fireEvent.click(getByText('Import and restart'));
    await flush();

    expect(toastState.items.some((t) => t.text.includes('no database entry'))).toBe(true);
    expect((getByText('Import from file…') as HTMLButtonElement).disabled).toBe(false);
  });
});
