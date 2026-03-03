// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Concrete calendar-sync providers.
//!
//! [`providers_from_config`] is the composition-root helper that selects the
//! active calendar-sync provider AND the matching backup target based on the
//! `sync_provider` config key and the availability of compiled-in
//! credentials. One selection policy: the pair always agrees on the account.

pub mod google;
pub(crate) mod google_http;
pub mod google_token;
pub mod noop;

use std::path::Path;
use std::sync::Arc;

use crate::backup::{google_drive::GoogleDriveBackup, noop::NoopBackupTarget};
use crate::traits::backup::BackupTarget;
use crate::traits::calendar_sync::CalendarSync;

fn noop_providers() -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    (Arc::new(noop::NoopCalendarSync), Arc::new(NoopBackupTarget))
}

/// Select the active [`CalendarSync`] provider and its [`BackupTarget`].
///
/// Rules:
/// - `sync_provider == "google"` AND compiled-in credentials present
///   → [`google::GoogleCalendarSync`] + [`GoogleDriveBackup`] sharing one
///   client (same account, same token file).
/// - Anything else (missing key, `"none"`, unknown provider, absent creds)
///   → [`noop::NoopCalendarSync`] + [`NoopBackupTarget`] with a one-line info
///   log explaining why.
#[must_use]
pub fn providers_from_config(
    sync_provider: Option<&str>,
    creds: Option<google::GoogleCredentials>,
    token_path: &Path,
) -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    match (sync_provider, creds) {
        (Some("google"), Some(c)) => match google::GoogleCalendarSync::new(c, token_path) {
            Ok(s) => {
                log::info!("calendar: using Google Calendar sync provider");
                let sync = Arc::new(s);
                let backup = Arc::new(GoogleDriveBackup::new(sync.clone()));
                (sync, backup)
            }
            Err(e) => {
                log::error!("calendar: cannot initialise Google Calendar provider ({e}) — falling back to noop");
                noop_providers()
            }
        },
        (Some("google"), None) => {
            log::info!("calendar: sync_provider is 'google' but no compiled Google credentials — using noop");
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
        (Some("google"), Some(c)) => {
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
        google::GoogleCredentials, google_token, providers_from_config, providers_with_mock_keyring,
    };
    use crate::calendar::google_http::test_support::mock_keyring;
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

    #[test_case(Some("google"), true, true ; "google_with_creds_uses_google")]
    #[test_case(Some("google"), false, false ; "google_no_creds_uses_noop")]
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
    fn providers_from_config_google_arm_selects_google() {
        // Exercises the production new() path for coverage.
        // Reaches the real Secret Service D-Bus IPC, but safely: the username is
        // derived from a unique tempdir basename, Entry::new does not connect
        // (only get_secret does), and search_items finds nothing so no unlock
        // prompt fires.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("google_auth.json");
        let (calendar, _) = providers_from_config(Some("google"), Some(dummy_creds()), &path);
        assert!(
            is_google(&calendar),
            "providers_from_config with google creds must select Google provider"
        );
    }
}
