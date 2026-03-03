// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Legacy adoption (M13.4): the first launch of a profiles-aware build turns
//! the pre-profiles database (and Google token) into the "Default" profile.
//!
//! Crash-safety ordering:
//! 1. **Copy** legacy files into the new profile directory (idempotent —
//!    a retry overwrites partial copies).
//! 2. **Write the registry** (atomic rename).
//! 3. **Rename** the originals to `*.pre-profiles-backup` last.
//!
//! A crash before step 2 retries cleanly on the next start (the legacy files
//! are still in place). A crash between 2 and 3 leaves the originals behind;
//! [`load_or_adopt`] renames them on the next start. At no point can a crash
//! hide the legacy data from a retry — which is why the renames come *after*
//! the registry write, not before.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::profiles::registry::{self, ProfileEntry, ProfilesRegistry, REGISTRY_VERSION};

/// Suffix appended to pre-profiles files kept as manual-recovery backups.
pub const LEGACY_BACKUP_SUFFIX: &str = ".pre-profiles-backup";

/// Name seeded for the adopted (or fresh-install) first profile.
pub const DEFAULT_PROFILE_NAME: &str = "Default";

const DB_FILES: [&str; 3] = ["apreswork.db", "apreswork.db-wal", "apreswork.db-shm"];
const TOKEN_FILE: &str = "google_auth.json";

/// Load the registry, adopting legacy data on first run.
///
/// - Registry exists → load it and rename any leftover legacy files (a
///   previous run may have crashed between the registry write and the
///   renames).
/// - No registry → create the "Default" profile; when a pre-profiles
///   `apreswork.db` exists in `data_dir` it is copied into the profile
///   directory together with the Google token from `config_dir` (the Google
///   Calendar integration shipped the token at the config-dir path before
///   profiles existed).
///
/// # Errors
///
/// Returns [`AppError::Internal`] on any filesystem or serialization failure;
/// the caller should surface it rather than continue with ambiguous state.
pub fn load_or_adopt(
    data_dir: &Path,
    config_dir: &Path,
    now: DateTime<Utc>,
) -> Result<ProfilesRegistry, AppError> {
    if let Some(existing) = registry::load(data_dir)? {
        rename_legacy_to_backup(data_dir, config_dir)?;
        return Ok(existing);
    }

    let entry = ProfileEntry {
        id: uuid::Uuid::now_v7().to_string(),
        name: DEFAULT_PROFILE_NAME.to_owned(),
        created_at: now,
    };
    let profile_dir = registry::profile_dir(data_dir, &entry.id);
    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| AppError::Internal(format!("failed to create profile directory: {e}")))?;
    copy_legacy_files(data_dir, config_dir, &profile_dir)?;

    let created = ProfilesRegistry {
        version: REGISTRY_VERSION,
        last_used: Some(entry.id.clone()),
        profiles: vec![entry],
    };
    registry::save(data_dir, &created)?;
    rename_legacy_to_backup(data_dir, config_dir)?;
    Ok(created)
}

/// All legacy file locations: the DB trio in the data dir plus the Google
/// token in the config dir.
fn legacy_paths(data_dir: &Path, config_dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = DB_FILES.iter().map(|name| data_dir.join(name)).collect();
    paths.push(config_dir.join(TOKEN_FILE));
    paths
}

fn copy_legacy_files(
    data_dir: &Path,
    config_dir: &Path,
    profile_dir: &Path,
) -> Result<(), AppError> {
    for src in legacy_paths(data_dir, config_dir) {
        if !src.exists() {
            continue;
        }
        // `src` always has a final component — it was built by join above.
        let name = src.file_name().unwrap_or_default();
        std::fs::copy(&src, profile_dir.join(name)).map_err(|e| {
            AppError::Internal(format!(
                "failed to copy legacy file {} into profile: {e}",
                src.display()
            ))
        })?;
    }
    Ok(())
}

/// Rename leftover pre-profiles files to `*.pre-profiles-backup`.
///
/// A pre-existing backup of the same name is overwritten — after adoption the
/// canonical data lives in the profile directory, so the newest leftover is
/// the one worth keeping.
fn rename_legacy_to_backup(data_dir: &Path, config_dir: &Path) -> Result<(), AppError> {
    for src in legacy_paths(data_dir, config_dir) {
        if !src.exists() {
            continue;
        }
        let mut backup = src.as_os_str().to_os_string();
        backup.push(LEGACY_BACKUP_SUFFIX);
        std::fs::rename(&src, &backup).map_err(|e| {
            AppError::Internal(format!(
                "failed to move legacy file {} aside: {e}",
                src.display()
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use tempfile::{tempdir, TempDir};

    use super::{load_or_adopt, DEFAULT_PROFILE_NAME, LEGACY_BACKUP_SUFFIX};
    use crate::profiles::registry::{self, ProfilesRegistry, REGISTRY_VERSION};

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
    }

    fn dirs() -> (TempDir, TempDir) {
        (tempdir().expect("data dir"), tempdir().expect("config dir"))
    }

    /// Run `load_or_adopt` and return the directory created for the first profile.
    fn adopted_profile_dir(data: &std::path::Path, config: &std::path::Path) -> std::path::PathBuf {
        let registry = load_or_adopt(data, config, now()).expect("adopt");
        registry::profile_dir(data, &registry.profiles[0].id)
    }

    fn write_legacy_db(data_dir: &std::path::Path) {
        std::fs::write(data_dir.join("apreswork.db"), b"main-db-bytes").expect("db");
        std::fs::write(data_dir.join("apreswork.db-wal"), b"wal-bytes").expect("wal");
    }

    #[test]
    fn fresh_install_creates_single_default_profile() {
        let (data, config) = dirs();
        let registry = load_or_adopt(data.path(), config.path(), now()).expect("adopt");

        assert_eq!(registry.profiles.len(), 1);
        let profile = &registry.profiles[0];
        assert_eq!(profile.name, DEFAULT_PROFILE_NAME);
        assert_eq!(registry.last_used.as_deref(), Some(profile.id.as_str()));
        // Profile dir exists and is empty (migrations create the DB later).
        let dir = registry::profile_dir(data.path(), &profile.id);
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).expect("read dir").count(), 0);
        assert_eq!(
            registry::load(data.path()).expect("load"),
            Some(registry.clone())
        );
    }

    #[test]
    fn legacy_db_and_token_are_copied_and_originals_backed_up() {
        let (data, config) = dirs();
        write_legacy_db(data.path());
        std::fs::write(config.path().join("google_auth.json"), b"token-bytes").expect("token");

        let profile_dir = adopted_profile_dir(data.path(), config.path());

        assert_eq!(
            std::fs::read(profile_dir.join("apreswork.db")).expect("db copy"),
            b"main-db-bytes"
        );
        assert_eq!(
            std::fs::read(profile_dir.join("apreswork.db-wal")).expect("wal copy"),
            b"wal-bytes"
        );
        assert_eq!(
            std::fs::read(profile_dir.join("google_auth.json")).expect("token copy"),
            b"token-bytes"
        );
        for (dir, name) in [
            (data.path(), "apreswork.db"),
            (data.path(), "apreswork.db-wal"),
            (config.path(), "google_auth.json"),
        ] {
            assert!(!dir.join(name).exists(), "{name} should be moved aside");
            assert!(
                dir.join(format!("{name}{LEGACY_BACKUP_SUFFIX}")).exists(),
                "{name} backup should exist"
            );
        }
        // -shm never existed; no phantom backup.
        assert!(!data
            .path()
            .join(format!("apreswork.db-shm{LEGACY_BACKUP_SUFFIX}"))
            .exists());
    }

    #[test]
    fn second_run_loads_existing_registry_without_new_profiles() {
        let (data, config) = dirs();
        write_legacy_db(data.path());
        let first = load_or_adopt(data.path(), config.path(), now()).expect("first");
        let second = load_or_adopt(data.path(), config.path(), now()).expect("second");
        assert_eq!(first, second);
    }

    #[test]
    fn crash_between_registry_write_and_rename_is_cleaned_up_on_next_start() {
        let (data, config) = dirs();
        // Simulate: registry written, but legacy files never renamed.
        let registry = ProfilesRegistry {
            version: REGISTRY_VERSION,
            last_used: None,
            profiles: vec![registry::test_support::entry("p1", "Default")],
        };
        registry::save(data.path(), &registry).expect("save");
        write_legacy_db(data.path());

        let loaded = load_or_adopt(data.path(), config.path(), now()).expect("load");
        assert_eq!(loaded, registry, "no new profile may be invented");
        assert!(!data.path().join("apreswork.db").exists());
        assert!(data
            .path()
            .join(format!("apreswork.db{LEGACY_BACKUP_SUFFIX}"))
            .exists());
    }

    #[test]
    fn crash_before_registry_write_retries_cleanly() {
        let (data, config) = dirs();
        write_legacy_db(data.path());
        // Simulate a crashed first attempt: an orphan profile dir with a
        // partial copy, no registry.
        let orphan = registry::profile_dir(data.path(), "orphan-id");
        std::fs::create_dir_all(&orphan).expect("orphan dir");
        std::fs::write(orphan.join("apreswork.db"), b"partial").expect("partial copy");

        let profile_dir = adopted_profile_dir(data.path(), config.path());
        assert_eq!(
            std::fs::read(profile_dir.join("apreswork.db")).expect("db copy"),
            b"main-db-bytes",
            "retry must adopt the real legacy DB"
        );
    }
}
