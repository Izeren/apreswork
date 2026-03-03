// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Backup service tests: a mock target + a real in-memory store exercise the
//! export gates, the stale-writer guard, the restore decision matrix, and
//! the manual file export/import flow. The archive verify/swap plumbing is
//! covered in `archive_tests`, which reuses this module's fixtures.

mod archive_tests;

use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tempfile::tempdir;
use test_case::test_case;

use super::archive::{self, DB_ENTRY_NAME, PENDING_IMPORT};
use super::{
    apply_pending_import, export_if_dirty, export_now, export_on_exit, export_to_file,
    get_backup_status, maybe_export, restore_check, set_backup_enabled, stage_import,
    start_backup_timer, BackupContext, ExportOutcome, RestoreOutcome, RestoreSkipReason,
    KEY_BACKUP_ENABLED, KEY_BACKUP_INTERVAL_MINUTES, KEY_LAST_BACKUP_ERROR, KEY_LAST_EXPORT_AT,
};
use crate::db::migrations;
use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::test_support::{test_now, test_store, utc};
use crate::traits::backup::{BackupTarget, RemoteBackupMeta};
use crate::traits::storage::{ConfigStore as _, Store};

struct MockBackupTarget {
    available: bool,
    meta: Result<Option<RemoteBackupMeta>, String>,
    meta_delay: Duration,
    download: Result<Vec<u8>, String>,
    upload_error: Option<String>,
    uploads: Mutex<Vec<(Vec<u8>, RemoteBackupMeta)>>,
}

impl MockBackupTarget {
    fn available() -> Self {
        Self {
            available: true,
            meta: Ok(None),
            meta_delay: Duration::ZERO,
            download: Err("no backup on target".to_owned()),
            upload_error: None,
            uploads: Mutex::new(Vec::new()),
        }
    }

    fn unavailable() -> Self {
        Self {
            available: false,
            ..Self::available()
        }
    }

    fn with_meta(mut self, meta: Option<RemoteBackupMeta>) -> Self {
        self.meta = Ok(meta);
        self
    }

    fn with_meta_error(mut self, msg: &str) -> Self {
        self.meta = Err(msg.to_owned());
        self
    }

    /// Make every freshness probe hang for `delay` (stuck-network simulation).
    fn with_meta_delay(mut self, delay: Duration) -> Self {
        self.meta_delay = delay;
        self
    }

    fn with_download(mut self, bytes: Vec<u8>) -> Self {
        self.download = Ok(bytes);
        self
    }

    fn with_upload_error(mut self, msg: &str) -> Self {
        self.upload_error = Some(msg.to_owned());
        self
    }

    fn with_uploads<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[(Vec<u8>, RemoteBackupMeta)]) -> R,
    {
        f(&self.uploads.lock().expect("uploads lock"))
    }
}

impl BackupTarget for MockBackupTarget {
    fn is_available(&self) -> bool {
        self.available
    }

    fn get_meta(&self, _now: DateTime<Utc>) -> Result<Option<RemoteBackupMeta>, AppError> {
        if !self.meta_delay.is_zero() {
            std::thread::sleep(self.meta_delay);
        }
        self.meta.clone().map_err(AppError::Backup)
    }

    fn upload(
        &self,
        _now: DateTime<Utc>,
        zip_bytes: &[u8],
        meta: &RemoteBackupMeta,
    ) -> Result<(), AppError> {
        if let Some(msg) = &self.upload_error {
            return Err(AppError::Backup(msg.clone()));
        }
        self.uploads
            .lock()
            .expect("uploads lock")
            .push((zip_bytes.to_vec(), meta.clone()));
        Ok(())
    }

    fn download(&self, _now: DateTime<Utc>) -> Result<Vec<u8>, AppError> {
        self.download.clone().map_err(AppError::Backup)
    }
}

fn test_dir() -> tempfile::TempDir {
    tempdir().expect("test tempdir")
}

fn seeded_dir_10h() -> tempfile::TempDir {
    let dir = test_dir();
    seed_profile_db(dir.path(), true, Some(utc(2026, 7, 12, 10, 0)));
    dir
}

fn remote_meta(last_mutation: Option<DateTime<Utc>>) -> RemoteBackupMeta {
    RemoteBackupMeta {
        last_mutation,
        schema_version: migrations::current_version(),
    }
}

fn set_last_mutation(store: &SqliteStore, last_mutation: Option<DateTime<Utc>>) {
    let mut config = store.get_config().expect("config");
    config.last_mutation = last_mutation;
    store.update_config(&config).expect("update config");
}

/// An opted-in store with the given freshness/bookkeeping state.
fn enabled_store(
    last_mutation: Option<DateTime<Utc>>,
    last_export: Option<DateTime<Utc>>,
) -> SqliteStore {
    let store = test_store();
    store
        .set_config_value(KEY_BACKUP_ENABLED, "true")
        .expect("enable");
    set_last_mutation(&store, last_mutation);
    if let Some(at) = last_export {
        store
            .set_config_value(KEY_LAST_EXPORT_AT, &at.to_rfc3339())
            .expect("stamp export");
    }
    store
}

/// A real backup archive whose embedded database has `last_mutation`.
fn valid_backup_zip(last_mutation: Option<DateTime<Utc>>) -> Vec<u8> {
    let store = test_store();
    set_last_mutation(&store, last_mutation);
    let work = test_dir();
    archive::build_backup_zip(&store, work.path()).expect("zip")
}

/// Materialize a migrated profile database on disk for the pre-open peeks.
fn seed_profile_db(profile_dir: &Path, backup_enabled: bool, last_mutation: Option<DateTime<Utc>>) {
    let store = test_store();
    if backup_enabled {
        store
            .set_config_value(KEY_BACKUP_ENABLED, "true")
            .expect("enable");
    }
    set_last_mutation(&store, last_mutation);
    store
        .vacuum_into(&profile_dir.join(DB_ENTRY_NAME))
        .expect("seed profile db");
}

/// A zip built in-test with arbitrary entries (for malformed-archive cases).
fn zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        writer.start_file(*name, options).expect("start entry");
        writer.write_all(bytes).expect("write entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

fn config_value(store: &SqliteStore, key: &str) -> Option<String> {
    store.get_config_value(key).expect("config value")
}

fn target_with_newer_backup() -> MockBackupTarget {
    MockBackupTarget::available()
        .with_meta(Some(remote_meta(Some(utc(2026, 7, 12, 12, 0)))))
        .with_download(valid_backup_zip(Some(utc(2026, 7, 12, 12, 0))))
}

fn write_import_zip(dir: &Path, stamp: DateTime<Utc>) -> std::path::PathBuf {
    let zip_path = dir.join("import.zip");
    std::fs::write(&zip_path, valid_backup_zip(Some(stamp))).expect("write");
    zip_path
}

/// A temp dir holding `import.zip` = a valid backup archive stamped at 09:00.
fn dir_with_import_zip() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = test_dir();
    let zip_path = write_import_zip(dir.path(), utc(2026, 7, 12, 9, 0));
    (dir, zip_path)
}

fn assert_restore_skipped(dir: &Path, target: &MockBackupTarget, reason: RestoreSkipReason) {
    assert_eq!(
        restore_check(dir, target, test_now()),
        RestoreOutcome::Skipped(reason)
    );
}

fn export_now_at_noon(store: &SqliteStore, target: &MockBackupTarget) -> ExportOutcome {
    let work = test_dir();
    export_now(store, target, work.path(), utc(2026, 7, 12, 12, 0)).expect("export")
}

#[test]
fn get_backup_status_reads_fresh_store_as_disabled_and_empty() {
    let store = test_store();
    let status = get_backup_status(&store, &MockBackupTarget::unavailable(), None).expect("status");
    assert!(!status.enabled);
    assert!(!status.connected);
    assert_eq!(status.last_export_at, None);
    assert_eq!(status.last_backup_error, None);
    assert_eq!(status.restored_this_run, None);
}

#[test]
fn get_backup_status_is_lenient_and_passes_the_restore_notice_through() {
    let store = enabled_store(None, None);
    store
        .set_config_value(KEY_LAST_EXPORT_AT, "not-a-date")
        .expect("garbage stamp");
    store
        .set_config_value(KEY_LAST_BACKUP_ERROR, "   ")
        .expect("blank error");

    let status = get_backup_status(&store, &MockBackupTarget::available(), Some("2026-07-12"))
        .expect("status");
    assert!(status.enabled);
    assert!(status.connected);
    assert_eq!(
        status.last_export_at, None,
        "garbage timestamp reads as None"
    );
    assert_eq!(status.last_backup_error, None, "blank error reads as None");
    assert_eq!(status.restored_this_run.as_deref(), Some("2026-07-12"));
}

#[test]
fn set_backup_enabled_requires_a_connected_target_to_enable() {
    let store = test_store();
    let err = set_backup_enabled(&store, &MockBackupTarget::unavailable(), true)
        .expect_err("must reject");
    assert!(matches!(err, AppError::Validation(_)), "got: {err}");
    assert_eq!(
        config_value(&store, KEY_BACKUP_ENABLED),
        None,
        "nothing persisted"
    );

    set_backup_enabled(&store, &MockBackupTarget::available(), true).expect("enable");
    assert_eq!(
        config_value(&store, KEY_BACKUP_ENABLED).as_deref(),
        Some("true")
    );

    set_backup_enabled(&store, &MockBackupTarget::unavailable(), false).expect("disable");
    assert_eq!(
        config_value(&store, KEY_BACKUP_ENABLED).as_deref(),
        Some("false")
    );
}

#[test]
fn export_now_rejects_an_unavailable_target() {
    let store = test_store();
    let work = test_dir();
    let err = export_now(
        &store,
        &MockBackupTarget::unavailable(),
        work.path(),
        utc(2026, 7, 12, 12, 0),
    )
    .expect_err("must reject");
    assert!(matches!(err, AppError::Validation(_)), "got: {err}");
}

#[test]
fn export_now_skips_and_warns_when_the_remote_backup_is_newer() {
    let store = enabled_store(Some(utc(2026, 7, 12, 10, 0)), None);
    let target =
        MockBackupTarget::available().with_meta(Some(remote_meta(Some(utc(2026, 7, 12, 11, 0)))));

    let outcome = export_now_at_noon(&store, &target);

    assert_eq!(outcome, ExportOutcome::SkippedStaleRemote);
    assert!(
        target.with_uploads(<[_]>::is_empty),
        "nothing may be uploaded"
    );
    let error = config_value(&store, KEY_LAST_BACKUP_ERROR).expect("warning recorded");
    assert!(error.contains("restart to pull"), "got: {error}");
    assert_eq!(
        config_value(&store, KEY_LAST_EXPORT_AT),
        None,
        "no export stamp"
    );
}

#[test_case(None ; "no_remote_backup")]
#[test_case(Some(None) ; "remote_never_mutated")]
#[test_case(Some(Some(utc(2026, 7, 12,9, 0))) ; "remote_older")]
#[test_case(Some(Some(utc(2026, 7, 12,10, 0))) ; "remote_equal")]
// Three distinct fixtures: no remote file / remote without last_mutation /
// remote at T — mirrors `RemoteBackupMeta::last_mutation` being optional.
#[allow(clippy::option_option)]
fn export_now_uploads_over_an_older_or_absent_backup(
    remote_mutation: Option<Option<DateTime<Utc>>>,
) {
    let store = enabled_store(Some(utc(2026, 7, 12, 10, 0)), None);
    store
        .set_config_value(KEY_LAST_BACKUP_ERROR, "previous failure")
        .expect("seed error");
    let target = MockBackupTarget::available().with_meta(remote_mutation.map(remote_meta));

    let outcome = export_now_at_noon(&store, &target);

    assert_eq!(outcome, ExportOutcome::Uploaded);
    target.with_uploads(|u| {
        assert_eq!(u.len(), 1);
        let (zip_bytes, meta) = &u[0];
        assert_eq!(
            meta.last_mutation,
            Some(utc(2026, 7, 12, 10, 0)),
            "meta mirrors the local database"
        );
        assert_eq!(meta.schema_version, migrations::current_version());
        let verify_dir = test_dir();
        archive::extract_and_verify(zip_bytes, verify_dir.path(), migrations::current_version())
            .expect("uploaded archive must verify");
    });
    assert_eq!(
        config_value(&store, KEY_LAST_EXPORT_AT).as_deref(),
        Some(utc(2026, 7, 12, 12, 0).to_rfc3339().as_str()),
        "export stamped"
    );
    assert_eq!(
        config_value(&store, KEY_LAST_BACKUP_ERROR).as_deref(),
        Some(""),
        "error cleared"
    );
}

fn run_failing_export(target: &MockBackupTarget) -> (AppError, SqliteStore) {
    let store = enabled_store(Some(utc(2026, 7, 12, 10, 0)), None);
    let work = test_dir();
    let err =
        export_now(&store, target, work.path(), utc(2026, 7, 12, 12, 0)).expect_err("must fail");
    (err, store)
}

#[test]
fn export_now_records_an_upload_failure_and_propagates_it() {
    let target = MockBackupTarget::available().with_upload_error("drive said no");
    let (err, store) = run_failing_export(&target);
    assert!(err.to_string().contains("drive said no"), "got: {err}");
    let recorded = config_value(&store, KEY_LAST_BACKUP_ERROR).expect("error recorded");
    assert!(recorded.contains("drive said no"), "got: {recorded}");
}

#[test]
fn export_now_records_a_freshness_probe_failure() {
    let target = MockBackupTarget::available().with_meta_error("probe timeout");
    let (err, _store) = run_failing_export(&target);
    assert!(err.to_string().contains("probe timeout"), "got: {err}");
    assert!(
        target.with_uploads(<[_]>::is_empty),
        "must not upload blind"
    );
}

#[test_case(|| {
    let store = test_store();
    set_last_mutation(&store, Some(utc(2026, 7, 12, 10, 0)));
    (store, MockBackupTarget::available())
} ; "backup_disabled")]
#[test_case(|| {
    let store = enabled_store(Some(utc(2026, 7, 12, 10, 0)), None);
    (store, MockBackupTarget::unavailable())
} ; "target_unavailable")]
#[allow(clippy::needless_pass_by_value)]
fn maybe_export_skips_when_preconditions_fail(make_pair: fn() -> (SqliteStore, MockBackupTarget)) {
    let (store, target) = make_pair();
    let work = test_dir();
    let outcome =
        maybe_export(&store, &target, work.path(), utc(2026, 7, 12, 12, 0)).expect("gate");
    assert_eq!(outcome, None);
    assert!(target.with_uploads(<[_]>::is_empty));
}

#[test_case(None, None, None ; "never_mutated_never_exported")]
#[test_case(Some(utc(2026, 7, 12,9, 0)), Some(utc(2026, 7, 12,10, 0)), None ; "clean_since_export")]
#[test_case(Some(utc(2026, 7, 12,10, 3)), Some(utc(2026, 7, 12,10, 0)), None ; "dirty_but_interval_not_elapsed")]
#[test_case(Some(utc(2026, 7, 12,10, 3)), None, Some(ExportOutcome::Uploaded) ; "dirty_never_exported")]
#[test_case(Some(utc(2026, 7, 12,10, 3)), Some(utc(2026, 7, 12,9, 55)), Some(ExportOutcome::Uploaded) ; "dirty_and_elapsed")]
#[allow(clippy::needless_pass_by_value)]
fn maybe_export_gate_matrix(
    last_mutation: Option<DateTime<Utc>>,
    last_export: Option<DateTime<Utc>>,
    expected: Option<ExportOutcome>,
) {
    // now = 10:04; default interval 5 minutes.
    let store = enabled_store(last_mutation, last_export);
    let target = MockBackupTarget::available();
    let work = test_dir();

    let outcome =
        maybe_export(&store, &target, work.path(), utc(2026, 7, 12, 10, 4)).expect("gate");

    assert_eq!(outcome, expected);
    assert_eq!(
        target.with_uploads(<[_]>::len),
        usize::from(expected.is_some())
    );
}

#[test_case("1", Some(ExportOutcome::Uploaded) ; "short_interval_elapsed")]
#[test_case("abc", None ; "nonsense_falls_back_to_default")]
#[test_case("0", None ; "zero_falls_back_to_default")]
#[allow(clippy::needless_pass_by_value)]
fn maybe_export_honours_the_configured_interval(raw: &str, expected: Option<ExportOutcome>) {
    // Dirty at 10:03, exported at 10:00, now 10:02 + 2 minutes elapsed.
    let store = enabled_store(Some(utc(2026, 7, 12, 10, 1)), Some(utc(2026, 7, 12, 10, 0)));
    store
        .set_config_value(KEY_BACKUP_INTERVAL_MINUTES, raw)
        .expect("interval");
    let work = test_dir();

    let outcome = maybe_export(
        &store,
        &MockBackupTarget::available(),
        work.path(),
        utc(2026, 7, 12, 10, 2),
    )
    .expect("gate");
    assert_eq!(outcome, expected);
}

#[test_case(Some(utc(2026, 7, 12, 10, 1)), Some(utc(2026, 7, 12, 10, 0)), Some(ExportOutcome::Uploaded) ; "dirty_exports")]
#[test_case(Some(utc(2026, 7, 12, 9, 0)), Some(utc(2026, 7, 12, 10, 0)), None ; "clean_skips")]
#[allow(clippy::needless_pass_by_value)]
fn export_if_dirty_ignores_the_interval_but_keeps_the_dirty_gate(
    last_mutation: Option<DateTime<Utc>>,
    last_export: Option<DateTime<Utc>>,
    expected: Option<ExportOutcome>,
) {
    let work = test_dir();
    let store = enabled_store(last_mutation, last_export);
    let outcome = export_if_dirty(
        &store,
        &MockBackupTarget::available(),
        work.path(),
        utc(2026, 7, 12, 10, 2),
    )
    .expect("gate");
    assert_eq!(outcome, expected);
}

#[test]
fn start_backup_timer_exports_on_its_poll_cadence() {
    let store: Arc<dyn crate::traits::storage::Store + Send + Sync> =
        Arc::new(enabled_store(Some(utc(2026, 7, 12, 10, 0)), None));
    let target = Arc::new(MockBackupTarget::available());
    let work = test_dir();

    let context = BackupContext {
        store,
        target: target.clone(),
        profile_dir: work.path().to_path_buf(),
    };
    start_backup_timer(move || Some(context.clone()), Duration::from_millis(20));

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while target.with_uploads(<[_]>::is_empty) {
        assert!(
            std::time::Instant::now() < deadline,
            "timer never exported within the deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn backup_timer_tick_skips_when_no_context_resolves() {
    // Must not panic or touch anything — there is nothing to touch.
    super::backup_timer_tick(None, test_now());
}

#[test_case(Some(utc(2026, 7, 12,10, 1)), 1 ; "flushes_a_dirty_database")]
#[test_case(Some(utc(2026, 7, 12,9, 0)), 0 ; "skips_a_clean_database")]
fn export_on_exit_flushes_only_a_dirty_database(
    last_mutation: Option<DateTime<Utc>>,
    expected_uploads: usize,
) {
    let store = Arc::new(enabled_store(last_mutation, Some(utc(2026, 7, 12, 10, 0))));
    let target = Arc::new(MockBackupTarget::available());
    let work = test_dir();

    export_on_exit(
        store,
        target.clone(),
        work.path().to_path_buf(),
        Duration::from_secs(10),
        test_now(),
    );

    assert_eq!(target.with_uploads(<[_]>::len), expected_uploads);
}

#[test]
fn export_on_exit_warns_but_skips_when_the_remote_is_newer() {
    let store = Arc::new(enabled_store(Some(utc(2026, 7, 12, 10, 0)), None));
    let target = Arc::new(
        MockBackupTarget::available().with_meta(Some(remote_meta(Some(utc(2026, 7, 12, 11, 0))))),
    );
    let work = test_dir();

    export_on_exit(
        store.clone(),
        target.clone(),
        work.path().to_path_buf(),
        Duration::from_secs(10),
        test_now(),
    );

    assert!(
        target.with_uploads(<[_]>::is_empty),
        "guard holds on exit too"
    );
    let warning = config_value(&store, KEY_LAST_BACKUP_ERROR).expect("warning recorded");
    assert!(warning.contains("restart to pull"), "got: {warning}");
}

#[test]
fn export_on_exit_gives_up_after_the_timeout() {
    let store = Arc::new(enabled_store(
        Some(utc(2026, 7, 12, 10, 1)),
        Some(utc(2026, 7, 12, 10, 0)),
    ));
    let target = Arc::new(MockBackupTarget::available().with_meta_delay(Duration::from_secs(5)));
    let work = test_dir();

    let started = std::time::Instant::now();
    export_on_exit(
        store,
        target.clone(),
        work.path().to_path_buf(),
        Duration::from_millis(50),
        test_now(),
    );

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "exit must not wait for a stuck export"
    );
    assert!(
        target.with_uploads(<[_]>::is_empty),
        "the stuck export never landed"
    );
}

// Opt-in is a per-database flag: an opted-out flag *or* an empty profile dir
// (no flag at all) both read as "disabled" → skip.
#[test_case(true ; "when_the_profile_never_opted_in")]
#[test_case(false ; "a_missing_local_database_too")]
fn restore_check_skips_a_disabled_database(opted_out_flag: bool) {
    let dir = test_dir();
    if opted_out_flag {
        seed_profile_db(dir.path(), false, Some(utc(2026, 7, 12, 9, 0)));
    }
    let target =
        MockBackupTarget::available().with_meta(Some(remote_meta(Some(utc(2026, 7, 12, 12, 0)))));
    assert_restore_skipped(dir.path(), &target, RestoreSkipReason::BackupDisabled);
}

#[test_case(MockBackupTarget::unavailable, false, RestoreSkipReason::NotConnected ; "no_credentials")]
#[test_case(MockBackupTarget::available, true, RestoreSkipReason::NoRemoteBackup ; "no_remote_backup")]
#[test_case(|| MockBackupTarget::available().with_meta_error("offline"), true, RestoreSkipReason::ProbeFailed ; "probe_fails")]
fn restore_check_skips(
    make_target: fn() -> MockBackupTarget,
    seed: bool,
    reason: RestoreSkipReason,
) {
    let dir = test_dir();
    if seed {
        seed_profile_db(dir.path(), true, Some(utc(2026, 7, 12, 9, 0)));
    }
    let target = make_target();
    assert_restore_skipped(dir.path(), &target, reason);
}

#[test_case(Some(utc(2026, 7, 12,9, 0)) ; "remote_older")]
#[test_case(Some(utc(2026, 7, 12,10, 0)) ; "remote_equal")]
#[test_case(None ; "remote_never_mutated")]
fn restore_check_keeps_a_fresh_local_database(remote_mutation: Option<DateTime<Utc>>) {
    let dir = seeded_dir_10h();
    let target = MockBackupTarget::available().with_meta(Some(remote_meta(remote_mutation)));

    assert_restore_skipped(dir.path(), &target, RestoreSkipReason::LocalFresh);
    assert_eq!(
        archive::read_local_last_mutation(&dir.path().join(DB_ENTRY_NAME)),
        Some(utc(2026, 7, 12, 10, 0)),
        "local database untouched"
    );
}

#[test_case(Some(utc(2026, 7, 12,10, 0)); "local_mutated")]
#[test_case(None; "local_never_mutated")]
fn restore_check_restores_with_varying_local_state(local_mutation: Option<DateTime<Utc>>) {
    let dir = test_dir();
    seed_profile_db(dir.path(), true, local_mutation);
    let target = target_with_newer_backup();
    let outcome = restore_check(dir.path(), &target, test_now());
    assert!(
        matches!(outcome, RestoreOutcome::Restored { .. }),
        "got: {outcome:?}"
    );
    let db = dir.path().join(DB_ENTRY_NAME);
    assert_eq!(
        archive::read_local_last_mutation(&db),
        Some(utc(2026, 7, 12, 12, 0)),
        "backup swapped in"
    );
    if local_mutation.is_some() {
        let safety = dir.path().join(format!("{DB_ENTRY_NAME}.pre-restore"));
        assert_eq!(
            archive::read_local_last_mutation(&safety),
            local_mutation,
            "replaced database kept as the safety copy"
        );
    }
}

#[test_case(|| {
    // A directory squatting on the safety-copy path makes the swap fail after verification.
    let dir = seeded_dir_10h();
    std::fs::create_dir(dir.path().join(format!("{DB_ENTRY_NAME}.pre-restore")))
        .expect("squatting dir");
    let target = target_with_newer_backup();
    (dir, target)
}, "safety copy" ; "swap_fails")]
#[test_case(|| {
    let dir = seeded_dir_10h();
    let target =
        MockBackupTarget::available().with_meta(Some(remote_meta(Some(utc(2026, 7, 12, 12, 0)))));
    (dir, target)
}, "download failed" ; "download_fails")]
#[allow(clippy::needless_pass_by_value)] // fn-pointer args cannot be borrowed; test_case passes by value
fn restore_check_fails_closed_leaving_db_untouched(
    make_pair: fn() -> (tempfile::TempDir, MockBackupTarget),
    msg_fragment: &str,
) {
    let (dir, target) = make_pair();
    let outcome = restore_check(dir.path(), &target, test_now());
    assert!(
        matches!(&outcome, RestoreOutcome::Failed(msg) if msg.contains(msg_fragment)),
        "got: {outcome:?}"
    );
    assert_eq!(
        archive::read_local_last_mutation(&dir.path().join(DB_ENTRY_NAME)),
        Some(utc(2026, 7, 12, 10, 0)),
        "local database untouched"
    );
}

#[test]
fn restore_check_fails_closed_when_verification_fails() {
    let dir = seeded_dir_10h();
    let target = MockBackupTarget::available()
        .with_meta(Some(remote_meta(Some(utc(2026, 7, 12, 12, 0)))))
        .with_download(b"corrupt".to_vec());

    let outcome = restore_check(dir.path(), &target, test_now());

    assert!(
        matches!(outcome, RestoreOutcome::Failed(_)),
        "got: {outcome:?}"
    );
    assert_eq!(
        archive::read_local_last_mutation(&dir.path().join(DB_ENTRY_NAME)),
        Some(utc(2026, 7, 12, 10, 0)),
        "local database untouched"
    );
    assert!(
        !dir.path()
            .join(format!("{DB_ENTRY_NAME}.pre-restore"))
            .exists(),
        "no swap happened"
    );
}

#[test]
fn export_to_file_writes_a_verifiable_archive() {
    let store = test_store();
    set_last_mutation(&store, Some(utc(2026, 7, 12, 10, 0)));
    let dir = test_dir();
    let dest = dir.path().join("my-backup.zip");

    export_to_file(&store, &dest, dir.path()).expect("export");

    let bytes = std::fs::read(&dest).expect("read");
    archive::extract_and_verify(&bytes, dir.path(), migrations::current_version())
        .expect("written file must verify");
}

#[test]
fn stage_import_verifies_stamps_and_stages_the_archive() {
    let (dir, zip_path) = dir_with_import_zip();

    stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 14, 0)).expect("stage");

    let pending = dir.path().join(PENDING_IMPORT);
    assert_eq!(
        archive::read_local_last_mutation(&pending),
        Some(utc(2026, 7, 12, 14, 0)),
        "staged database is stamped as the newest state"
    );
}

#[test]
fn stage_import_rejects_a_bad_archive_without_staging() {
    let dir = test_dir();
    let zip_path = dir.path().join("import.zip");
    std::fs::write(&zip_path, b"corrupt").expect("write");

    stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 14, 0)).expect_err("must reject");
    assert!(!dir.path().join(PENDING_IMPORT).exists(), "nothing staged");
}

#[test]
fn stage_import_fails_when_a_directory_squats_on_the_staging_path() {
    let (dir, zip_path) = dir_with_import_zip();
    std::fs::create_dir(dir.path().join(PENDING_IMPORT)).expect("squatting dir");

    let err = stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 14, 0)).expect_err("must fail");
    assert!(
        err.to_string().contains("previously staged import"),
        "got: {err}"
    );
}

#[test]
fn stage_import_rejects_a_database_it_cannot_stamp() {
    // A database that passes verification (integrity + schema_version) but
    // has no config table: the freshness stamp must fail closed, unstaged.
    let dir = test_dir();
    let raw = dir.path().join("stampless.db");
    let conn = rusqlite::Connection::open(&raw).expect("open");
    conn.execute("CREATE TABLE schema_version (version INTEGER NOT NULL)", [])
        .expect("create");
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        rusqlite::params![migrations::current_version()],
    )
    .expect("insert");
    drop(conn);
    let zip_path = dir.path().join("import.zip");
    std::fs::write(
        &zip_path,
        zip_with_entries(&[(DB_ENTRY_NAME, &std::fs::read(&raw).expect("read"))]),
    )
    .expect("write");

    let err = stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 14, 0)).expect_err("must fail");
    assert!(err.to_string().contains("could not stamp"), "got: {err}");
    assert!(!dir.path().join(PENDING_IMPORT).exists(), "nothing staged");
}

#[test]
fn stage_import_replaces_a_previously_staged_import() {
    let (dir, zip_path) = dir_with_import_zip();

    stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 14, 0)).expect("first stage");
    stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 15, 0)).expect("second stage");

    assert_eq!(
        archive::read_local_last_mutation(&dir.path().join(PENDING_IMPORT)),
        Some(utc(2026, 7, 12, 15, 0)),
        "second staging wins"
    );
}

#[test]
fn apply_pending_import_swaps_once_and_reports_honestly() {
    let dir = test_dir();
    seed_profile_db(dir.path(), false, Some(utc(2026, 7, 12, 10, 0)));
    let zip_path = write_import_zip(dir.path(), utc(2026, 7, 12, 9, 0));
    stage_import(&zip_path, dir.path(), utc(2026, 7, 12, 14, 0)).expect("stage");

    assert!(
        apply_pending_import(dir.path()).expect("apply"),
        "swap happened"
    );
    let db = dir.path().join(DB_ENTRY_NAME);
    assert_eq!(
        archive::read_local_last_mutation(&db),
        Some(utc(2026, 7, 12, 14, 0)),
        "import applied"
    );
    assert_eq!(
        archive::read_local_last_mutation(&dir.path().join(format!("{DB_ENTRY_NAME}.pre-restore"))),
        Some(utc(2026, 7, 12, 10, 0)),
        "replaced database kept"
    );

    assert!(
        !apply_pending_import(dir.path()).expect("second apply"),
        "nothing left to apply"
    );
}
