// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Backup archive plumbing: consistent snapshots, the single-entry zip
//! format, verify-before-swap, and the pre-open database peeks used by the
//! restore check.
//!
//! The archive format is deliberately rigid (Decision 2): exactly one entry
//! named `apreswork.db`. Nothing else from the profile directory is ever
//! read into an archive — in particular `google_auth.json` (M11.6).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::error::AppError;
use crate::traits::storage::Store;

/// The only entry a backup archive may contain.
pub const DB_ENTRY_NAME: &str = "apreswork.db";

/// Scratch name for the `VACUUM INTO` snapshot during export.
pub(crate) const SNAPSHOT_TMP: &str = "apreswork.db.export-snapshot";

/// Scratch name for the extracted database while it is being verified.
pub(crate) const RESTORE_TMP: &str = "apreswork.db.restore-verify";

/// A verified database staged by a manual import, applied (swapped in) at
/// the next profile activation before the store opens.
pub const PENDING_IMPORT: &str = "apreswork.db.pending-import";

/// Snapshot the live database and wrap it in a single-entry zip.
///
/// The snapshot is taken with `VACUUM INTO` (consistent under WAL), zipped
/// as [`DB_ENTRY_NAME`], and deleted again; `work_dir` must be writable and
/// on the same filesystem is not required. Only the snapshot file ever
/// enters the archive (M11.6).
///
/// # Errors
///
/// Returns [`AppError::Database`] when the snapshot fails and
/// [`AppError::Backup`] when the archive cannot be built.
pub fn build_backup_zip(store: &dyn Store, work_dir: &Path) -> Result<Vec<u8>, AppError> {
    let snapshot = work_dir.join(SNAPSHOT_TMP);
    if snapshot.exists() {
        fs::remove_file(&snapshot)
            .map_err(|e| AppError::Backup(format!("could not clear stale snapshot: {e}")))?;
    }
    store.vacuum_into(&snapshot)?;
    let result = zip_single_file(&snapshot);
    // Best-effort cleanup on both paths; the next export clears leftovers.
    let _ = fs::remove_file(&snapshot);
    result
}

/// Zip one file as the archive's sole [`DB_ENTRY_NAME`] entry.
fn zip_single_file(path: &Path) -> Result<Vec<u8>, AppError> {
    let bytes =
        fs::read(path).map_err(|e| AppError::Backup(format!("could not read snapshot: {e}")))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file(DB_ENTRY_NAME, options)
        .map_err(|e| AppError::Backup(format!("could not build backup archive: {e}")))?;
    writer
        .write_all(&bytes)
        .map_err(|e| AppError::Backup(format!("could not build backup archive: {e}")))?;
    let cursor = writer
        .finish()
        .map_err(|e| AppError::Backup(format!("could not finalize backup archive: {e}")))?;
    Ok(cursor.into_inner())
}

/// Extract a backup archive and verify the database inside it.
///
/// Verification (Decision 4 safety valve 1): the archive must contain
/// exactly one entry named [`DB_ENTRY_NAME`]; the extracted database must
/// pass `PRAGMA integrity_check` and carry a readable `schema_version` no
/// newer than `max_schema_version`. On success the verified file's path is
/// returned; on any failure the scratch file is removed and the local
/// database is untouched.
///
/// # Errors
///
/// Returns [`AppError::Backup`] describing the first check that failed.
pub fn extract_and_verify(
    zip_bytes: &[u8],
    work_dir: &Path,
    max_schema_version: i64,
) -> Result<PathBuf, AppError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| AppError::Backup(format!("not a valid backup archive: {e}")))?;
    if archive.len() != 1 {
        return Err(AppError::Backup(format!(
            "backup archive must contain exactly one entry, found {}",
            archive.len()
        )));
    }
    let mut entry = archive
        .by_index(0)
        .map_err(|e| AppError::Backup(format!("could not read backup archive entry: {e}")))?;
    if entry.name() != DB_ENTRY_NAME {
        return Err(AppError::Backup(format!(
            "unexpected backup archive entry '{}' (expected '{DB_ENTRY_NAME}')",
            entry.name()
        )));
    }

    let out = work_dir.join(RESTORE_TMP);
    if out.exists() {
        fs::remove_file(&out)
            .map_err(|e| AppError::Backup(format!("could not clear stale restore file: {e}")))?;
    }
    let mut file = fs::File::create(&out)
        .map_err(|e| AppError::Backup(format!("could not write restore file: {e}")))?;
    std::io::copy(&mut entry, &mut file)
        .map_err(|e| AppError::Backup(format!("could not extract backup archive: {e}")))?;
    drop(file);

    if let Err(e) = verify_restored_db(&out, max_schema_version) {
        let _ = fs::remove_file(&out);
        return Err(e);
    }
    Ok(out)
}

/// Integrity + schema gate on an extracted database file.
fn verify_restored_db(path: &Path, max_schema_version: i64) -> Result<(), AppError> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| AppError::Backup(format!("backup database cannot be opened: {e}")))?;
    let check: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|e| AppError::Backup(format!("backup database integrity check failed: {e}")))?;
    if check != "ok" {
        return Err(AppError::Backup(
            "backup database failed its integrity check".into(),
        ));
    }
    let version: i64 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .map_err(|_| AppError::Backup("backup database has no readable schema version".into()))?;
    if version > max_schema_version {
        return Err(AppError::Backup(format!(
            "backup was written by a newer app (schema {version} > {max_schema_version}) — \
             update the app before restoring"
        )));
    }
    Ok(())
}

/// Swap a verified database into place, keeping the replaced one as the
/// single `*.pre-restore` safety copy (Decision 4 safety valve 2).
///
/// The current `apreswork.db` and its WAL sidecars are renamed to
/// `apreswork.db.pre-restore{,-wal,-shm}` (overwriting the previous safety
/// copy; stale sidecar copies are removed so the safety copy is never a mix
/// of two generations), then `verified_db` is renamed in. `verified_db`
/// must live on the same filesystem — callers stage it in the profile dir.
///
/// On failure the already-moved originals are renamed back (best effort),
/// so the profile keeps a database at the expected path; even if a rollback
/// rename fails, the safety copy still holds the complete pre-swap data.
///
/// # Errors
///
/// Returns [`AppError::Backup`] on any filesystem failure.
pub fn swap_in_database(profile_dir: &Path, verified_db: &Path) -> Result<(), AppError> {
    let mut moved_aside: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = move_aside_and_swap(profile_dir, verified_db, &mut moved_aside);
    if result.is_err() {
        for (safety, current) in moved_aside.iter().rev() {
            let _ = fs::rename(safety, current);
        }
    }
    result
}

/// Fallible body of [`swap_in_database`]; records every current→safety
/// rename in `moved_aside` so the caller can roll back on failure.
fn move_aside_and_swap(
    profile_dir: &Path,
    verified_db: &Path,
    moved_aside: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), AppError> {
    for suffix in ["", "-wal", "-shm"] {
        let current = profile_dir.join(format!("{DB_ENTRY_NAME}{suffix}"));
        let safety = profile_dir.join(format!("{DB_ENTRY_NAME}.pre-restore{suffix}"));
        if safety.exists() {
            fs::remove_file(&safety).map_err(|e| {
                AppError::Backup(format!("could not clear previous safety copy: {e}"))
            })?;
        }
        if current.exists() {
            fs::rename(&current, &safety).map_err(|e| {
                AppError::Backup(format!("could not move current database aside: {e}"))
            })?;
            moved_aside.push((safety, current));
        }
    }
    fs::rename(verified_db, profile_dir.join(DB_ENTRY_NAME))
        .map_err(|e| AppError::Backup(format!("could not move restored database in place: {e}")))
}

/// Read a single config value from a profile database that is not open yet.
///
/// Used by the restore check before the store exists. The connection is
/// opened read-write (WAL recovery after an unclean shutdown may need write
/// access) but only issues a `SELECT`. Any failure — missing file, missing
/// table, missing key — reads as `None`: the caller treats the local side
/// as having no comparable state.
#[must_use]
pub fn read_local_config_value(db_path: &Path, key: &str) -> Option<String> {
    if !db_path.exists() {
        return None;
    }
    let conn = Connection::open(db_path).ok()?;
    conn.query_row(
        "SELECT value FROM config WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Read the local database's `last_mutation` without opening the store.
///
/// Empty or unparsable values read as `None` (mirrors the config store's
/// empty-string-is-`None` convention).
#[must_use]
pub fn read_local_last_mutation(db_path: &Path) -> Option<DateTime<Utc>> {
    let raw = read_local_config_value(db_path, "last_mutation")?;
    DateTime::parse_from_rfc3339(&raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
