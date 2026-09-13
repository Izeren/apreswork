// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrapper for saving Google OAuth client credentials.

// Tauri command signatures require by-value AppHandle params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use crate::calendar::google::{ClientCredentialStore, GoogleCredentials};
use crate::domain::validation::validate_client_credentials;
use crate::error::AppError;

/// Validation tests pass unreachable!() — the factory is never reached.
/// Happy-path tests use a mock — neither variant touches the OS keyring or calls restart.
fn save_credentials_impl<F>(
    store_fn: F,
    client_id: &str,
    client_secret: &str,
) -> Result<(), AppError>
where
    F: FnOnce() -> Result<ClientCredentialStore, AppError>,
{
    let id = client_id.trim();
    let secret = client_secret.trim();
    validate_client_credentials(id, secret)?;
    store_fn()?.save(&GoogleCredentials {
        client_id: id.to_owned(),
        client_secret: secret.to_owned(),
    })
}

/// Save Google OAuth client credentials to the OS keyring and restart the app.
///
/// Inputs are trimmed before validation. Empty or over-length values are
/// rejected.
///
/// # Errors
///
/// Returns [`AppError::Validation`] when `client_id` or `client_secret` is
/// invalid after trim. Returns [`AppError::CalendarSync`] when the keyring
/// save fails. Returns [`AppError::Internal`] if the blocking task is
/// cancelled.
#[tauri::command]
pub async fn save_google_client_credentials<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    client_id: String,
    client_secret: String,
) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        save_credentials_impl(ClientCredentialStore::new, &client_id, &client_secret)
    })
    .await
    .map_err(|e| AppError::Internal(format!("save credentials task failed: {e}")))??;
    app.restart()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use test_case::test_case;

    use super::save_credentials_impl;
    use crate::calendar::google::ClientCredentialStore;
    use crate::calendar::google_token::test_support::make_mock_entry;
    use crate::domain::validation::CLIENT_CRED_MAX_LEN;
    use crate::error::AppError;
    use crate::test_support::assert_validation_contains;

    #[test_case("",    "secret", "client_id"     ; "empty_id")]
    #[test_case("   ", "secret", "client_id"     ; "whitespace_id")]
    #[test_case("id",  "",       "client_secret" ; "empty_secret")]
    #[test_case("id",  "  ",     "client_secret" ; "whitespace_secret")]
    fn save_rejects_empty_field(id: &str, secret: &str, field: &str) {
        let result = save_credentials_impl(|| unreachable!(), id, secret);
        assert_validation_contains(&result, field);
    }

    #[test]
    fn save_rejects_id_that_exceeds_max_length() {
        let long = "a".repeat(CLIENT_CRED_MAX_LEN + 1);
        let result = save_credentials_impl(|| unreachable!(), &long, "secret");
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "got: {result:?}"
        );
    }

    #[test]
    fn save_writes_trimmed_values_to_store() {
        let entry = make_mock_entry();
        save_credentials_impl(
            || Ok(ClientCredentialStore::with_mock_entry(Arc::clone(&entry))),
            "  my-id  ",
            "  my-secret  ",
        )
        .expect("save must succeed");
        let loaded = ClientCredentialStore::with_mock_entry(entry)
            .load()
            .expect("load must succeed")
            .expect("entry must be present");
        assert_eq!(
            loaded.client_id, "my-id",
            "trim must remove leading/trailing whitespace"
        );
        assert_eq!(
            loaded.client_secret, "my-secret",
            "trim must remove leading/trailing whitespace"
        );
    }
}
