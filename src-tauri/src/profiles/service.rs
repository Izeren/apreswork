// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Registry-mutating operations behind the profile commands.
//!
//! Free functions over `&mut ProfilesRegistry` + the data dir so the command
//! layer stays thin and everything is testable without a Tauri runtime.
//! Every mutating function persists the registry (atomic save) before
//! returning `Ok`.

use std::path::Path;

use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::profiles::registry::{self, ProfileEntry, ProfilesRegistry};

fn validate_profile_name(name: &str) -> Result<&str, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation(
            "Profile name must not be empty.".into(),
        ));
    }
    Ok(name)
}

fn check_name_not_duplicate(
    registry: &ProfilesRegistry,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<(), AppError> {
    let duplicate = registry
        .profiles
        .iter()
        .any(|p| exclude_id.is_none_or(|ex| p.id != ex) && p.name.eq_ignore_ascii_case(name));
    if duplicate {
        return Err(AppError::Validation(format!(
            "A profile named '{name}' already exists."
        )));
    }
    Ok(())
}

/// Find `id`'s index in `registry.profiles`, or [`AppError::NotFound`].
/// Shared by [`rename_profile`] and [`delete_profile`], which both need the
/// index (not just the entry) to mutate/remove in place.
fn find_profile_index(registry: &ProfilesRegistry, id: &str) -> Result<usize, AppError> {
    registry
        .profiles
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| AppError::NotFound {
            entity: "Profile".to_owned(),
            id: id.to_owned(),
        })
}

/// Create a profile: validated name, fresh uuid7 id, and an empty profile
/// directory. Appends to the registry and saves it.
///
/// # Errors
///
/// Returns [`AppError::Validation`] for an empty or duplicate name (case
/// insensitive) and [`AppError::Internal`] on filesystem failure.
pub fn create_profile(
    registry: &mut ProfilesRegistry,
    data_dir: &Path,
    name: &str,
    now: DateTime<Utc>,
) -> Result<ProfileEntry, AppError> {
    let name = validate_profile_name(name)?;
    check_name_not_duplicate(registry, name, None)?;
    let entry = ProfileEntry {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_owned(),
        created_at: now,
    };
    std::fs::create_dir_all(registry::profile_dir(data_dir, &entry.id))
        .map_err(|e| AppError::Internal(format!("failed to create profile directory: {e}")))?;
    registry.profiles.push(entry.clone());
    registry::save(data_dir, registry)?;
    Ok(entry)
}

/// Rename a profile: same name validation as [`create_profile`], with the
/// profile itself excluded from the duplicate check (so a pure case change
/// like "default" → "Default" is allowed).
///
/// # Errors
///
/// Returns [`AppError::NotFound`] for an unknown id and
/// [`AppError::Validation`] for an empty or duplicate name.
pub fn rename_profile(
    registry: &mut ProfilesRegistry,
    data_dir: &Path,
    id: &str,
    new_name: &str,
) -> Result<ProfileEntry, AppError> {
    let new_name = validate_profile_name(new_name)?;
    let idx = find_profile_index(registry, id)?;
    check_name_not_duplicate(registry, new_name, Some(id))?;
    new_name.clone_into(&mut registry.profiles[idx].name);
    registry::save(data_dir, registry)?;
    Ok(registry.profiles[idx].clone())
}

/// Delete a profile: its data directory is removed first, then the registry
/// entry. Deletion destroys the profile's data — the UI confirms before
/// calling this. If the directory removal fails the registry is left
/// untouched, so the profile stays listed and the deletion can be retried.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] for an unknown id and
/// [`AppError::Internal`] on filesystem failure.
pub fn delete_profile(
    registry: &mut ProfilesRegistry,
    data_dir: &Path,
    id: &str,
) -> Result<(), AppError> {
    let idx = find_profile_index(registry, id)?;
    let profile_dir = registry::profile_dir(data_dir, id);
    if profile_dir.is_dir() {
        std::fs::remove_dir_all(&profile_dir)
            .map_err(|e| AppError::Internal(format!("failed to remove profile directory: {e}")))?;
    }
    registry.profiles.remove(idx);
    if registry.last_used.as_deref() == Some(id) {
        registry.last_used = None;
    }
    registry::save(data_dir, registry)
}

/// Record `id` as the profile to target on the next startup and save.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] for an unknown id.
pub fn mark_last_used(
    registry: &mut ProfilesRegistry,
    data_dir: &Path,
    id: &str,
) -> Result<(), AppError> {
    if registry.find(id).is_none() {
        return Err(AppError::NotFound {
            entity: "Profile".to_owned(),
            id: id.to_owned(),
        });
    }
    registry.last_used = Some(id.to_owned());
    registry::save(data_dir, registry)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use tempfile::{tempdir, TempDir};
    use test_case::test_case;

    use super::{create_profile, delete_profile, mark_last_used, rename_profile};
    use crate::error::AppError;
    use crate::profiles::registry::{self, ProfileEntry, ProfilesRegistry};

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap()
    }

    fn empty_registry() -> (TempDir, ProfilesRegistry) {
        (
            tempdir().expect("tempdir"),
            ProfilesRegistry {
                version: 1,
                last_used: None,
                profiles: vec![],
            },
        )
    }

    /// A fresh registry with one profile ("Bob") created and marked as
    /// last-used. Shared setup for tests that need a pre-marked profile.
    fn registry_with_marked_profile() -> (TempDir, ProfilesRegistry, ProfileEntry) {
        let (dir, mut registry) = empty_registry();
        let created = create_profile(&mut registry, dir.path(), "Bob", now()).expect("create");
        mark_last_used(&mut registry, dir.path(), &created.id).expect("mark");
        (dir, registry, created)
    }

    #[test]
    fn create_profile_persists_and_creates_dir() {
        let (dir, mut registry) = empty_registry();
        let created = create_profile(&mut registry, dir.path(), "  Bob  ", now()).expect("create");

        assert_eq!(created.name, "Bob", "name is trimmed");
        assert!(registry::profile_dir(dir.path(), &created.id).is_dir());
        let persisted = registry::load(dir.path()).expect("load").expect("present");
        assert_eq!(persisted, registry);
        assert_eq!(persisted.profiles.len(), 1);
    }

    #[test_case("" ; "empty_name")]
    #[test_case("   " ; "blank_name")]
    fn create_profile_rejects_empty_names(name: &str) {
        let (dir, mut registry) = empty_registry();
        let err = create_profile(&mut registry, dir.path(), name, now()).expect_err("must reject");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(registry.profiles.is_empty(), "nothing may be appended");
    }

    #[test_case("Bob" ; "exact_duplicate")]
    #[test_case("bob" ; "case_insensitive_duplicate")]
    fn create_profile_rejects_duplicate_names(duplicate: &str) {
        let (dir, mut registry) = empty_registry();
        create_profile(&mut registry, dir.path(), "Bob", now()).expect("first");
        let err =
            create_profile(&mut registry, dir.path(), duplicate, now()).expect_err("must reject");
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[test]
    fn rename_profile_trims_persists_and_keeps_id() {
        let (dir, mut registry) = empty_registry();
        let created = create_profile(&mut registry, dir.path(), "Default", now()).expect("create");

        let renamed =
            rename_profile(&mut registry, dir.path(), &created.id, "  Alice  ").expect("rename");
        assert_eq!(renamed.name, "Alice", "name is trimmed");
        assert_eq!(renamed.id, created.id);

        let persisted = registry::load(dir.path()).expect("load").expect("present");
        assert_eq!(persisted.profiles[0].name, "Alice");
    }

    #[test]
    fn rename_profile_allows_pure_case_change_of_itself() {
        let (dir, mut registry) = empty_registry();
        let created = create_profile(&mut registry, dir.path(), "default", now()).expect("create");
        let renamed =
            rename_profile(&mut registry, dir.path(), &created.id, "Default").expect("rename");
        assert_eq!(renamed.name, "Default");
    }

    #[test_case("" ; "empty_name")]
    #[test_case("   " ; "blank_name")]
    fn rename_profile_rejects_empty_names(name: &str) {
        let (dir, mut registry) = empty_registry();
        let created = create_profile(&mut registry, dir.path(), "Default", now()).expect("create");
        let err =
            rename_profile(&mut registry, dir.path(), &created.id, name).expect_err("must reject");
        assert!(matches!(err, AppError::Validation(_)));
        assert_eq!(registry.profiles[0].name, "Default", "name unchanged");
    }

    #[test_case("Bob" ; "exact_duplicate")]
    #[test_case("bob" ; "case_insensitive_duplicate")]
    fn rename_profile_rejects_other_profiles_names(duplicate: &str) {
        let (dir, mut registry) = empty_registry();
        let target = create_profile(&mut registry, dir.path(), "Default", now()).expect("first");
        create_profile(&mut registry, dir.path(), "Bob", now()).expect("second");
        let err = rename_profile(&mut registry, dir.path(), &target.id, duplicate)
            .expect_err("must reject");
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[test]
    fn rename_profile_rejects_unknown_id() {
        let (dir, mut registry) = empty_registry();
        let err = rename_profile(&mut registry, dir.path(), "ghost", "Anything")
            .expect_err("must reject");
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[test]
    fn delete_profile_removes_entry_directory_and_last_used() {
        let (dir, mut registry, created) = registry_with_marked_profile();

        delete_profile(&mut registry, dir.path(), &created.id).expect("delete");

        assert!(registry.profiles.is_empty());
        assert_eq!(registry.last_used, None, "stale pointer cleared");
        assert!(!registry::profile_dir(dir.path(), &created.id).exists());
        let persisted = registry::load(dir.path()).expect("load").expect("present");
        assert_eq!(persisted, registry);
    }

    #[test]
    fn delete_profile_keeps_other_profiles_and_their_last_used() {
        let (dir, mut registry) = empty_registry();
        let keep = create_profile(&mut registry, dir.path(), "Keep", now()).expect("keep");
        let doomed = create_profile(&mut registry, dir.path(), "Doomed", now()).expect("doomed");
        mark_last_used(&mut registry, dir.path(), &keep.id).expect("mark");

        delete_profile(&mut registry, dir.path(), &doomed.id).expect("delete");

        assert_eq!(registry.profiles.len(), 1);
        assert_eq!(registry.profiles[0].id, keep.id);
        assert_eq!(
            registry.last_used,
            Some(keep.id.clone()),
            "unrelated pointer kept"
        );
        assert!(registry::profile_dir(dir.path(), &keep.id).is_dir());
    }

    #[test]
    fn delete_profile_tolerates_missing_directory() {
        let (dir, mut registry) = empty_registry();
        let created = create_profile(&mut registry, dir.path(), "Bob", now()).expect("create");
        std::fs::remove_dir_all(registry::profile_dir(dir.path(), &created.id))
            .expect("pre-remove");
        delete_profile(&mut registry, dir.path(), &created.id).expect("delete");
        assert!(registry.profiles.is_empty());
    }

    #[test]
    fn delete_profile_rejects_unknown_id() {
        let (dir, mut registry) = empty_registry();
        let err = delete_profile(&mut registry, dir.path(), "ghost").expect_err("must reject");
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[test]
    fn mark_last_used_persists_selection() {
        let (dir, _registry, created) = registry_with_marked_profile();
        let persisted = registry::load(dir.path()).expect("load").expect("present");
        assert_eq!(persisted.last_used, Some(created.id));
    }

    #[test]
    fn mark_last_used_rejects_unknown_id() {
        let (dir, mut registry) = empty_registry();
        let err = mark_last_used(&mut registry, dir.path(), "ghost").expect_err("must reject");
        assert!(matches!(err, AppError::NotFound { .. }));
    }
}
