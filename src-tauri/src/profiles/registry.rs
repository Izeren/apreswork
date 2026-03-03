// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Profile registry: the `profiles.json` document at the app data-dir root.
//!
//! Profile data lives in `profiles/<id>/` directories **derived** from the
//! id — no paths are stored, so nothing can go stale. Writes are atomic
//! (temp file + rename) so a crash mid-write never corrupts the registry.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub const REGISTRY_VERSION: u32 = 1;

/// One profile as stored in `profiles.json`.
///
/// Unknown fields are ignored on load (serde default), so registries written
/// by builds that stored a PIN hash still parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilesRegistry {
    pub version: u32,
    /// Id of the profile to auto-open at startup (updated on unlock/switch).
    pub last_used: Option<String>,
    pub profiles: Vec<ProfileEntry>,
}

impl ProfilesRegistry {
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&ProfileEntry> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// The profile the app auto-opens at startup: the last-used profile
    /// when it still exists, otherwise the first one (in-app switching
    /// covers the rest). `None` only for an empty registry — then the gate
    /// shows with its create form.
    #[must_use]
    pub fn startup_profile(&self) -> Option<&ProfileEntry> {
        self.last_used
            .as_deref()
            .and_then(|id| self.find(id))
            .or_else(|| self.profiles.first())
    }
}

/// Path of the registry document inside the app data directory.
#[must_use]
pub fn registry_path(data_dir: &Path) -> PathBuf {
    data_dir.join("profiles.json")
}

/// Directory holding a profile's isolated data set.
#[must_use]
pub fn profile_dir(data_dir: &Path, profile_id: &str) -> PathBuf {
    data_dir.join("profiles").join(profile_id)
}

/// Load the registry; `Ok(None)` when no registry exists yet.
///
/// # Errors
///
/// Returns [`AppError::Internal`] when the file is unreadable, malformed, or
/// carries an unsupported version (never silently drops profiles).
pub fn load(data_dir: &Path) -> Result<Option<ProfilesRegistry>, AppError> {
    let path = registry_path(data_dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(AppError::Internal(format!(
                "failed to read profiles.json: {e}"
            )))
        }
    };
    let registry: ProfilesRegistry = serde_json::from_str(&raw)
        .map_err(|e| AppError::Internal(format!("profiles.json is malformed: {e}")))?;
    if registry.version != REGISTRY_VERSION {
        return Err(AppError::Internal(format!(
            "profiles.json version {} is not supported by this build (expected {REGISTRY_VERSION})",
            registry.version
        )));
    }
    Ok(Some(registry))
}

/// Persist the registry atomically (temp file + rename).
///
/// # Errors
///
/// Returns [`AppError::Internal`] on serialization or filesystem failure.
pub fn save(data_dir: &Path, registry: &ProfilesRegistry) -> Result<(), AppError> {
    let raw = serde_json::to_string_pretty(registry)
        .map_err(|e| AppError::Internal(format!("failed to serialize profiles.json: {e}")))?;
    let tmp = data_dir.join("profiles.json.tmp");
    std::fs::write(&tmp, raw)
        .map_err(|e| AppError::Internal(format!("failed to write profiles.json: {e}")))?;
    std::fs::rename(&tmp, registry_path(data_dir))
        .map_err(|e| AppError::Internal(format!("failed to replace profiles.json: {e}")))?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use chrono::{TimeZone as _, Utc};

    use super::ProfileEntry;

    /// A registry entry with sensible test defaults; override fields inline.
    pub(crate) fn entry(id: &str, name: &str) -> ProfileEntry {
        ProfileEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            created_at: Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use test_case::test_case;

    use super::test_support::entry;
    use super::{load, profile_dir, registry_path, save, ProfilesRegistry, REGISTRY_VERSION};
    use crate::error::AppError;

    fn registry_with(entries: Vec<super::ProfileEntry>) -> ProfilesRegistry {
        ProfilesRegistry {
            version: REGISTRY_VERSION,
            last_used: entries.first().map(|e| e.id.clone()),
            profiles: entries,
        }
    }

    #[test]
    fn load_missing_registry_returns_none() {
        let dir = tempdir().expect("tempdir");
        assert_eq!(load(dir.path()).expect("load"), None);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir().expect("tempdir");
        let registry = registry_with(vec![entry("a", "Alice"), entry("b", "Bob")]);
        save(dir.path(), &registry).expect("save");
        let loaded = load(dir.path()).expect("load").expect("registry present");
        assert_eq!(loaded, registry);
    }

    #[test]
    fn load_tolerates_a_legacy_pin_hash_field() {
        // Registries written by builds that supported profile PINs carry a
        // pin_hash per profile; those files must keep loading.
        let dir = tempdir().expect("tempdir");
        let raw = r#"{
            "version": 1,
            "last_used": "a",
            "profiles": [{
                "id": "a",
                "name": "Alice",
                "pin_hash": "$argon2id$fake",
                "created_at": "2026-07-12T12:00:00Z"
            }]
        }"#;
        std::fs::write(registry_path(dir.path()), raw).expect("write");
        let loaded = load(dir.path()).expect("load").expect("registry present");
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].name, "Alice");
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempdir().expect("tempdir");
        save(dir.path(), &registry_with(vec![])).expect("save");
        assert!(!dir.path().join("profiles.json.tmp").exists());
        assert!(registry_path(dir.path()).exists());
    }

    #[test]
    fn save_overwrites_existing_registry() {
        let dir = tempdir().expect("tempdir");
        save(dir.path(), &registry_with(vec![entry("a", "One")])).expect("first save");
        let second = registry_with(vec![entry("b", "Two")]);
        save(dir.path(), &second).expect("second save");
        assert_eq!(load(dir.path()).expect("load"), Some(second));
    }

    #[test]
    fn load_unreadable_registry_is_internal_error_not_fresh_install() {
        let dir = tempdir().expect("tempdir");
        // A directory at the registry path fails read_to_string with a
        // non-NotFound error; that must NOT be treated as "no registry yet"
        // (which would silently re-adopt and orphan existing profiles).
        std::fs::create_dir(registry_path(dir.path())).expect("mkdir");
        let err = load(dir.path()).expect_err("must fail");
        assert!(matches!(err, AppError::Internal(_)), "got: {err}");
    }

    #[test]
    fn load_malformed_json_is_internal_error() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(registry_path(dir.path()), "{not json").expect("write");
        let err = load(dir.path()).expect_err("must fail");
        assert!(matches!(err, AppError::Internal(_)), "got: {err}");
    }

    #[test]
    fn load_unsupported_version_is_internal_error() {
        let dir = tempdir().expect("tempdir");
        let mut registry = registry_with(vec![]);
        registry.version = 2;
        let raw = serde_json::to_string(&registry).expect("serialize");
        std::fs::write(registry_path(dir.path()), raw).expect("write");
        let err = load(dir.path()).expect_err("must fail");
        assert!(
            err.to_string().contains("version 2"),
            "message should name the version, got: {err}"
        );
    }

    #[test]
    fn find_returns_matching_entry_only() {
        let registry = registry_with(vec![entry("a", "One"), entry("b", "Two")]);
        assert_eq!(registry.find("b").map(|e| e.name.as_str()), Some("Two"));
        assert_eq!(registry.find("missing"), None);
    }

    #[test_case(vec!["a", "b"], Some("b"), Some("b") ; "last_used_wins")]
    #[test_case(vec!["a", "b"], None, Some("a") ; "no_last_used_falls_back_to_first")]
    #[test_case(vec!["a", "b"], Some("ghost"), Some("a") ; "stale_last_used_falls_back_to_first")]
    #[test_case(vec!["a"], None, Some("a") ; "sole_profile_opens")]
    #[test_case(vec![], None, None ; "empty_registry_shows_the_gate")]
    fn startup_profile_cases(profiles: Vec<&str>, last_used: Option<&str>, expected: Option<&str>) {
        let mut registry = registry_with(profiles.into_iter().map(|id| entry(id, id)).collect());
        registry.last_used = last_used.map(str::to_owned);
        assert_eq!(registry.startup_profile().map(|e| e.id.as_str()), expected);
    }

    #[test]
    fn profile_dir_derives_from_id() {
        let dir = profile_dir(std::path::Path::new("/data"), "abc");
        assert_eq!(dir, std::path::PathBuf::from("/data/profiles/abc"));
    }
}
