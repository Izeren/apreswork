// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Multi-user profiles (M13): fully isolated per-user data sets inside one
//! installed app.
//!
//! A profile is a directory `app_data_dir/profiles/<uuid>/` holding that
//! user's `apreswork.db` (and `google_auth.json` once connected). The
//! `profiles.json` registry at the data-dir root lists the profiles; see
//! [`registry`]. Opening a profile — including starting the REST API server —
//! happens only after the picker selects one; see [`activate`].
//!
//! Module map:
//! - [`registry`] — `profiles.json` model + atomic load/save
//! - [`adoption`] — first-run adoption of the pre-profiles database (M13.4)
//! - [`service`] — registry-mutating operations behind the profile commands
//! - [`activate`] — builds `AppState` and starts timers + REST post-unlock

pub mod activate;
pub mod adoption;
pub mod registry;
pub mod service;

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;

use crate::error::AppError;
use crate::profiles::registry::ProfilesRegistry;

/// Identity of the profile the current `AppState` serves.
///
/// Carried inside `AppState` so every surface (Tauri commands, REST) can
/// report whose data it is touching (`GET /api/profile`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActiveProfile {
    pub id: String,
    pub name: String,
}

/// Always-managed Tauri state: the profile registry and where it lives.
///
/// Managed during `setup()` — before any profile is unlocked — so the gate
/// commands can list, create, and unlock profiles while `AppState` does not
/// exist yet.
#[derive(Debug)]
pub struct ProfilesState {
    pub data_dir: PathBuf,
    pub registry: Mutex<ProfilesRegistry>,
}

impl ProfilesState {
    #[must_use]
    pub fn new(data_dir: PathBuf, registry: ProfilesRegistry) -> Self {
        Self {
            data_dir,
            registry: Mutex::new(registry),
        }
    }

    /// Lock the registry, mapping a poisoned mutex to a clean error.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] if a previous holder panicked.
    pub fn lock_registry(&self) -> Result<MutexGuard<'_, ProfilesRegistry>, AppError> {
        self.registry
            .lock()
            .map_err(|_| AppError::Internal("profiles registry lock poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveProfile, ProfilesState};
    use crate::profiles::registry::ProfilesRegistry;

    #[test]
    fn active_profile_serializes_id_and_name() {
        let profile = ActiveProfile {
            id: "p-1".to_owned(),
            name: "Alice".to_owned(),
        };
        let json = serde_json::to_value(&profile).expect("serialize");
        assert_eq!(json["id"], "p-1");
        assert_eq!(json["name"], "Alice");
    }

    #[test]
    fn lock_registry_returns_guard() {
        let state = ProfilesState::new(
            std::env::temp_dir(),
            ProfilesRegistry {
                version: 1,
                last_used: None,
                profiles: vec![],
            },
        );
        let guard = state.lock_registry().expect("lock");
        assert!(guard.profiles.is_empty());
    }
}
