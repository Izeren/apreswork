// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import * as api from '../../api';
import type { BackupStatus } from '../../types';

export interface ShellApi {
  getBackupStatus: () => Promise<BackupStatus>;
}

export const defaultShellApi: ShellApi = {
  getBackupStatus: () => api.getBackupStatus(),
};
