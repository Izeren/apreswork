// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Concrete calendar-sync providers.
//!
//! Composition-root helper that selects the active calendar-sync provider and
//! matching backup target. Selection is based on the `sync_provider` config
//! key and the availability of resolved credentials.
//! One selection policy: the pair always agrees on the account.

pub mod google;
pub(crate) mod google_client_creds;
pub(crate) mod google_http;
pub mod google_token;
pub mod noop;

use std::path::Path;
use std::sync::Arc;

use crate::backup::{google_drive::GoogleDriveBackup, noop::NoopBackupTarget};
use crate::error::AppError;
use crate::traits::backup::BackupTarget;
use crate::traits::calendar_sync::CalendarSync;

const SYNC_PROVIDER_GOOGLE: &str = "google";

fn noop_providers() -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    (Arc::new(noop::NoopCalendarSync), Arc::new(NoopBackupTarget))
}

/// The `from_keyring_fn` parameter is injected so callers can test
/// resolution without touching the OS keyring.
pub(crate) fn resolve_client_creds(
    from_keyring_fn: impl FnOnce() -> Result<Option<google::GoogleCredentials>, AppError>,
) -> Option<google::GoogleCredentials> {
    match google::GoogleCredentials::env_or_keyring(from_keyring_fn) {
        Ok(creds) => creds,
        Err(e) => {
            log::warn!("calendar: keyring read for client credentials failed: {e}");
            None
        }
    }
}

fn google_arm_from_sync(
    sync_result: Result<google::GoogleCalendarSync, AppError>,
) -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    match sync_result {
        Ok(s) => {
            log::info!("calendar: using Google Calendar sync provider");
            let sync = Arc::new(s);
            let backup = Arc::new(GoogleDriveBackup::new(sync.clone()));
            (sync, backup)
        }
        Err(e) => {
            log::error!(
                "calendar: cannot initialise Google Calendar provider ({e}) — falling back to noop"
            );
            noop_providers()
        }
    }
}

/// When using Google provider and credentials, providers share one client (same account, same token file).
#[must_use]
pub fn providers_from_config(
    sync_provider: Option<&str>,
    creds: Option<google::GoogleCredentials>,
    token_path: &Path,
) -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    match (sync_provider, creds) {
        (Some(SYNC_PROVIDER_GOOGLE), Some(c)) => {
            google_arm_from_sync(google::GoogleCalendarSync::new(c, token_path))
        }
        (Some(SYNC_PROVIDER_GOOGLE), None) => {
            log::info!("calendar: sync_provider is 'google' but no Google credentials available — using noop");
            noop_providers()
        }
        (Some(other), _) => {
            log::info!("calendar: unknown sync_provider '{other}' — using noop");
            noop_providers()
        }
        (None, _) => {
            log::info!("calendar: no sync_provider configured — using noop");
            noop_providers()
        }
    }
}

/// Select providers using a pre-built keyring (test only — avoids OS keyring IPC).
#[cfg(test)]
pub(crate) fn providers_with_mock_keyring(
    sync_provider: Option<&str>,
    creds: Option<google::GoogleCredentials>,
    keyring: google_token::KeyringStore,
) -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    match (sync_provider, creds) {
        (Some(SYNC_PROVIDER_GOOGLE), Some(c)) => {
            let s = google::GoogleCalendarSync::new_with_mock_keyring(c, keyring);
            let sync = Arc::new(s);
            let backup = Arc::new(GoogleDriveBackup::new(sync.clone()));
            (sync, backup)
        }
        _ => noop_providers(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;
    use test_case::test_case;

    use super::{
        google::GoogleCredentials, google_arm_from_sync, google_token, providers_from_config,
        providers_with_mock_keyring, resolve_client_creds, SYNC_PROVIDER_GOOGLE,
    };
    use crate::calendar::google_http::test_support::mock_keyring;
    use crate::error::AppError;
    use crate::test_support::test_now;
    use crate::traits::calendar_sync::CalendarSync;

    fn dummy_creds() -> GoogleCredentials {
        GoogleCredentials {
            client_id: "ci".to_owned(),
            client_secret: "cs".to_owned(),
        }
    }

    fn is_google(provider: &Arc<dyn CalendarSync>) -> bool {
        provider
            .begin_auth(test_now(), crate::test_support::test_instant_now())
            .is_ok()
    }

    #[test_case(Some(SYNC_PROVIDER_GOOGLE), true, true ; "google_with_creds_uses_google")]
    #[test_case(Some(SYNC_PROVIDER_GOOGLE), false, false ; "google_no_creds_uses_noop")]
    #[test_case(Some("none"), true, false ; "none_provider_uses_noop")]
    #[test_case(None, true, false ; "missing_provider_uses_noop")]
    #[test_case(Some("outlook"), true, false ; "unknown_provider_uses_noop")]
    fn provider_selection(sync_provider: Option<&str>, has_creds: bool, want_google: bool) {
        let keyring: google_token::KeyringStore = mock_keyring();
        let creds = if has_creds { Some(dummy_creds()) } else { None };
        let (calendar, backup) = providers_with_mock_keyring(sync_provider, creds, keyring);
        assert_eq!(
            is_google(&calendar),
            want_google,
            "expected is_google={want_google} for sync_provider={sync_provider:?}"
        );
        // The pair agrees: a Drive target probes (and fails without a stored
        // token — no network is reached), the noop target reads Ok(None).
        assert_eq!(
            backup
                .get_meta(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
                .is_err(),
            want_google,
            "backup target must match the calendar provider"
        );
    }

    #[test]
    fn resolve_client_creds_returns_none_on_keyring_error() {
        let result =
            resolve_client_creds(|| Err(AppError::CalendarSync("keyring unavailable".into())));
        assert!(result.is_none(), "keyring error must yield None");
    }

    #[test]
    fn resolve_client_creds_returns_keyring_creds_when_present() {
        let result = resolve_client_creds(|| {
            Ok(Some(GoogleCredentials {
                client_id: "kring-id".to_owned(),
                client_secret: "kring-sec".to_owned(),
            }))
        });
        assert_eq!(result.map(|c| c.client_id).as_deref(), Some("kring-id"));
    }

    #[test]
    fn providers_from_config_google_arm_selects_google() {
        // Reaches the real Secret Service D-Bus IPC, but safely: the username is
        // derived from a unique tempdir basename, Entry::new does not connect
        // (only get_secret does), and search_items finds nothing so no unlock
        // prompt fires.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("google_auth.json");
        let (calendar, _) =
            providers_from_config(Some(SYNC_PROVIDER_GOOGLE), Some(dummy_creds()), &path);
        assert!(
            is_google(&calendar),
            "providers_from_config with google creds must select Google provider"
        );
    }

    #[test]
    fn google_arm_from_sync_error_falls_back_to_noop() {
        let (calendar, _) =
            google_arm_from_sync(Err(AppError::CalendarSync("injected sync failure".into())));
        assert!(
            !is_google(&calendar),
            "a sync construction failure must fall back to the noop provider"
        );
    }
}
