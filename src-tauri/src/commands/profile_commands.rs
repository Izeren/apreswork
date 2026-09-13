// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for the profile gate, the switcher, and the
//! profiles management page.
//!
//! These operate on the always-managed `ProfilesState` registry and the
//! process-scoped `ActiveState` slot — they are the only data-touching
//! commands available before a profile is unlocked. The registry logic
//! lives in `profiles::service`; activation composition in
//! `profiles::activate`.

// Tauri command signatures require by-value `State`/`AppHandle`/`String`
// params; the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use std::ops::ControlFlow;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::Manager as _;

use crate::calendar;
use crate::calendar::google::GoogleCredentials;
use crate::error::AppError;
use crate::profiles::registry::{ProfileEntry, ProfilesRegistry};
use crate::profiles::{activate, service, ActiveProfile, ProfilesState};
use crate::state::ActiveState;

/// A profile as exposed to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl From<&ProfileEntry> for ProfileInfo {
    fn from(entry: &ProfileEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            created_at: entry.created_at,
        }
    }
}

/// Snapshot for the frontend gate: whether a profile is active, and what the
/// picker should list.
#[derive(Debug, Serialize)]
pub struct ProfileStatusResponse {
    pub active: Option<ActiveProfile>,
    pub profiles: Vec<ProfileInfo>,
    pub last_used: Option<String>,
}

/// Shared preamble for `unlock_profile` and `switch_profile`: both start by resolving the target profile under the registry lock.
fn with_profile_entry<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    id: &str,
    after: impl FnOnce(&mut ProfilesRegistry, &ProfilesState, ProfileEntry) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let profiles_state = app.state::<Arc<ProfilesState>>();
    let mut registry = profiles_state.lock_registry()?;
    let entry = registry
        .find(id)
        .cloned()
        .ok_or_else(|| AppError::NotFound {
            entity: "Profile".to_owned(),
            id: id.to_owned(),
        })?;
    after(&mut registry, &profiles_state, entry)
}

/// Current gate status + the registry contents for the picker.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if the registry lock is poisoned.
#[tauri::command]
pub fn profile_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<ProfileStatusResponse, AppError> {
    let profiles_state = app.state::<Arc<ProfilesState>>();
    let registry = profiles_state.lock_registry()?;
    // The registry is the name authority: the active state carries a copy
    // taken at activation, which goes stale after a rename.
    let active = app
        .try_state::<ActiveState>()
        .and_then(|slot| slot.get_opt())
        .map(|s| {
            let mut active = s.profile.clone();
            if let Some(entry) = registry.find(&active.id) {
                active.name.clone_from(&entry.name);
            }
            active
        });
    Ok(ProfileStatusResponse {
        active,
        profiles: registry.profiles.iter().map(ProfileInfo::from).collect(),
        last_used: registry.last_used.clone(),
    })
}

fn default_creds() -> Option<GoogleCredentials> {
    calendar::resolve_client_creds(GoogleCredentials::from_keyring)
}

async fn spawn_blocking_profile<T: Send + 'static>(
    op: impl FnOnce() -> Result<T, AppError> + Send + 'static,
    task_name: &'static str,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|e| AppError::Internal(format!("{task_name} task failed: {e}")))?
}

/// Tests call this with `resolve_creds = || None` to bypass the OS keyring.
async fn unlock_profile_impl<R, F>(
    app: tauri::AppHandle<R>,
    id: String,
    resolve_creds: F,
) -> Result<ActiveProfile, AppError>
where
    R: tauri::Runtime,
    F: FnOnce() -> Option<GoogleCredentials> + Send + 'static,
{
    let (entry, data_dir) = with_profile_entry(&app, &id, |registry, profiles_state, entry| {
        service::mark_last_used(registry, &profiles_state.data_dir, &id)?;
        Ok((entry, profiles_state.data_dir.clone()))
    })?;
    // Provider's blocking HTTP client panics when created or dropped on async runtime.
    // Same rule as provider calls in auth_commands. Keyring I/O is synchronous D-Bus.
    spawn_blocking_profile(
        move || activate::activate_profile(&app, &data_dir, &entry, Utc::now(), resolve_creds()),
        "profile activation",
    )
    .await
}

/// Activate the selected profile: open its database and install it into the
/// process-scoped `ActiveState` slot (the REST server and background timers
/// pick it up on their own).
///
/// # Errors
///
/// Returns [`AppError::Validation`] when a profile is already active (switch
/// instead), [`AppError::NotFound`] for an unknown id, and activation errors
/// from `profiles::activate`.
#[tauri::command]
pub async fn unlock_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<ActiveProfile, AppError> {
    unlock_profile_impl(app, id, default_creds).await
}

/// Create a new profile (validated name, empty data dir).
///
/// # Errors
///
/// Returns [`AppError::Validation`] for an empty/duplicate name,
/// [`AppError::Internal`] on filesystem failure.
#[tauri::command]
pub fn create_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
) -> Result<ProfileInfo, AppError> {
    let profiles_state = app.state::<Arc<ProfilesState>>();
    let mut registry = profiles_state.lock_registry()?;
    let entry =
        service::create_profile(&mut registry, &profiles_state.data_dir, &name, Utc::now())?;
    Ok(ProfileInfo::from(&entry))
}

/// Rename a profile (active or not).
///
/// # Errors
///
/// Returns [`AppError::NotFound`] for an unknown id and
/// [`AppError::Validation`] for an empty or duplicate name.
#[tauri::command]
pub fn rename_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    name: String,
) -> Result<ProfileInfo, AppError> {
    let profiles_state = app.state::<Arc<ProfilesState>>();
    let mut registry = profiles_state.lock_registry()?;
    let entry = service::rename_profile(&mut registry, &profiles_state.data_dir, &id, &name)?;
    Ok(ProfileInfo::from(&entry))
}

async fn switch_profile_impl<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    creds_fn: impl FnOnce() -> Option<GoogleCredentials> + Send + 'static,
) -> Result<ActiveProfile, AppError> {
    let outcome = with_profile_entry(&app, &id, |registry, profiles_state, entry| {
        let current = app.try_state::<ActiveState>().and_then(|s| s.get_opt());
        if current.is_some_and(|s| s.profile.id == entry.id) {
            return Ok(ControlFlow::Break(ActiveProfile {
                id: entry.id.clone(),
                name: entry.name.clone(),
            }));
        }
        service::mark_last_used(registry, &profiles_state.data_dir, &id)?;
        Ok(ControlFlow::Continue((
            entry,
            profiles_state.data_dir.clone(),
        )))
    })?;
    let (entry, data_dir) = match outcome {
        ControlFlow::Break(profile) => return Ok(profile),
        ControlFlow::Continue(pair) => pair,
    };
    spawn_blocking_profile(
        move || activate::switch_active_profile(&app, &data_dir, &entry, Utc::now(), creds_fn()),
        "profile switch",
    )
    .await
}

/// Switch the running app to another profile in-process.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] for an unknown id, and activation errors
/// from `profiles::activate` (on failure the slot is left empty and the
/// frontend falls back to the profile gate).
#[tauri::command]
pub async fn switch_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<ActiveProfile, AppError> {
    // see unlock_profile_impl for spawn_blocking rationale
    switch_profile_impl(app, id, default_creds).await
}

/// Destructive — the UI confirms first.
///
/// # Errors
///
/// Returns [`AppError::Validation`] for the active profile,
/// [`AppError::NotFound`] for an unknown id, and [`AppError::Internal`] on
/// filesystem failure.
#[tauri::command]
pub fn delete_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<(), AppError> {
    if app
        .try_state::<ActiveState>()
        .and_then(|s| s.get_opt())
        .is_some_and(|s| s.profile.id == id)
    {
        return Err(AppError::Validation(
            "This profile is currently active — switch to another profile first.".into(),
        ));
    }
    let profiles_state = app.state::<Arc<ProfilesState>>();
    let mut registry = profiles_state.lock_registry()?;
    service::delete_profile(&mut registry, &profiles_state.data_dir, &id)
}

#[cfg(test)]
mod tests {
    use crate::commands::test_support::{tempdir, Arc, Manager as _, MockRuntime, TempDir};

    use super::{
        create_profile, delete_profile, profile_status, rename_profile, switch_profile,
        switch_profile_impl, unlock_profile, unlock_profile_impl,
    };
    use crate::error::AppError;
    use crate::profiles::activate::build_app_state;
    use crate::profiles::registry::test_support::entry;
    use crate::profiles::registry::{
        profile_dir, ProfileEntry, ProfilesRegistry, REGISTRY_VERSION,
    };
    use crate::profiles::{ActiveProfile, ProfilesState};
    use crate::state::ActiveState;

    /// A mock app with a managed `ProfilesState` (the pre-unlock world).
    fn gated_app(profiles: Vec<ProfileEntry>) -> (TempDir, tauri::App<MockRuntime>) {
        let dir = tempdir().expect("tempdir");
        let registry = ProfilesRegistry {
            version: REGISTRY_VERSION,
            last_used: None,
            profiles,
        };
        let app = tauri::test::mock_app();
        app.manage(Arc::new(ProfilesState::new(
            dir.path().to_path_buf(),
            registry,
        )));
        (dir, app)
    }

    /// Additionally manage a pre-filled `ActiveState` slot holding a state
    /// for `id` (the post-unlock world).
    fn activate(app: &tauri::App<MockRuntime>, dir: &TempDir, id: &str) {
        let state = build_app_state(
            &profile_dir(dir.path(), id),
            ActiveProfile {
                id: id.to_owned(),
                name: format!("Profile {id}"),
            },
            None,
            None,
        )
        .expect("build state");
        app.manage(ActiveState::from(Arc::new(state)));
    }

    #[test]
    fn profile_status_reports_gate_state() {
        let (dir, app) = gated_app(vec![entry("p1", "Alice"), entry("p2", "Bob")]);

        let before = profile_status(app.handle().clone()).expect("status");
        assert_eq!(before.active, None, "nothing unlocked yet");
        assert_eq!(before.profiles.len(), 2);

        activate(&app, &dir, "p1");
        let after = profile_status(app.handle().clone()).expect("status");
        assert_eq!(after.active.map(|a| a.id), Some("p1".to_owned()));
    }

    #[tokio::test]
    async fn unlock_profile_unknown_id_is_not_found() {
        let (_dir, app) = gated_app(vec![entry("p1", "Alice")]);
        let err = unlock_profile(app.handle().clone(), "ghost".to_owned())
            .await
            .expect_err("must fail");
        assert!(matches!(err, AppError::NotFound { .. }), "got: {err}");
    }

    #[tokio::test]
    async fn unlock_profile_rejects_when_already_active() {
        let (dir, app) = gated_app(vec![entry("p1", "Alice")]);
        activate(&app, &dir, "p1");
        let err = unlock_profile_impl(app.handle().clone(), "p1".to_owned(), || None)
            .await
            .expect_err("second unlock must fail");
        assert!(matches!(err, AppError::Validation(_)), "got: {err}");
    }

    #[test]
    fn create_profile_command_persists_and_rejects_duplicates() {
        let (dir, app) = gated_app(vec![]);
        let info = create_profile(app.handle().clone(), "Bob".to_owned()).expect("create");
        assert_eq!(info.name, "Bob");
        assert!(
            dir.path().join("profiles.json").exists(),
            "registry must be persisted"
        );

        let err = create_profile(app.handle().clone(), " bob ".to_owned())
            .expect_err("duplicate name must fail");
        assert!(matches!(err, AppError::Validation(_)), "got: {err}");
    }

    #[test]
    fn rename_profile_command_renames_and_validates() {
        let (_dir, app) = gated_app(vec![entry("p1", "Default"), entry("p2", "Bob")]);

        let info = rename_profile(app.handle().clone(), "p1".to_owned(), " Alice ".to_owned())
            .expect("rename");
        assert_eq!(info.name, "Alice", "name is trimmed");
        let status = profile_status(app.handle().clone()).expect("status");
        assert!(status.profiles.iter().any(|p| p.name == "Alice"));

        let err = rename_profile(app.handle().clone(), "p1".to_owned(), "bob".to_owned())
            .expect_err("duplicate name must fail");
        assert!(matches!(err, AppError::Validation(_)), "got: {err}");

        let err = rename_profile(app.handle().clone(), "ghost".to_owned(), "X".to_owned())
            .expect_err("unknown id must fail");
        assert!(matches!(err, AppError::NotFound { .. }), "got: {err}");
    }

    #[test]
    fn profile_status_reports_the_renamed_name_for_the_active_profile() {
        let (dir, app) = gated_app(vec![entry("p1", "Default")]);
        activate(&app, &dir, "p1");

        rename_profile(app.handle().clone(), "p1".to_owned(), "Alice".to_owned()).expect("rename");

        let status = profile_status(app.handle().clone()).expect("status");
        assert_eq!(
            status.active.map(|a| a.name),
            Some("Alice".to_owned()),
            "active name must come from the registry, not the stale activation copy"
        );
    }

    #[test]
    fn delete_profile_command_deletes_a_non_active_profile() {
        let (dir, app) = gated_app(vec![entry("p1", "Alice"), entry("p2", "Stale")]);
        activate(&app, &dir, "p1");

        delete_profile(app.handle().clone(), "p2".to_owned()).expect("delete");

        let status = profile_status(app.handle().clone()).expect("status");
        assert_eq!(status.profiles.len(), 1);
        assert_eq!(status.profiles[0].id, "p1");
    }

    #[test]
    fn delete_profile_command_rejects_the_active_profile() {
        let (dir, app) = gated_app(vec![entry("p1", "Alice")]);
        activate(&app, &dir, "p1");

        let err = delete_profile(app.handle().clone(), "p1".to_owned())
            .expect_err("active profile must not be deletable");
        assert!(matches!(err, AppError::Validation(_)), "got: {err}");
        let status = profile_status(app.handle().clone()).expect("status");
        assert_eq!(status.profiles.len(), 1, "profile kept");
    }

    #[test]
    fn delete_profile_command_unknown_id_is_not_found() {
        let (_dir, app) = gated_app(vec![entry("p1", "Alice")]);
        let err = delete_profile(app.handle().clone(), "ghost".to_owned()).expect_err("must fail");
        assert!(matches!(err, AppError::NotFound { .. }), "got: {err}");
    }

    #[tokio::test]
    async fn switch_profile_unknown_id_is_not_found() {
        let (_dir, app) = gated_app(vec![]);
        let err = switch_profile(app.handle().clone(), "ghost".to_owned())
            .await
            .expect_err("unknown id must fail");
        assert!(matches!(err, AppError::NotFound { .. }), "got: {err}");
    }

    #[tokio::test]
    async fn switch_profile_to_the_active_profile_is_a_no_op() {
        let (dir, app) = gated_app(vec![entry("p1", "Alice")]);
        activate(&app, &dir, "p1");
        let before = app.state::<ActiveState>().get().expect("active");

        let profile = switch_profile(app.handle().clone(), "p1".to_owned())
            .await
            .expect("no-op switch");

        assert_eq!(profile.id, "p1");
        assert_eq!(profile.name, "Alice", "name comes from the registry");
        let after = app.state::<ActiveState>().get().expect("still active");
        assert!(
            Arc::ptr_eq(&before, &after),
            "the running state must not be torn down"
        );
    }

    #[tokio::test]
    async fn switch_profile_activates_the_target_in_process() {
        let (dir, app) = gated_app(vec![entry("p1", "Alice"), entry("p2", "Bob")]);
        activate(&app, &dir, "p1");

        let profile = switch_profile_impl(app.handle().clone(), "p2".to_owned(), || None)
            .await
            .expect("switch");

        assert_eq!(profile.id, "p2");
        assert_eq!(profile.name, "Bob");
        let status = profile_status(app.handle().clone()).expect("status");
        assert_eq!(status.active.map(|a| a.id), Some("p2".to_owned()));
        assert_eq!(status.last_used.as_deref(), Some("p2"));
    }
}
