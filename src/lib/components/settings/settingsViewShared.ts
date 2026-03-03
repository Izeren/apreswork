// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import * as api from '../../api';
import type { AuthStatus, ExternalCalendar, SyncOutcome, SyncStatus } from '../../types';

export interface SettingsViewApi {
  googleAuthStatus: () => Promise<AuthStatus>;
  beginGoogleAuth: () => Promise<string>;
  openExternalUrl: (url: string) => Promise<void>;
  googleListCalendars: () => Promise<ExternalCalendar[]>;
  getPullCalendars: () => Promise<string[]>;
  setPullCalendars: (calendarIds: string[]) => Promise<void>;
  googleDisconnect: () => Promise<void>;
  getSyncStatus: () => Promise<SyncStatus>;
  syncNow: () => Promise<SyncOutcome>;
  syncErrorMessage: (e: unknown, fallback: string) => string;
}

export const defaultSettingsViewApi: SettingsViewApi = {
  googleAuthStatus: api.googleAuthStatus,
  beginGoogleAuth: api.beginGoogleAuth,
  openExternalUrl: api.openExternalUrl,
  googleListCalendars: api.googleListCalendars,
  getPullCalendars: api.getPullCalendars,
  setPullCalendars: api.setPullCalendars,
  googleDisconnect: api.googleDisconnect,
  getSyncStatus: api.getSyncStatus,
  syncNow: api.syncNow,
  syncErrorMessage: api.syncErrorMessage,
};
