// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import * as api from '../../api';
import type { AppConfig, UpdateConfigInput } from '../../types';

export interface SchedulingSectionApi {
  getConfig: () => Promise<AppConfig>;
  updateConfig: (config: UpdateConfigInput) => Promise<AppConfig>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
}

export const defaultSchedulingSectionApi: SchedulingSectionApi = {
  getConfig: api.getConfig,
  updateConfig: api.updateConfig,
  apiErrorMessage: api.apiErrorMessage,
};
