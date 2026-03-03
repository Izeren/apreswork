// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Token persistence for the Google provider.
//!
//! Refresh tokens are stored in the OS keyring (Secret Service on Linux,
//! Credential Manager on Windows) via [`KeyringStore`].
//! Access tokens are memory-only and never written to disk by this code.
//! Token contents are NEVER logged or included in error messages.
//!
//! [`StoredToken`] is the transient token returned by the OAuth exchange worker
//! (production code). [`TokenFile`] is the legacy on-disk persistence type,
//! compiled only in test builds (`#[cfg(test)]`) to support `google_token`
//! unit tests; production code uses [`KeyringStore`] exclusively.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Credential data persisted in the OS keyring.
///
/// Serialized as a JSON blob stored under the (service, username) keyring key.
/// The `access_token` field from the legacy `google_auth.json` format is not
/// present — access tokens are memory-only.
///
/// # Security
///
/// No `Debug` derive — avoids accidental exposure via `{:?}` in log macros.
#[derive(Serialize, Deserialize, Clone)]
pub struct PersistedCredential {
    /// Long-lived refresh token (Google sometimes omits it on re-consent).
    pub refresh_token: Option<String>,
    /// Absolute expiry of the most recently issued access token (UTC).
    pub expires_at: DateTime<Utc>,
}

/// OS keyring handle for the Google refresh token.
///
/// Production instances come from [`KeyringStore::for_token_path`]; test
/// instances from [`KeyringStore::with_mock_entry`]. Both expose the same
/// `load` / `save` / `delete` API.
#[derive(Clone)]
pub struct KeyringStore {
    entry: Arc<keyring::Entry>,
}

/// Derive the (service, username) keyring key from a token file path.
///
/// The profile id is the parent directory's basename, giving each profile its
/// own keyring entry without threading the profile id through the call stack.
/// **This is the single key-derivation policy — all call sites must use it.**
pub(crate) fn keyring_key(path: &Path) -> (String, String) {
    let profile_id = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        // "default" is unreachable with the current profile path structure
        // (profiles/<uuid>/google_auth.json always has a usable parent basename).
        .unwrap_or("default");
    (
        "com.apreswork.app".to_owned(),
        format!("google-oauth:{profile_id}"),
    )
}

/// User-actionable message for a [`keyring::Error`].
///
/// The underlying OS error is discarded — it can contain storage-layer details
/// that might reveal credential context in adversarial scenarios. Only the
/// actionable guidance is returned.
fn keyring_error_message(err: &keyring::Error) -> String {
    match err {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            "keyring unavailable — ensure Secret Service / kwallet (Linux), \
             Keychain (macOS), or Credential Manager (Windows) is running \
             and this app has permission to access it"
                .to_owned()
        }
        keyring::Error::NoEntry => "keyring: no entry found".to_owned(),
        _ => "keyring error (details omitted to protect credentials)".to_owned(),
    }
}

fn map_keyring_error(err: &keyring::Error) -> AppError {
    AppError::CalendarSync(keyring_error_message(err))
}

impl KeyringStore {
    /// Production constructor: derives the keyring key from `token_path`.
    ///
    /// The parent directory basename is the profile id (single policy).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] if the platform keyring rejects the
    /// service or username values (e.g., exceeds length limits).
    pub fn for_token_path(token_path: &Path) -> Result<Self, AppError> {
        let (service, username) = keyring_key(token_path);
        let entry = keyring::Entry::new(&service, &username).map_err(|e| map_keyring_error(&e))?;
        Ok(Self {
            entry: Arc::new(entry),
        })
    }

    /// Test constructor: accepts a pre-built [`keyring::Entry`] (typically
    /// wrapping a [`keyring::mock::MockCredential`]).
    ///
    /// Sharing the same `Arc` across multiple store instances gives
    /// cross-instance persistence: a save via one store is visible to a load
    /// via another, mirroring how the OS keyring works in production.
    #[cfg(test)]
    pub(crate) fn with_mock_entry(entry: Arc<keyring::Entry>) -> Self {
        Self { entry }
    }

    /// Returns `Ok(None)` when no credential has been stored yet.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] when the keyring is unavailable or
    /// the stored blob cannot be deserialized.
    pub fn load(&self) -> Result<Option<PersistedCredential>, AppError> {
        let raw = match self.entry.get_password() {
            Ok(s) => s,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(e) => return Err(map_keyring_error(&e)),
        };
        serde_json::from_str::<PersistedCredential>(&raw)
            .map(Some)
            .map_err(|_| {
                AppError::CalendarSync(
                    "stored credential is unreadable — disconnect and reconnect Google Calendar"
                        .to_owned(),
                )
            })
    }

    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] when the keyring is unavailable.
    pub fn save(&self, cred: &PersistedCredential) -> Result<(), AppError> {
        let json = serde_json::to_string(cred)
            .map_err(|_| AppError::CalendarSync("cannot serialize credential".to_owned()))?;
        self.entry
            .set_password(&json)
            .map_err(|e| map_keyring_error(&e))
    }

    /// Delete the stored credential.
    ///
    /// Idempotent: returns `Ok(())` when no credential exists.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] when the keyring is unavailable.
    pub fn delete(&self) -> Result<(), AppError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_error(&e)),
        }
    }
}

/// Transient token returned by the OAuth exchange worker.
///
/// No `Debug` derive — avoids accidental token exposure via `{:?}`.
/// `Clone` is provided so the caller can capture the refresh token before
/// moving the struct into `finish_flow`.
#[derive(Serialize, Deserialize, Clone)]
pub struct StoredToken {
    pub access_token: String,
    /// Long-lived refresh token (Google sometimes omits it on re-consent).
    pub refresh_token: Option<String>,
    /// Absolute expiry (UTC). Compare against `Utc::now() + 60s` margin.
    pub expires_at: DateTime<Utc>,
}

/// Thin wrapper around the token file path — **test-only legacy format**.
#[cfg(test)]
#[derive(Clone)]
pub struct TokenFile {
    pub(crate) path: std::path::PathBuf,
}

#[cfg(test)]
impl TokenFile {
    #[must_use]
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }

    /// Returns `Ok(None)` when the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] when the file exists but cannot be
    /// read or contains invalid JSON. The error message contains only path and
    /// kind context — no token content is ever included.
    pub fn load(&self) -> Result<Option<StoredToken>, AppError> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => serde_json::from_str::<StoredToken>(&contents)
                .map(Some)
                .map_err(|_| {
                    AppError::CalendarSync(format!(
                        "stored token file is unreadable — disconnect and reconnect \
                         (path: {})",
                        self.path.display()
                    ))
                }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AppError::CalendarSync(format!(
                "cannot read token file {}: {}",
                self.path.display(),
                e.kind()
            ))),
        }
    }

    /// Persist `token` to disk.
    ///
    /// On unix, the file is opened with mode `0600` at open time (before any
    /// content is written) via [`OpenOptionsExt::mode`]. On non-unix platforms
    /// a plain create is used (per-user `%APPDATA%` ACLs protect the directory,
    /// per DESIGN.md §8.3).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on write failure (path + kind only).
    pub fn save(&self, token: &StoredToken) -> Result<(), AppError> {
        let contents = serde_json::to_string(token)
            .map_err(|e| AppError::CalendarSync(format!("cannot serialize token: {e}")))?;
        self.write_contents(contents.as_bytes())
    }

    /// Delete the token file.
    ///
    /// Idempotent: returns `Ok(())` when the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] for any deletion failure other than
    /// `NotFound`.
    pub fn delete(&self) -> Result<(), AppError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::CalendarSync(format!(
                "cannot delete token file {}: {}",
                self.path.display(),
                e.kind()
            ))),
        }
    }

    #[cfg(unix)]
    fn write_contents(&self, data: &[u8]) -> Result<(), AppError> {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            // Mode is set at open time — before any content is written — per
            // the plan requirement (never chmod after).
            .mode(0o600)
            .open(&self.path)
            .map_err(|e| {
                AppError::CalendarSync(format!(
                    "cannot open token file {} for writing: {}",
                    self.path.display(),
                    e.kind()
                ))
            })?;
        file.write_all(data).map_err(|e| {
            AppError::CalendarSync(format!(
                "cannot write token file {}: {}",
                self.path.display(),
                e.kind()
            ))
        })
    }

    #[cfg(not(unix))]
    fn write_contents(&self, data: &[u8]) -> Result<(), AppError> {
        std::fs::write(&self.path, data).map_err(|e| {
            AppError::CalendarSync(format!(
                "cannot write token file {}: {}",
                self.path.display(),
                e.kind()
            ))
        })
    }
}

/// Shared test infrastructure for `google_token` consumers.
///
/// Exposes the parameterized credential double and its factory so that other
/// test modules (e.g. `google::tests`) can reuse them without duplicating the
/// implementation.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;

    /// Parameterized [`keyring::credential::CredentialApi`] double.
    ///
    /// - `fail_load`: `get_secret` returns `NoStorageAccess`; otherwise `NoEntry`.
    /// - `fail_save`: `set_secret` returns `NoStorageAccess`; otherwise `Ok(())`.
    /// - `fail_delete`: `delete_credential` returns `NoStorageAccess`; otherwise `NoEntry`.
    pub(crate) struct FailCredential {
        pub(crate) fail_load: bool,
        pub(crate) fail_save: bool,
        pub(crate) fail_delete: bool,
    }

    impl keyring::credential::CredentialApi for FailCredential {
        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            if self.fail_load {
                Err(keyring::Error::NoStorageAccess(Box::new(
                    std::io::Error::other("test: keyring unavailable"),
                )))
            } else {
                Err(keyring::Error::NoEntry)
            }
        }

        fn set_secret(&self, _secret: &[u8]) -> keyring::Result<()> {
            if self.fail_save {
                Err(keyring::Error::NoStorageAccess(Box::new(
                    std::io::Error::other("test: keyring unavailable"),
                )))
            } else {
                Ok(())
            }
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            if self.fail_delete {
                Err(keyring::Error::NoStorageAccess(Box::new(
                    std::io::Error::other("test: keyring unavailable"),
                )))
            } else {
                Err(keyring::Error::NoEntry)
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Build a [`keyring::Entry`] backed by a [`FailCredential`].
    pub(crate) fn fail_entry(
        fail_load: bool,
        fail_save: bool,
        fail_delete: bool,
    ) -> Arc<keyring::Entry> {
        Arc::new(keyring::Entry::new_with_credential(Box::new(
            FailCredential {
                fail_load,
                fail_save,
                fail_delete,
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::TimeZone as _;
    use tempfile::tempdir;

    use super::test_support::fail_entry;
    use super::{
        keyring_error_message, keyring_key, KeyringStore, PersistedCredential, StoredToken,
        TokenFile,
    };
    use crate::error::AppError;

    fn make_mock_entry() -> Arc<keyring::Entry> {
        Arc::new(keyring::Entry::new_with_credential(Box::new(
            keyring::mock::MockCredential::default(),
        )))
    }

    fn sample_cred() -> PersistedCredential {
        PersistedCredential {
            refresh_token: Some("rt-test".to_owned()),
            expires_at: chrono::Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn keyring_save_then_load_roundtrip() {
        let entry = make_mock_entry();
        let writer = KeyringStore::with_mock_entry(Arc::clone(&entry));
        let reader = KeyringStore::with_mock_entry(Arc::clone(&entry));

        writer.save(&sample_cred()).expect("save");
        let loaded = reader.load().expect("load").expect("Some");

        assert_eq!(loaded.refresh_token, sample_cred().refresh_token);
        assert_eq!(loaded.expires_at, sample_cred().expires_at);
    }

    #[test]
    fn keyring_load_missing_returns_none() {
        let store = KeyringStore::with_mock_entry(make_mock_entry());
        assert!(store.load().expect("load").is_none());
    }

    #[test]
    fn keyring_delete_clears_and_is_idempotent() {
        let entry = make_mock_entry();
        let s1 = KeyringStore::with_mock_entry(Arc::clone(&entry));
        let s2 = KeyringStore::with_mock_entry(Arc::clone(&entry));

        s1.save(&sample_cred()).expect("save");
        s1.delete().expect("first delete");
        assert!(s2.load().expect("load after delete").is_none());
        s1.delete().expect("second delete must be idempotent");
    }

    #[test]
    fn keyring_error_message_actionable_no_inner_details() {
        let sentinel = "INNER_ERROR_SENTINEL_MUST_NOT_LEAK";
        let inner_err = || -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::other(sentinel))
        };

        for err in [
            keyring::Error::NoStorageAccess(inner_err()),
            keyring::Error::PlatformFailure(inner_err()),
        ] {
            let msg = keyring_error_message(&err);
            assert!(
                ["Secret Service", "Keychain", "Credential Manager"]
                    .iter()
                    .any(|s| msg.contains(s)),
                "message should name platform keyring services: {msg}"
            );
            assert!(
                !msg.contains(sentinel),
                "inner error details must not appear in output: {msg}"
            );
        }
    }

    #[test]
    fn different_parent_dirs_produce_different_usernames() {
        let dir1 = tempdir().expect("tempdir 1");
        let dir2 = tempdir().expect("tempdir 2");
        let path1 = dir1.path().join("token.json");
        let path2 = dir2.path().join("token.json");
        let (_, u1) = keyring_key(&path1);
        let (_, u2) = keyring_key(&path2);
        assert_ne!(
            u1, u2,
            "different parent dirs must produce different usernames"
        );
    }

    #[test]
    fn keyring_error_message_no_entry_arm() {
        let msg = keyring_error_message(&keyring::Error::NoEntry);
        assert!(msg.contains("no entry"), "NoEntry arm: {msg}");
    }

    #[test]
    fn keyring_error_message_unknown_variant_arm() {
        let msg = keyring_error_message(&keyring::Error::TooLong("key".to_owned(), 10));
        assert!(msg.contains("omitted"), "_ arm: {msg}");
    }

    #[test]
    fn for_token_path_derives_key_and_constructs_store() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("google_auth.json");
        let (svc, user) = keyring_key(&path);
        let _store = KeyringStore::for_token_path(&path).expect("keyring entry");
        assert_eq!(svc, "com.apreswork.app");
        assert!(user.starts_with("google-oauth:"), "username: {user}");
    }

    #[test]
    fn keyring_load_corrupt_raw_data_returns_err() {
        let entry = make_mock_entry();
        entry
            .set_password("not-valid-json")
            .expect("seed invalid json");
        let store = KeyringStore::with_mock_entry(Arc::clone(&entry));
        // PersistedCredential has no Debug, so use .err().expect() instead of .expect_err().
        let err = store
            .load()
            .err()
            .expect("expected Err for corrupt raw data");
        let msg = err.to_string();
        assert!(
            msg.starts_with("Calendar sync error:"),
            "expected CalendarSync variant, got: {msg}"
        );
        assert!(msg.contains("unreadable"), "error: {msg}");
    }

    #[test]
    fn fail_credential_nonfailing_paths_behave_as_empty_store() {
        let store = KeyringStore::with_mock_entry(fail_entry(false, false, false));
        store
            .save(&sample_cred())
            .expect("save on FailCredential should succeed");
        assert!(store.load().expect("load after save").is_none());
        store
            .delete()
            .expect("delete on FailCredential should succeed");
    }

    #[test]
    fn keyring_delete_keyring_error_returns_err() {
        let store = KeyringStore::with_mock_entry(fail_entry(false, false, true));
        let err = store.delete().expect_err("expected Err");
        assert!(matches!(err, AppError::CalendarSync(_)));
    }

    #[test]
    fn fail_load_credential_propagates_calendar_sync_err() {
        let store = KeyringStore::with_mock_entry(fail_entry(true, false, false));
        let err = store.load().err().expect("expected Err from load");
        assert!(matches!(err, AppError::CalendarSync(_)));
    }

    fn sample_token() -> StoredToken {
        StoredToken {
            access_token: "at-test".to_owned(),
            refresh_token: Some("rt-test".to_owned()),
            expires_at: chrono::Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let tf = TokenFile::new(dir.path().join("token.json"));
        let tok = sample_token();
        tf.save(&tok).expect("save");
        let loaded = tf.load().expect("load").expect("Some");
        assert_eq!(loaded.access_token, tok.access_token);
        assert_eq!(loaded.refresh_token, tok.refresh_token);
        assert_eq!(loaded.expires_at, tok.expires_at);
    }

    #[test]
    fn load_missing_returns_none() {
        let dir = tempdir().expect("tempdir");
        let tf = TokenFile::new(dir.path().join("no_such_file.json"));
        assert!(tf.load().expect("load missing").is_none());
    }

    #[test]
    fn corrupt_json_returns_err_without_token_content() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("corrupt.json");
        std::fs::write(&path, b"CORRUPT_MARKER_XYZ").expect("write corrupt");
        let tf = TokenFile::new(path);
        // unwrap_err() needs T: Debug; StoredToken deliberately omits Debug.
        // Use err().expect() instead — AppError does implement Debug.
        let err = tf.load().err().expect("expected Err for corrupt JSON");
        match &err {
            AppError::CalendarSync(msg) => {
                assert!(
                    !msg.contains("CORRUPT_MARKER_XYZ"),
                    "error must NOT contain token file contents: {msg}"
                );
                assert!(
                    msg.contains("unreadable"),
                    "error should mention 'unreadable': {msg}"
                );
            }
            other => panic!("expected CalendarSync error, got: {other:?}"),
        }
    }

    #[test]
    fn delete_idempotent() {
        let dir = tempdir().expect("tempdir");
        let tf = TokenFile::new(dir.path().join("token.json"));
        tf.save(&sample_token()).expect("save");
        tf.delete().expect("first delete");
        tf.delete().expect("second delete — must be idempotent");
        // Also test on a file that was never created.
        let tf2 = TokenFile::new(dir.path().join("never_created.json"));
        tf2.delete().expect("delete of non-existent file");
    }

    #[test]
    fn save_overwrites_existing() {
        let dir = tempdir().expect("tempdir");
        let tf = TokenFile::new(dir.path().join("token.json"));
        let first = StoredToken {
            access_token: "first-token".to_owned(),
            refresh_token: None,
            expires_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        };
        let second = StoredToken {
            access_token: "second-token".to_owned(),
            refresh_token: Some("rt-2".to_owned()),
            expires_at: chrono::Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
        };
        tf.save(&first).expect("save first");
        tf.save(&second).expect("save second (overwrite)");
        let loaded = tf.load().expect("load after overwrite").expect("Some");
        assert_eq!(loaded.access_token, "second-token");
        assert_eq!(loaded.refresh_token, Some("rt-2".to_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn save_creates_file_with_mode_0600() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("token.json");
        let tf = TokenFile::new(path.clone());
        tf.save(&sample_token()).expect("save");
        let meta = std::fs::metadata(&path).expect("metadata");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected mode 0600, got {mode:#o}");
    }

    #[test]
    fn load_non_notfound_error_returns_err() {
        // A directory at the token path makes read_to_string return EISDIR
        // (not NotFound), exercising the fallthrough Err branch.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("dir_not_a_file");
        std::fs::create_dir(&path).expect("create dir");
        let tf = TokenFile::new(path);
        let err = tf.load().err().expect("expected Err");
        assert!(matches!(err, AppError::CalendarSync(_)));
    }

    #[test]
    fn delete_non_notfound_error_returns_err() {
        // A non-empty directory at the token path makes remove_file return
        // EISDIR (not NotFound), exercising the fallthrough Err branch.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("dir_not_a_file");
        std::fs::create_dir(&path).expect("create dir");
        // Put a file inside so the directory cannot be removed as a dir either.
        std::fs::write(path.join("inner"), b"x").expect("write inner");
        let tf = TokenFile::new(path);
        let err = tf.delete().expect_err("expected Err");
        assert!(matches!(err, AppError::CalendarSync(_)));
    }

    #[test]
    fn save_missing_parent_dir_returns_err() {
        // A token path whose parent directory does not exist causes
        // OpenOptions::open to fail, exercising the open map_err branch.
        let tf = TokenFile::new(std::path::PathBuf::from("/nonexistent/dir/token.json"));
        let err = tf.save(&sample_token()).expect_err("expected Err");
        assert!(matches!(err, AppError::CalendarSync(_)));
    }
}
