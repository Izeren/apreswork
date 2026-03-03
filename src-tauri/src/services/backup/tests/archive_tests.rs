// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Archive plumbing tests: build/verify round-trip, malformed-archive
//! rejection, swap-in with safety copy + rollback, and the pre-open peeks.
//! Shared fixtures (mock target, zip builders, seeded profile dirs) live in
//! the parent test module.

use tempfile::tempdir;
use test_case::test_case;

use super::{seed_profile_db, set_last_mutation, valid_backup_zip, zip_with_entries};
use crate::db::migrations;
use crate::services::backup::archive::{self, DB_ENTRY_NAME};
use crate::services::backup::KEY_BACKUP_ENABLED;
use crate::test_support::{test_store, utc};

/// Stage `incoming.db` = "new" in `dir`, swap it in, and return the staged
/// path (the swap moves it, so callers can assert it no longer exists).
fn stage_and_swap(dir: &std::path::Path) -> std::path::PathBuf {
    let verified = dir.join("incoming.db");
    std::fs::write(&verified, "new").expect("stage");
    archive::swap_in_database(dir, &verified).expect("swap");
    verified
}

#[test]
fn backup_zip_round_trips_through_verify() {
    let store = test_store();
    set_last_mutation(&store, Some(utc(2026, 7, 12, 10, 0)));
    let work = tempdir().expect("workdir");

    let zip_bytes = archive::build_backup_zip(&store, work.path()).expect("build");
    let verified =
        archive::extract_and_verify(&zip_bytes, work.path(), migrations::current_version())
            .expect("verify");

    assert_eq!(
        archive::read_local_last_mutation(&verified),
        Some(utc(2026, 7, 12, 10, 0)),
        "snapshot carries the live database state"
    );
}

#[test]
fn backup_zip_contains_only_the_database_never_the_token_file() {
    // M11.6: a token file sitting in the work dir must never enter the zip.
    let store = test_store();
    let work = tempdir().expect("workdir");
    std::fs::write(work.path().join("google_auth.json"), b"{\"secret\":1}").expect("token file");

    let zip_bytes = archive::build_backup_zip(&store, work.path()).expect("build");

    let reader = std::io::Cursor::new(zip_bytes);
    let zip = zip::ZipArchive::new(reader).expect("open zip");
    assert_eq!(zip.len(), 1, "exactly one entry");
    assert_eq!(zip.file_names().next(), Some(DB_ENTRY_NAME));
}

#[test]
fn verify_rejects_garbage_bytes() {
    let work = tempdir().expect("workdir");
    let err = archive::extract_and_verify(b"not a zip", work.path(), 99).expect_err("must reject");
    assert!(
        err.to_string().contains("not a valid backup archive"),
        "got: {err}"
    );
}

#[test_case(&[("evil.db", b"x" as &[u8])], "unexpected backup archive entry" ; "wrong_entry_name")]
#[test_case(&[(DB_ENTRY_NAME, b"x" as &[u8]), ("extra.txt", b"y")], "exactly one entry" ; "two_entries")]
fn verify_rejects_malformed_archives(entries: &[(&str, &[u8])], expected: &str) {
    let work = tempdir().expect("workdir");
    let err = archive::extract_and_verify(&zip_with_entries(entries), work.path(), 99)
        .expect_err("must reject");
    assert!(err.to_string().contains(expected), "got: {err}");
}

#[test]
fn verify_rejects_a_non_database_entry_and_cleans_up() {
    let work = tempdir().expect("workdir");
    let zip_bytes = zip_with_entries(&[(DB_ENTRY_NAME, b"this is not sqlite")]);
    archive::extract_and_verify(&zip_bytes, work.path(), 99).expect_err("must reject");
    let leftovers = std::fs::read_dir(work.path()).expect("dir").count();
    assert_eq!(leftovers, 0, "scratch file must be removed on failure");
}

#[test]
fn verify_rejects_a_database_without_schema_version() {
    let work = tempdir().expect("workdir");
    let raw = work.path().join("bare.db");
    let conn = rusqlite::Connection::open(&raw).expect("open");
    conn.execute("CREATE TABLE misc (id INTEGER)", [])
        .expect("create");
    drop(conn);

    let zip_bytes = zip_with_entries(&[(DB_ENTRY_NAME, &std::fs::read(&raw).expect("read"))]);
    let err = archive::extract_and_verify(&zip_bytes, work.path(), 99).expect_err("must reject");
    assert!(
        err.to_string().contains("no readable schema version"),
        "got: {err}"
    );
}

#[test]
fn build_backup_zip_clears_a_stale_snapshot_scratch_file() {
    // A crash between snapshot and cleanup leaves the scratch file behind;
    // the next export must clear it rather than fail.
    let store = test_store();
    let work = tempdir().expect("workdir");
    std::fs::write(work.path().join(archive::SNAPSHOT_TMP), b"crashed export").expect("stale");

    let zip_bytes = archive::build_backup_zip(&store, work.path()).expect("build");
    archive::extract_and_verify(&zip_bytes, work.path(), migrations::current_version())
        .expect("archive built over the stale scratch must verify");
}

#[test]
fn extract_and_verify_clears_a_stale_restore_scratch_file() {
    let work = tempdir().expect("workdir");
    std::fs::write(work.path().join(archive::RESTORE_TMP), b"crashed restore").expect("stale");

    archive::extract_and_verify(
        &valid_backup_zip(None),
        work.path(),
        migrations::current_version(),
    )
    .expect("stale scratch must not block a restore");
}

#[test]
fn verify_rejects_a_backup_from_a_newer_app() {
    let zip_bytes = valid_backup_zip(None);
    let work = tempdir().expect("workdir");
    let err =
        archive::extract_and_verify(&zip_bytes, work.path(), migrations::current_version() - 1)
            .expect_err("must reject");
    assert!(err.to_string().contains("newer app"), "got: {err}");
}

#[test]
fn swap_keeps_the_replaced_database_as_the_safety_copy() {
    let dir = tempdir().expect("dir");
    for (name, content) in [
        (DB_ENTRY_NAME.to_owned(), "old"),
        (format!("{DB_ENTRY_NAME}-wal"), "old-wal"),
        (format!("{DB_ENTRY_NAME}-shm"), "old-shm"),
    ] {
        std::fs::write(dir.path().join(name), content).expect("seed");
    }
    let verified = stage_and_swap(dir.path());

    let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).expect("read");
    assert_eq!(read(DB_ENTRY_NAME), "new");
    assert_eq!(read(&format!("{DB_ENTRY_NAME}.pre-restore")), "old");
    assert_eq!(read(&format!("{DB_ENTRY_NAME}.pre-restore-wal")), "old-wal");
    assert!(!dir.path().join(format!("{DB_ENTRY_NAME}-wal")).exists());
    assert!(!verified.exists(), "staged file was moved, not copied");
}

#[test]
fn swap_never_mixes_two_safety_copy_generations() {
    let dir = tempdir().expect("dir");
    // Current DB has no WAL sidecars, but a previous restore left safety
    // copies including a -wal. That stale -wal must go.
    std::fs::write(dir.path().join(DB_ENTRY_NAME), "old2").expect("seed");
    std::fs::write(
        dir.path().join(format!("{DB_ENTRY_NAME}.pre-restore")),
        "ancient",
    )
    .expect("seed");
    std::fs::write(
        dir.path().join(format!("{DB_ENTRY_NAME}.pre-restore-wal")),
        "ancient-wal",
    )
    .expect("seed");
    stage_and_swap(dir.path());

    let safety = std::fs::read_to_string(dir.path().join(format!("{DB_ENTRY_NAME}.pre-restore")))
        .expect("read");
    assert_eq!(safety, "old2");
    assert!(
        !dir.path()
            .join(format!("{DB_ENTRY_NAME}.pre-restore-wal"))
            .exists(),
        "stale sidecar safety copy must be removed"
    );
}

#[test]
fn swap_rolls_the_originals_back_when_the_final_rename_fails() {
    let dir = tempdir().expect("dir");
    std::fs::write(dir.path().join(DB_ENTRY_NAME), "old").expect("seed");
    std::fs::write(dir.path().join(format!("{DB_ENTRY_NAME}-wal")), "old-wal").expect("seed");
    // The staged file is missing, so the final rename fails after the
    // current database was already moved aside.
    let missing = dir.path().join("incoming.db");

    let err = archive::swap_in_database(dir.path(), &missing).expect_err("must fail");

    assert!(err.to_string().contains("restored database"), "got: {err}");
    let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).expect("read");
    assert_eq!(read(DB_ENTRY_NAME), "old", "rollback restores the main db");
    assert_eq!(read(&format!("{DB_ENTRY_NAME}-wal")), "old-wal");
}

#[test]
fn read_local_config_value_reads_none_for_every_failure_mode() {
    let dir = tempdir().expect("dir");
    let absent = dir.path().join("missing.db");
    assert_eq!(
        archive::read_local_config_value(&absent, "k"),
        None,
        "absent db"
    );

    let no_table = dir.path().join("bare.db");
    let conn = rusqlite::Connection::open(&no_table).expect("open");
    conn.execute("CREATE TABLE misc (id INTEGER)", [])
        .expect("create");
    drop(conn);
    assert_eq!(
        archive::read_local_config_value(&no_table, "k"),
        None,
        "no config table"
    );

    seed_profile_db(dir.path(), true, None);
    let db = dir.path().join(DB_ENTRY_NAME);
    assert_eq!(
        archive::read_local_config_value(&db, KEY_BACKUP_ENABLED).as_deref(),
        Some("true"),
        "present key"
    );
    assert_eq!(
        archive::read_local_config_value(&db, "ghost-key"),
        None,
        "missing key"
    );
}

#[test]
fn read_local_last_mutation_parses_and_tolerates_garbage() {
    let dir = tempdir().expect("dir");
    seed_profile_db(dir.path(), false, Some(utc(2026, 7, 12, 9, 30)));
    let db = dir.path().join(DB_ENTRY_NAME);
    assert_eq!(
        archive::read_local_last_mutation(&db),
        Some(utc(2026, 7, 12, 9, 30))
    );

    let conn = rusqlite::Connection::open(&db).expect("open");
    conn.execute(
        "UPDATE config SET value = 'not-a-date' WHERE key = 'last_mutation'",
        [],
    )
    .expect("corrupt");
    drop(conn);
    assert_eq!(
        archive::read_local_last_mutation(&db),
        None,
        "garbage reads as None"
    );
}
