// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { open as pluginOpen, save as pluginSave } from '@tauri-apps/plugin-dialog';
import * as api from '../../api';
import type { BackupStatus } from '../../types';

export interface BackupSectionApi {
  getBackupStatus: () => Promise<BackupStatus>;
  setBackupEnabled: (enabled: boolean) => Promise<BackupStatus>;
  backupNow: () => Promise<BackupStatus>;
  exportBackupToFile: (path: string) => Promise<void>;
  importBackupFromFile: (path: string) => Promise<void>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
  backupErrorMessage: (e: unknown, fallback: string) => string;
}

export interface BackupSectionDialog {
  open: typeof pluginOpen;
  save: typeof pluginSave;
}

/** Delegates to the real api at call time so sibling tests' vi.mocked(api.*) are observed. */
export const defaultBackupSectionApi: BackupSectionApi = {
  getBackupStatus: () => api.getBackupStatus(),
  setBackupEnabled: (enabled) => api.setBackupEnabled(enabled),
  backupNow: () => api.backupNow(),
  exportBackupToFile: (path) => api.exportBackupToFile(path),
  importBackupFromFile: (path) => api.importBackupFromFile(path),
  apiErrorMessage: (e, fallback) => api.apiErrorMessage(e, fallback),
  backupErrorMessage: (e, fallback) => api.backupErrorMessage(e, fallback),
};

export const defaultBackupSectionDialog: BackupSectionDialog = {
  open: (options) => pluginOpen(options),
  save: (options) => pluginSave(options),
};
