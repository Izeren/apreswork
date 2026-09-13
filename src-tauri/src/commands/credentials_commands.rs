// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrapper for saving Google OAuth client credentials.

// Tauri command signatures require by-value AppHandle params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tauri::Manager as _;

use crate::calendar;
use crate::calendar::google::{ClientCredentialStore, GoogleCredentials, GoogleEndpoints};
use crate::calendar::google_client_creds::probe_google_credentials;
use crate::domain::validation::validate_client_credentials;
use crate::error::AppError;
use crate::profiles::{activate, ProfilesState};
use crate::state::ActiveState;

/// Happy-path tests use a mock — neither variant touches the OS keyring or calls restart.
fn credentials_saved_impl<F>(store_fn: F) -> bool
where
    F: FnOnce() -> Result<ClientCredentialStore, AppError>,
{
    store_fn()
        .and_then(|s| s.load())
        .map(|opt| opt.is_some())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn google_client_credentials_saved() -> bool {
    tauri::async_runtime::spawn_blocking(|| credentials_saved_impl(ClientCredentialStore::new))
        .await
        .unwrap_or(false)
}

fn save_credentials_impl<S, P>(
    store_fn: S,
    probe_fn: P,
    client_id: &str,
    client_secret: &str,
) -> Result<(), AppError>
where
    S: FnOnce() -> Result<ClientCredentialStore, AppError>,
    P: FnOnce(&str, &str) -> Result<(), AppError>,
{
    let id = client_id.trim();
    let secret = client_secret.trim();
    validate_client_credentials(id, secret)?;
    probe_fn(id, secret)?;
    store_fn()?.save(&GoogleCredentials {
        client_id: id.to_owned(),
        client_secret: secret.to_owned(),
    })
}

fn reload_profile_creds<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let profile_id = app.state::<ActiveState>().get()?.profile.id.clone();
    let profiles_state = app.state::<Arc<ProfilesState>>();
    let (entry, data_dir) = {
        let registry = profiles_state.lock_registry()?;
        let entry = registry
            .find(&profile_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound {
                entity: "Profile".to_owned(),
                id: profile_id,
            })?;
        (entry, profiles_state.data_dir.clone())
    };
    let creds = calendar::resolve_client_creds(GoogleCredentials::from_keyring);
    activate::switch_active_profile(app, &data_dir, &entry, now, creds)?;
    Ok(())
}

/// Validates and saves BYO OAuth client credentials to the OS keyring.
///
/// Empty or over-length values are rejected before the probe runs. The
/// probe posts a dummy authorization code to Google; Google rejects
/// structurally invalid credentials with `"invalid_client"` even for a
/// dummy code. Transport failures are treated as best-effort and do not
/// block the save.
///
/// # Errors
///
/// Returns [`AppError::Validation`] when `client_id` or `client_secret` is
/// invalid after trim, when Google returns `"invalid_client"`, or when no
/// profile is active (the save completes before the reload runs). Returns
/// [`AppError::CalendarSync`] when the keyring save fails. Returns
/// [`AppError::Internal`] if the blocking task is cancelled.
#[tauri::command]
pub async fn save_google_client_credentials<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    client_id: String,
    client_secret: String,
) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let token_url = GoogleEndpoints::default().token_url;
        save_credentials_impl(
            ClientCredentialStore::new,
            |id, secret| probe_google_credentials(&token_url, id, secret),
            &client_id,
            &client_secret,
        )?;
        reload_profile_creds(&app, Utc::now())
    })
    .await
    .map_err(|e| AppError::Internal(format!("save credentials task failed: {e}")))?
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use test_case::test_case;

    use super::{credentials_saved_impl, save_credentials_impl};
    use crate::calendar::google::{ClientCredentialStore, GoogleCredentials};
    use crate::calendar::google_token::test_support::make_mock_entry;
    use crate::domain::validation::CLIENT_CRED_MAX_LEN;
    use crate::error::AppError;
    use crate::test_support::assert_validation_contains;

    #[test]
    fn credentials_saved_returns_false_when_store_is_empty() {
        let entry = make_mock_entry();
        let result = credentials_saved_impl(|| {
            Ok(ClientCredentialStore::with_mock_entry(Arc::clone(&entry)))
        });
        assert!(!result, "empty keyring must return false");
    }

    #[test]
    fn credentials_saved_returns_true_when_creds_stored() {
        let entry = make_mock_entry();
        ClientCredentialStore::with_mock_entry(Arc::clone(&entry))
            .save(&GoogleCredentials {
                client_id: "id".into(),
                client_secret: "sec".into(),
            })
            .expect("save must succeed");
        let result = credentials_saved_impl(|| {
            Ok(ClientCredentialStore::with_mock_entry(Arc::clone(&entry)))
        });
        assert!(result, "stored credentials must return true");
    }

    #[test]
    fn credentials_saved_returns_false_when_store_factory_fails() {
        let result =
            credentials_saved_impl(|| Err(AppError::CalendarSync("keyring unavailable".into())));
        assert!(!result, "factory error must return false");
    }

    #[test_case("",    "secret", "client_id"     ; "empty_id")]
    #[test_case("   ", "secret", "client_id"     ; "whitespace_id")]
    #[test_case("id",  "",       "client_secret" ; "empty_secret")]
    #[test_case("id",  "  ",     "client_secret" ; "whitespace_secret")]
    fn save_rejects_empty_field(id: &str, secret: &str, field: &str) {
        let result = save_credentials_impl(|| unreachable!(), |_, _| unreachable!(), id, secret);
        assert_validation_contains(&result, field);
    }

    #[test]
    fn save_rejects_id_that_exceeds_max_length() {
        let long = "a".repeat(CLIENT_CRED_MAX_LEN + 1);
        let result =
            save_credentials_impl(|| unreachable!(), |_, _| unreachable!(), &long, "secret");
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "got: {result:?}"
        );
    }

    #[test]
    fn save_rejects_when_probe_returns_invalid_client() {
        let result = save_credentials_impl(
            || unreachable!(),
            |_, _| {
                Err(AppError::Validation(
                    "Google rejected the OAuth credentials. Check the client ID and secret.".into(),
                ))
            },
            "my-id.apps.googleusercontent.com",
            "GOCSPX-secret",
        );
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "probe failure must propagate as Validation error; got: {result:?}"
        );
    }

    #[test]
    fn save_calls_store_when_probe_passes() {
        let entry = make_mock_entry();
        save_credentials_impl(
            || Ok(ClientCredentialStore::with_mock_entry(Arc::clone(&entry))),
            |_, _| Ok(()),
            "my-id.apps.googleusercontent.com",
            "GOCSPX-secret",
        )
        .expect("save must succeed when probe passes");
        let loaded = ClientCredentialStore::with_mock_entry(entry)
            .load()
            .expect("load must succeed")
            .expect("entry must be present");
        assert_eq!(loaded.client_id, "my-id.apps.googleusercontent.com");
        assert_eq!(loaded.client_secret, "GOCSPX-secret");
    }

    #[test]
    fn save_writes_trimmed_values_to_store() {
        let entry = make_mock_entry();
        save_credentials_impl(
            || Ok(ClientCredentialStore::with_mock_entry(Arc::clone(&entry))),
            |_, _| Ok(()),
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

    mod reload_tests {
        use std::sync::Arc;

        use tauri::Manager as _;
        use tempfile::{tempdir, TempDir};

        use crate::error::AppError;
        use crate::profiles::activate::build_app_state;
        use crate::profiles::registry::{
            profile_dir, ProfileEntry, ProfilesRegistry, REGISTRY_VERSION,
        };
        use crate::profiles::{ActiveProfile, ProfilesState};
        use crate::state::ActiveState;

        fn mock_app_with_active_profile(
            dir: &TempDir,
            registry_profiles: Vec<ProfileEntry>,
            active_id: &str,
        ) -> tauri::App<tauri::test::MockRuntime> {
            let registry = ProfilesRegistry {
                version: REGISTRY_VERSION,
                last_used: None,
                profiles: registry_profiles,
            };
            let app = tauri::test::mock_app();
            app.manage(Arc::new(ProfilesState::new(
                dir.path().to_path_buf(),
                registry,
            )));
            let pd = profile_dir(dir.path(), active_id);
            let state = build_app_state(
                &pd,
                ActiveProfile {
                    id: active_id.to_owned(),
                    name: "Test".to_owned(),
                },
                None,
                None,
            )
            .expect("build state");
            app.manage(ActiveState::from(Arc::new(state)));
            app
        }

        #[test]
        fn returns_ok_with_active_profile() {
            use crate::profiles::registry::test_support::entry;
            use crate::test_support::test_now;

            let dir = tempdir().expect("tempdir");
            let profile_id = "reload-test-id";
            let app = mock_app_with_active_profile(
                &dir,
                vec![entry(profile_id, "Reload Test")],
                profile_id,
            );

            let result = super::super::reload_profile_creds(app.handle(), test_now());
            assert!(result.is_ok(), "reload must succeed; got: {result:?}");
        }

        #[test]
        fn returns_not_found_when_profile_missing_from_registry() {
            use crate::test_support::test_now;

            let dir = tempdir().expect("tempdir");
            let app = mock_app_with_active_profile(&dir, vec![], "ghost-id");

            let result = super::super::reload_profile_creds(app.handle(), test_now());
            assert!(
                matches!(result, Err(AppError::NotFound { .. })),
                "missing registry entry must return NotFound; got: {result:?}"
            );
        }
    }
}
