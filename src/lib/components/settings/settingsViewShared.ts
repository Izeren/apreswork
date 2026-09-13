// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import * as api from '../../api';
import type { AuthStatus, ExternalCalendar, SyncOutcome, SyncStatus } from '../../types';

export interface SettingsViewApi {
  googleClientCredentialsSaved: () => Promise<boolean>;
  saveGoogleClientCredentials: (clientId: string, clientSecret: string) => Promise<void>;
  googleAuthStatus: () => Promise<AuthStatus>;
  beginGoogleAuth: () => Promise<string>;
  openExternalUrl: (url: string) => Promise<void>;
  googleListCalendars: () => Promise<ExternalCalendar[]>;
  getPullCalendars: () => Promise<string[]>;
  setPullCalendars: (calendarIds: string[]) => Promise<void>;
  googleDisconnect: () => Promise<void>;
  getSyncStatus: () => Promise<SyncStatus>;
  clearSyncError: () => Promise<void>;
  syncNow: () => Promise<SyncOutcome>;
  syncErrorMessage: (e: unknown, fallback: string) => string;
}

export const GOOGLE_CLIENT_ID_SUFFIX = '.apps.googleusercontent.com';

export const POLL_INTERVAL_MS = 2000;
export const POLL_MAX_TICKS = 150;

export const defaultSettingsViewApi: SettingsViewApi = {
  googleClientCredentialsSaved: api.googleClientCredentialsSaved,
  saveGoogleClientCredentials: api.saveGoogleClientCredentials,
  googleAuthStatus: api.googleAuthStatus,
  beginGoogleAuth: api.beginGoogleAuth,
  openExternalUrl: api.openExternalUrl,
  googleListCalendars: api.googleListCalendars,
  getPullCalendars: api.getPullCalendars,
  setPullCalendars: api.setPullCalendars,
  googleDisconnect: api.googleDisconnect,
  getSyncStatus: api.getSyncStatus,
  clearSyncError: api.clearSyncError,
  syncNow: api.syncNow,
  syncErrorMessage: api.syncErrorMessage,
};
