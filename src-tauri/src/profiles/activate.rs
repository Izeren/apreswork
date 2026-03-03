// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Profile activation: everything that used to happen unconditionally in
//! `lib.rs::setup()` now runs when a profile is unlocked.
//!
//! Activation installs the profile's [`AppState`] into the process-scoped
//! [`ActiveState`] slot. The REST server and the background timers are
//! process-scoped (started once in `lib.rs::setup()`) and resolve the slot
//! per request/tick, which is what lets [`switch_active_profile`] swap
//! profiles in-process: flush the old profile's backup, empty the slot,
//! activate the new profile into it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::Manager as _;

use crate::calendar;
use crate::error::AppError;
use crate::profiles::registry::{self, ProfileEntry};
use crate::profiles::ActiveProfile;
use crate::scheduler::engine::DefaultScheduler;
use crate::services;
use crate::services::backup::{RestoreOutcome, RestoreSkipReason, KEY_LAST_BACKUP_ERROR};
use crate::state::{ActiveState, AppState};
use crate::traits::backup::BackupTarget;
use crate::traits::calendar_sync::CalendarSync;
use crate::traits::scheduling::Scheduler;
use crate::traits::storage::{ConfigStore as _, Store};

fn calendar_providers(
    sync_provider: Option<&str>,
    profile_dir: &Path,
) -> (Arc<dyn CalendarSync>, Arc<dyn BackupTarget>) {
    calendar::providers_from_config(
        sync_provider,
        calendar::google::GoogleCredentials::compiled(),
        &profile_dir.join("google_auth.json"),
    )
}

/// Assemble an [`AppState`] rooted at a profile directory: open (and
/// migrate) the profile's database, build the scheduler + trigger, and pick
/// the calendar-sync/backup provider pair with the profile-local token path.
/// `restore_notice` carries the startup-restore outcome into the state (the
/// restore itself runs before the store opens; see [`activate_profile`]).
///
/// # Errors
///
/// Returns [`AppError::Internal`] for filesystem/path problems and
/// [`AppError::Database`] when the store cannot open or migrate.
pub fn build_app_state(
    profile_dir: &Path,
    profile: ActiveProfile,
    restore_notice: Option<String>,
) -> Result<AppState, AppError> {
    std::fs::create_dir_all(profile_dir)
        .map_err(|e| AppError::Internal(format!("failed to create profile directory: {e}")))?;
    let db_path = profile_dir.join("apreswork.db");
    let db_path_str = db_path.to_str().ok_or_else(|| {
        AppError::Internal("profile directory path contains invalid UTF-8".into())
    })?;
    let store = Arc::new(crate::db::sqlite::SqliteStore::new(db_path_str)?);
    let scheduler = Arc::new(DefaultScheduler);
    let executor = Arc::new(services::trigger::DefaultExecutor::new(scheduler.clone()));
    let trigger = Arc::new(services::trigger::RescheduleTrigger::new(
        store.clone(),
        executor,
    ));
    let sync_provider = store.get_config_value("sync_provider")?;
    let (calendar_sync, backup) = calendar_providers(sync_provider.as_deref(), profile_dir);
    Ok(AppState {
        store,
        scheduler,
        trigger,
        calendar_sync,
        backup,
        profile_dir: profile_dir.to_path_buf(),
        restore_notice,
        profile,
    })
}

/// Pre-open backup steps: apply a staged manual import if one exists,
/// otherwise run the backup-wins restore check (plans/drive-backup.md
/// Decision 4). Both must happen BEFORE the store opens the database file.
///
/// The store isn't built yet, so the provider key is peeked straight from
/// the DB file and an ephemeral target (dropped after the check) stands in
/// for the one [`build_app_state`] wires later.
fn pre_open_restore(profile_dir: &Path, now: DateTime<Utc>) -> RestoreOutcome {
    match services::backup::apply_pending_import(profile_dir) {
        Ok(true) => return RestoreOutcome::Skipped(RestoreSkipReason::ImportApplied),
        Ok(false) => {}
        Err(e) => {
            // The staged file stays for the next attempt; continue on local.
            log::warn!("backup: staged import failed (continuing on local data): {e}");
        }
    }
    let db_path = profile_dir.join(services::backup::archive::DB_ENTRY_NAME);
    let sync_provider =
        services::backup::archive::read_local_config_value(&db_path, "sync_provider");
    let (_, target) = calendar_providers(sync_provider.as_deref(), profile_dir);
    services::backup::restore_check(profile_dir, target.as_ref(), now)
}

/// Record the startup-restore outcome in the (now open) store: a restore
/// logs and clears any stale backup error, a failure surfaces on the
/// Settings card. Best-effort — bookkeeping must never block activation.
fn persist_restore_bookkeeping(store: &dyn Store, profile_name: &str, outcome: &RestoreOutcome) {
    match outcome {
        RestoreOutcome::Restored { .. } => {
            log::info!("backup: restored profile '{profile_name}' from backup");
            if let Err(e) = store.set_config_value(KEY_LAST_BACKUP_ERROR, "") {
                log::warn!("backup: could not clear the backup error after restore: {e}");
            }
        }
        RestoreOutcome::Failed(msg) => {
            let text = format!("Startup restore failed: {msg}");
            log::warn!("backup: {text}");
            if let Err(e) = store.set_config_value(KEY_LAST_BACKUP_ERROR, &text) {
                log::warn!("backup: could not record the restore failure: {e}");
            }
        }
        RestoreOutcome::Skipped(_) => {}
    }
}

/// Run a full reschedule when none has happened in the last 24 hours.
///
/// # Errors
///
/// Propagates storage and scheduling errors; the caller treats them as
/// best-effort (log + continue).
pub fn startup_auto_reschedule(
    store: &dyn Store,
    scheduler: &dyn Scheduler,
    now: DateTime<Utc>,
) -> Result<bool, AppError> {
    let config = store.get_config()?;
    let needs_reschedule = match config.last_reschedule {
        None => true,
        Some(last) => (now - last).num_hours() >= 24,
    };
    if needs_reschedule {
        services::scheduling::reschedule(store, scheduler, now)?;
    }
    Ok(needs_reschedule)
}

/// The message the second-activation guard returns: unlocking is only for
/// the empty slot; a running profile is replaced via [`switch_active_profile`].
const ALREADY_ACTIVE: &str = "A profile is already active — switch profiles instead.";

/// How long a switch waits for the old profile's final backup export before
/// proceeding (same bound as the graceful-exit flush in `lib.rs`).
const SWITCH_FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// The UI-facing restore notice for an activation outcome: the backup's
/// `last_mutation` as RFC 3339, `""` when the backup carried none, `None`
/// when nothing was restored this run.
fn restore_notice_of(outcome: &RestoreOutcome) -> Option<String> {
    match outcome {
        RestoreOutcome::Restored {
            backup_last_mutation,
        } => Some(backup_last_mutation.map_or_else(String::new, |d| d.to_rfc3339())),
        _ => None,
    }
}

/// The process-scoped [`ActiveState`] slot, managing a fresh one if setup
/// has not run yet (mock apps in tests).
fn active_handle<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> ActiveState {
    if let Some(existing) = app.try_state::<ActiveState>() {
        existing.inner().clone()
    } else {
        let fresh = ActiveState::new();
        app.manage(fresh.clone());
        fresh
    }
}

/// Acquire the process-wide activation guard for the [`ActiveState`] slot,
/// then run `after` while it is still held. Shared preamble for
/// `activate_profile` and `switch_active_profile`: both lock out concurrent
/// activations before touching the slot.
fn with_activation_guard<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    after: impl FnOnce(&ActiveState) -> T,
) -> T {
    let active = active_handle(app);
    let _guard = active.activation_guard();
    after(&active)
}

/// The caller holds the slot's activation guard and has verified the slot
/// is empty — a restore swaps the DB file, which must never happen while
/// an active profile's store has it open.
fn activate_core(
    active: &ActiveState,
    data_dir: &Path,
    entry: &ProfileEntry,
    now: DateTime<Utc>,
) -> Result<ActiveProfile, AppError> {
    let profile = ActiveProfile {
        id: entry.id.clone(),
        name: entry.name.clone(),
    };
    let profile_dir = registry::profile_dir(data_dir, &entry.id);

    let restore_outcome = pre_open_restore(&profile_dir, now);
    let restore_notice = restore_notice_of(&restore_outcome);

    let state = build_app_state(&profile_dir, profile.clone(), restore_notice)?;
    persist_restore_bookkeeping(state.store.as_ref(), &profile.name, &restore_outcome);

    match startup_auto_reschedule(state.store.as_ref(), state.scheduler.as_ref(), now) {
        Ok(ran) => {
            if ran {
                log::info!("startup auto-reschedule ran for profile '{}'", profile.name);
            }
        }
        Err(e) => log::warn!("startup auto-reschedule skipped: {e}"),
    }

    active.swap(Some(Arc::new(state)));
    Ok(profile)
}

/// Activate a profile into the empty [`ActiveState`] slot (first unlock).
///
/// Composition-root glue (thin by design); the buildable/testable parts live
/// in [`build_app_state`], [`startup_auto_reschedule`], and the backup
/// service. The REST server and background timers are process-scoped
/// (started in `lib.rs::setup()`) and pick the new state up on their own.
///
/// # Errors
///
/// Returns [`AppError::Validation`] when a profile is already active in this
/// process, plus anything [`build_app_state`] returns.
pub fn activate_profile<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    data_dir: &Path,
    entry: &ProfileEntry,
    now: DateTime<Utc>,
) -> Result<ActiveProfile, AppError> {
    with_activation_guard(app, |active| {
        if active.get_opt().is_some() {
            return Err(AppError::Validation(ALREADY_ACTIVE.into()));
        }
        activate_core(active, data_dir, entry, now)
    })
}

/// This is the single implementation of the switch policy (architecture
/// invariant §1: one definition per policy). [`switch_active_profile`]
/// delegates to this function after resolving the `ActiveState` from the
/// `AppHandle`.
///
/// The old [`AppState`] drops when its last clone releases (in-flight REST
/// requests and timer ticks resolved their own `Arc` and finish on it).
/// If activating the new profile fails, the slot stays empty and the
/// frontend falls back to the profile gate.
///
/// # Errors
///
/// Propagates anything [`build_app_state`] returns for the new profile.
pub fn switch_active_profile_direct(
    active: &ActiveState,
    data_dir: &Path,
    entry: &ProfileEntry,
    now: DateTime<Utc>,
) -> Result<ActiveProfile, AppError> {
    let _guard = active.activation_guard();
    if let Some(old) = active.swap(None) {
        services::backup::export_on_exit(
            old.store.clone(),
            old.backup.clone(),
            old.profile_dir.clone(),
            SWITCH_FLUSH_TIMEOUT,
            now,
        );
        log::info!("profiles: switched away from '{}'", old.profile.name);
    }
    activate_core(active, data_dir, entry, now)
}

/// Switch the running app to another profile in-process via an
/// `AppHandle`. Resolves the [`ActiveState`] from the handle, then
/// delegates to [`switch_active_profile_direct`].
///
/// # Errors
///
/// Propagates anything [`switch_active_profile_direct`] returns.
pub fn switch_active_profile<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    data_dir: &Path,
    entry: &ProfileEntry,
    now: DateTime<Utc>,
) -> Result<ActiveProfile, AppError> {
    let active = active_handle(app);
    switch_active_profile_direct(&active, data_dir, entry, now)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use tauri::Manager as _;
    use tempfile::tempdir;
    use test_case::test_case;

    use super::{
        activate_profile, build_app_state, persist_restore_bookkeeping, restore_notice_of,
        startup_auto_reschedule, switch_active_profile, switch_active_profile_direct,
    };
    use crate::error::AppError;
    use crate::profiles::registry::test_support::entry;
    use crate::profiles::{registry, ActiveProfile};
    use crate::scheduler::engine::DefaultScheduler;
    use crate::services::backup::{RestoreOutcome, RestoreSkipReason, KEY_LAST_BACKUP_ERROR};
    use crate::state::ActiveState;
    use crate::test_support::test_now;

    fn test_profile(id: &str) -> ActiveProfile {
        ActiveProfile {
            id: id.to_owned(),
            name: format!("Profile {id}"),
        }
    }

    #[test]
    fn build_app_state_creates_db_inside_profile_dir() {
        let dir = tempdir().expect("tempdir");
        let profile_dir = dir.path().join("profiles").join("p1");
        let state = build_app_state(&profile_dir, test_profile("p1"), None).expect("build");
        assert!(profile_dir.join("apreswork.db").exists());
        assert_eq!(state.profile.id, "p1");
        assert_eq!(state.profile_dir, profile_dir);
        assert_eq!(state.restore_notice, None);
    }

    #[test]
    fn build_app_state_carries_the_restore_notice() {
        let dir = tempdir().expect("tempdir");
        let notice = Some("2026-07-12T10:00:00+00:00".to_owned());
        let state = build_app_state(&dir.path().join("p"), test_profile("p"), notice.clone())
            .expect("build");
        assert_eq!(state.restore_notice, notice);
    }

    #[test]
    fn profiles_are_isolated_databases() {
        let dir = tempdir().expect("tempdir");
        let dir_a = dir.path().join("profiles").join("a");
        let dir_b = dir.path().join("profiles").join("b");

        let state_a = build_app_state(&dir_a, test_profile("a"), None).expect("build a");
        // Probe with a key migrations do NOT seed (sync_provider is seeded
        // into every fresh DB by migration 005, so it can't prove isolation).
        state_a
            .store
            .set_config_value("isolation_probe", "from-profile-a")
            .expect("write a");

        let state_b = build_app_state(&dir_b, test_profile("b"), None).expect("build b");
        assert_eq!(
            state_b
                .store
                .get_config_value("isolation_probe")
                .expect("read b"),
            None,
            "profile B must not see profile A's data"
        );
        assert_eq!(
            state_a
                .store
                .get_config_value("isolation_probe")
                .expect("read a"),
            Some("from-profile-a".to_owned())
        );
    }

    #[test]
    fn activate_profile_fills_the_slot_and_rejects_second_activation() {
        let dir = tempdir().expect("tempdir");
        let app = tauri::test::mock_app();
        let profile_entry = entry("p-act", "Activator");

        let active = activate_profile(app.handle(), dir.path(), &profile_entry, test_now())
            .expect("first activation");
        assert_eq!(active.id, "p-act");
        assert_eq!(active.name, "Activator");
        let slot = app.handle().state::<ActiveState>();
        assert_eq!(slot.get().expect("slot filled").profile.id, "p-act");

        let err = activate_profile(app.handle(), dir.path(), &profile_entry, test_now())
            .expect_err("second activation must fail");
        assert!(matches!(err, AppError::Validation(_)));
        assert!(
            err.to_string().contains("switch profiles instead"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn switch_active_profile_swaps_the_slot_between_isolated_profiles() {
        let dir = tempdir().expect("tempdir");
        let app = tauri::test::mock_app();

        activate_profile(app.handle(), dir.path(), &entry("p-one", "One"), test_now())
            .expect("activate one");
        let slot = app.handle().state::<ActiveState>();
        slot.get()
            .expect("one active")
            .store
            .set_config_value("switch_probe", "from-one")
            .expect("seed one");

        let switched =
            switch_active_profile(app.handle(), dir.path(), &entry("p-two", "Two"), test_now())
                .expect("switch to two");
        assert_eq!(switched.id, "p-two");
        let two = slot.get().expect("two active");
        assert_eq!(two.profile.id, "p-two");
        assert_eq!(
            two.store.get_config_value("switch_probe").expect("read"),
            None,
            "profile two must not see profile one's data"
        );

        // Switching back re-opens profile one's database with its data intact.
        switch_active_profile(app.handle(), dir.path(), &entry("p-one", "One"), test_now())
            .expect("switch back");
        let one = slot.get().expect("one active again");
        assert_eq!(
            one.store.get_config_value("switch_probe").expect("read"),
            Some("from-one".to_owned())
        );
    }

    #[test]
    fn switch_active_profile_with_an_empty_slot_acts_as_first_activation() {
        let dir = tempdir().expect("tempdir");
        let app = tauri::test::mock_app();

        let switched = switch_active_profile(
            app.handle(),
            dir.path(),
            &entry("p-cold", "Cold"),
            test_now(),
        )
        .expect("switch on cold start");
        assert_eq!(switched.id, "p-cold");
        let slot = app.handle().state::<ActiveState>();
        assert_eq!(slot.get().expect("slot filled").profile.id, "p-cold");
    }

    #[test_case(None, true ; "never_rescheduled_runs")]
    #[test_case(Some(25), true ; "stale_by_25h_runs")]
    #[test_case(Some(24), true ; "exactly_24h_runs")]
    #[test_case(Some(1), false ; "fresh_1h_skips")]
    fn startup_auto_reschedule_matrix(hours_ago: Option<i64>, expect_run: bool) {
        let dir = tempdir().expect("tempdir");
        let state = build_app_state(&dir.path().join("p"), test_profile("p"), None).expect("build");
        let now = Utc.with_ymd_and_hms(2026, 7, 12, 12, 0, 0).unwrap();
        if let Some(hours) = hours_ago {
            let mut config = state.store.get_config().expect("config");
            config.last_reschedule = Some(now - chrono::TimeDelta::hours(hours));
            state.store.update_config(&config).expect("update");
        }

        let ran = startup_auto_reschedule(state.store.as_ref(), &DefaultScheduler, now)
            .expect("auto reschedule");
        assert_eq!(ran, expect_run);
        if expect_run {
            let config = state.store.get_config().expect("config");
            assert_eq!(config.last_reschedule, Some(now), "reschedule stamps now");
        }
    }

    #[test]
    fn switch_active_profile_direct_on_cold_slot_acts_as_activation() {
        let dir = tempdir().expect("tempdir");
        let active = ActiveState::new();
        let result = switch_active_profile_direct(
            &active,
            dir.path(),
            &entry("p-cold-d", "ColdDirect"),
            test_now(),
        )
        .expect("cold switch");
        assert_eq!(result.id, "p-cold-d");
        assert_eq!(active.get().expect("slot filled").profile.id, "p-cold-d");
    }

    #[test]
    fn switch_active_profile_direct_swaps_slot_between_isolated_profiles() {
        let dir = tempdir().expect("tempdir");
        let active = ActiveState::new();

        switch_active_profile_direct(&active, dir.path(), &entry("p-one-d", "OneD"), test_now())
            .expect("cold switch to one");
        active
            .get()
            .expect("one active")
            .store
            .set_config_value("direct_probe", "from-one")
            .expect("seed one");

        let switched = switch_active_profile_direct(
            &active,
            dir.path(),
            &entry("p-two-d", "TwoD"),
            test_now(),
        )
        .expect("switch to two");
        assert_eq!(switched.id, "p-two-d");
        let two = active.get().expect("two active");
        assert_eq!(two.profile.id, "p-two-d");
        assert_eq!(
            two.store.get_config_value("direct_probe").expect("read"),
            None,
            "profile two must not see profile one's data"
        );

        switch_active_profile_direct(&active, dir.path(), &entry("p-one-d", "OneD"), test_now())
            .expect("switch back to one");
        let one = active.get().expect("one active again");
        assert_eq!(
            one.store.get_config_value("direct_probe").expect("read"),
            Some("from-one".to_owned())
        );
    }

    #[test]
    fn activate_profile_applies_a_staged_import() {
        let dir = tempdir().expect("tempdir");
        let app = tauri::test::mock_app();
        let profile_entry = entry("p-imp", "Importer");
        let profile_dir = registry::profile_dir(dir.path(), "p-imp");

        // Live DB with a marker the import must replace. Scoped so the
        // connection is closed before activation swaps the file.
        {
            let state =
                build_app_state(&profile_dir, test_profile("p-imp"), None).expect("build live");
            state
                .store
                .set_config_value("import_probe", "live")
                .expect("seed live");
        }
        let zip_path = dir.path().join("import.zip");
        {
            let source_dir = dir.path().join("source");
            let source =
                build_app_state(&source_dir, test_profile("src"), None).expect("build source");
            source
                .store
                .set_config_value("import_probe", "imported")
                .expect("seed source");
            crate::services::backup::export_to_file(source.store.as_ref(), &zip_path, &source_dir)
                .expect("export");
        }
        crate::services::backup::stage_import(&zip_path, &profile_dir, test_now())
            .expect("stage import");

        activate_profile(app.handle(), dir.path(), &profile_entry, test_now()).expect("activate");

        let state = app
            .handle()
            .state::<ActiveState>()
            .get()
            .expect("slot filled");
        assert_eq!(
            state
                .store
                .get_config_value("import_probe")
                .expect("read probe"),
            Some("imported".to_owned()),
            "activation must swap in the staged import"
        );
        assert!(
            !profile_dir
                .join(crate::services::backup::archive::PENDING_IMPORT)
                .exists(),
            "the staged file must be consumed"
        );
    }

    #[test_case(
        &RestoreOutcome::Restored { backup_last_mutation: None },
        Some("") ;
        "restored_clears_the_error"
    )]
    #[test_case(
        &RestoreOutcome::Failed("boom".to_owned()),
        Some("Startup restore failed: boom") ;
        "failed_records_the_error"
    )]
    #[test_case(
        &RestoreOutcome::Skipped(RestoreSkipReason::LocalFresh),
        Some("old error") ;
        "skip_leaves_bookkeeping_untouched"
    )]
    fn persist_restore_bookkeeping_matrix(outcome: &RestoreOutcome, expect: Option<&str>) {
        use crate::traits::storage::ConfigStore as _;
        let store = crate::test_support::test_store();
        store
            .set_config_value(KEY_LAST_BACKUP_ERROR, "old error")
            .expect("seed error");

        persist_restore_bookkeeping(&store, "Test", outcome);

        assert_eq!(
            store
                .get_config_value(KEY_LAST_BACKUP_ERROR)
                .expect("read error")
                .as_deref(),
            expect
        );
    }

    #[test]
    fn persist_restore_bookkeeping_survives_a_broken_store() {
        // Bookkeeping is best-effort: a store failure must only log, never
        // panic or propagate (activation continues).
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("apreswork.db");
        let store =
            crate::db::sqlite::SqliteStore::new(db_path.to_str().expect("utf-8")).expect("open");
        let conn = rusqlite::Connection::open(&db_path).expect("second connection");
        conn.execute("DROP TABLE config", [])
            .expect("break the store");
        drop(conn);

        persist_restore_bookkeeping(
            &store,
            "Broken",
            &RestoreOutcome::Restored {
                backup_last_mutation: None,
            },
        );
        persist_restore_bookkeeping(&store, "Broken", &RestoreOutcome::Failed("boom".to_owned()));
    }

    #[test_case(
        &RestoreOutcome::Restored {
            backup_last_mutation: Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).single()
        },
        Some("2026-07-12T10:00:00+00:00") ;
        "restored_with_timestamp"
    )]
    #[test_case(
        &RestoreOutcome::Restored { backup_last_mutation: None },
        Some("") ;
        "restored_without_timestamp"
    )]
    #[test_case(
        &RestoreOutcome::Skipped(RestoreSkipReason::LocalFresh),
        None ;
        "skipped_gives_no_notice"
    )]
    #[test_case(&RestoreOutcome::Failed("boom".to_owned()), None ; "failed_gives_no_notice")]
    fn restore_notice_of_matrix(outcome: &RestoreOutcome, expect: Option<&str>) {
        assert_eq!(restore_notice_of(outcome).as_deref(), expect);
    }
}
