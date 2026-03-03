// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Backup orchestration (plans/drive-backup.md): dirty-checked exports with a
//! stale-writer guard, the backup-wins restore check that runs before the
//! store opens, manual file export/import, and the interval/exit triggers.
//!
//! All decision logic goes through [`BackupTarget`], so everything here is
//! testable with a mock and no test ever talks to a live provider.

pub mod archive;

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::AppError;
use crate::traits::backup::{BackupTarget, RemoteBackupMeta};
use crate::traits::storage::Store;

/// `"true"` once the user opts this profile into automatic backup.
pub const KEY_BACKUP_ENABLED: &str = "backup_enabled";
/// Minutes between interval exports (default [`DEFAULT_INTERVAL_MINUTES`]).
pub const KEY_BACKUP_INTERVAL_MINUTES: &str = "backup_interval_minutes";
/// RFC 3339 instant of the last successful export.
pub const KEY_LAST_EXPORT_AT: &str = "last_export_at";
/// Last export/restore problem for the Settings card; empty = none.
pub const KEY_LAST_BACKUP_ERROR: &str = "last_backup_error";

/// Default interval between automatic exports.
pub const DEFAULT_INTERVAL_MINUTES: i64 = 5;

/// How often the background thread re-evaluates the export gates.
pub const BACKUP_TIMER_POLL: Duration = Duration::from_secs(60);

/// Stale-writer guard message (Decision 5) — also shown by the Settings card.
const STALE_WRITER_WARNING: &str = "Backup on Drive is newer — restart to pull it.";

/// Backup bookkeeping for the Settings card and REST status endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    /// Whether this profile opted into automatic backup.
    pub enabled: bool,
    /// Whether the backup target has credentials (can enable / back up now).
    pub connected: bool,
    /// Instant of the last successful export, if any.
    pub last_export_at: Option<DateTime<Utc>>,
    /// Last export/restore problem; `None` when the last run succeeded.
    pub last_backup_error: Option<String>,
    /// Set when this app run restored the database from a backup — the
    /// backup's `last_mutation` as RFC 3339 (UI toast, Decision 4).
    pub restored_this_run: Option<String>,
}

fn parse_rfc3339_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Read backup bookkeeping. Lenient like sync status: unparseable timestamps
/// read as `None`, blank error strings read as `None`.
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn get_backup_status(
    store: &dyn Store,
    target: &dyn BackupTarget,
    restore_notice: Option<&str>,
) -> Result<BackupStatus, AppError> {
    let enabled = store.get_config_value(KEY_BACKUP_ENABLED)?.as_deref() == Some("true");
    let last_export_at = store
        .get_config_value(KEY_LAST_EXPORT_AT)?
        .as_deref()
        .and_then(parse_rfc3339_utc);
    let last_backup_error = store
        .get_config_value(KEY_LAST_BACKUP_ERROR)?
        .filter(|s| !s.trim().is_empty());
    Ok(BackupStatus {
        enabled,
        connected: target.is_available(),
        last_export_at,
        last_backup_error,
        restored_this_run: restore_notice.map(str::to_owned),
    })
}

/// Opt the profile in or out of automatic backup.
///
/// # Errors
///
/// Returns [`AppError::Validation`] when enabling without a connected
/// target, [`AppError::Database`] on storage failure.
pub fn set_backup_enabled(
    store: &dyn Store,
    target: &dyn BackupTarget,
    enabled: bool,
) -> Result<(), AppError> {
    if enabled && !target.is_available() {
        return Err(AppError::Validation(
            "Connect Google before enabling backup.".into(),
        ));
    }
    store.set_config_value(KEY_BACKUP_ENABLED, if enabled { "true" } else { "false" })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportOutcome {
    /// A fresh archive was uploaded.
    Uploaded,
    /// The stale-writer guard fired: the remote backup is newer than this
    /// database, so uploading would clobber fresher data (Decision 5).
    SkippedStaleRemote,
}

fn current_meta(store: &dyn Store) -> Result<RemoteBackupMeta, AppError> {
    Ok(RemoteBackupMeta {
        last_mutation: store.get_config()?.last_mutation,
        schema_version: crate::db::migrations::current_version(),
    })
}

/// Export now (manual "Back up now" and the gated triggers' final step).
///
/// Applies the stale-writer guard, snapshots + uploads, and records the
/// bookkeeping: success stamps [`KEY_LAST_EXPORT_AT`] and clears the error;
/// a guard skip or failure records [`KEY_LAST_BACKUP_ERROR`].
///
/// # Errors
///
/// Returns [`AppError::Validation`] when the target has no credentials, and
/// propagates snapshot/upload errors (after recording them).
pub fn export_now(
    store: &dyn Store,
    target: &dyn BackupTarget,
    work_dir: &Path,
    now: DateTime<Utc>,
) -> Result<ExportOutcome, AppError> {
    if !target.is_available() {
        return Err(AppError::Validation(
            "Backup is not available — connect Google first.".into(),
        ));
    }
    match export_inner(store, target, work_dir, now) {
        Ok(ExportOutcome::Uploaded) => {
            store.set_config_value(KEY_LAST_EXPORT_AT, &now.to_rfc3339())?;
            store.set_config_value(KEY_LAST_BACKUP_ERROR, "")?;
            Ok(ExportOutcome::Uploaded)
        }
        Ok(ExportOutcome::SkippedStaleRemote) => {
            store.set_config_value(KEY_LAST_BACKUP_ERROR, STALE_WRITER_WARNING)?;
            Ok(ExportOutcome::SkippedStaleRemote)
        }
        Err(e) => {
            // Best-effort bookkeeping; the original error is what matters.
            let _ = store.set_config_value(KEY_LAST_BACKUP_ERROR, &e.to_string());
            Err(e)
        }
    }
}

/// Guard + snapshot + upload, without bookkeeping.
fn export_inner(
    store: &dyn Store,
    target: &dyn BackupTarget,
    work_dir: &Path,
    now: DateTime<Utc>,
) -> Result<ExportOutcome, AppError> {
    let local = current_meta(store)?;
    if let Some(remote) = target.get_meta(now)? {
        // Option ordering (None < Some) matches "no mutations yet is oldest".
        if remote.last_mutation > local.last_mutation {
            return Ok(ExportOutcome::SkippedStaleRemote);
        }
    }
    let zip_bytes = archive::build_backup_zip(store, work_dir)?;
    target.upload(now, &zip_bytes, &local)?;
    Ok(ExportOutcome::Uploaded)
}

/// Whether the database changed since the last export.
fn is_dirty(last_mutation: Option<DateTime<Utc>>, last_export: Option<DateTime<Utc>>) -> bool {
    match (last_mutation, last_export) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(m), Some(e)) => m > e,
    }
}

/// The configured export interval, defaulting (and clamping nonsense) to
/// [`DEFAULT_INTERVAL_MINUTES`].
fn interval_minutes(store: &dyn Store) -> Result<i64, AppError> {
    Ok(store
        .get_config_value(KEY_BACKUP_INTERVAL_MINUTES)?
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|m| *m >= 1)
        .unwrap_or(DEFAULT_INTERVAL_MINUTES))
}

fn gated_export(
    store: &dyn Store,
    target: &dyn BackupTarget,
    work_dir: &Path,
    now: DateTime<Utc>,
    enforce_interval: bool,
) -> Result<Option<ExportOutcome>, AppError> {
    if store.get_config_value(KEY_BACKUP_ENABLED)?.as_deref() != Some("true") {
        return Ok(None);
    }
    if !target.is_available() {
        return Ok(None);
    }
    let last_export = store
        .get_config_value(KEY_LAST_EXPORT_AT)?
        .as_deref()
        .and_then(parse_rfc3339_utc);
    if enforce_interval {
        if let Some(last) = last_export {
            if now - last < chrono::Duration::minutes(interval_minutes(store)?) {
                return Ok(None);
            }
        }
    }
    if !is_dirty(store.get_config()?.last_mutation, last_export) {
        return Ok(None);
    }
    export_now(store, target, work_dir, now).map(Some)
}

/// Interval-timer trigger: export only when enabled, connected, the interval
/// elapsed, and the database changed since the last export (Decision 5).
///
/// # Errors
///
/// Propagates [`export_now`] errors (already recorded as bookkeeping).
pub fn maybe_export(
    store: &dyn Store,
    target: &dyn BackupTarget,
    work_dir: &Path,
    now: DateTime<Utc>,
) -> Result<Option<ExportOutcome>, AppError> {
    gated_export(store, target, work_dir, now, true)
}

/// Graceful-exit trigger: same gates as [`maybe_export`] minus the interval —
/// close-A-open-B must not wait for the next tick (Decision 5).
///
/// # Errors
///
/// Propagates [`export_now`] errors (already recorded as bookkeeping).
pub fn export_if_dirty(
    store: &dyn Store,
    target: &dyn BackupTarget,
    work_dir: &Path,
    now: DateTime<Utc>,
) -> Result<Option<ExportOutcome>, AppError> {
    gated_export(store, target, work_dir, now, false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreSkipReason {
    /// No credentials on this device — backup can't be reached.
    NotConnected,
    /// The local database has not opted into backup.
    BackupDisabled,
    /// The target holds no backup yet.
    NoRemoteBackup,
    /// The local database is at least as new as the backup.
    LocalFresh,
    /// The freshness probe failed (offline/timeout) — offline-first skip.
    ProbeFailed,
    /// A staged manual import was applied instead this activation.
    ImportApplied,
}

/// Outcome of the pre-open restore check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The backup replaced the local database (safety copy kept).
    Restored {
        /// The backup's embedded `last_mutation` (for the UI notice).
        backup_last_mutation: Option<DateTime<Utc>>,
    },
    /// Nothing was restored; startup proceeds on the local database.
    Skipped(RestoreSkipReason),
    /// A restore step failed after the decision to restore; the local
    /// database is untouched and the message goes to the Settings card.
    Failed(String),
}

/// Backup-wins restore check, run during profile activation BEFORE the store
/// opens. Never returns `Err`: every failure keeps the local database and
/// startup continues (Decision 4).
#[must_use]
pub fn restore_check(
    profile_dir: &Path,
    target: &dyn BackupTarget,
    now: DateTime<Utc>,
) -> RestoreOutcome {
    if !target.is_available() {
        return RestoreOutcome::Skipped(RestoreSkipReason::NotConnected);
    }
    let db_path = profile_dir.join(archive::DB_ENTRY_NAME);
    if archive::read_local_config_value(&db_path, KEY_BACKUP_ENABLED).as_deref() != Some("true") {
        return RestoreOutcome::Skipped(RestoreSkipReason::BackupDisabled);
    }
    let remote = match target.get_meta(now) {
        Ok(Some(meta)) => meta,
        Ok(None) => return RestoreOutcome::Skipped(RestoreSkipReason::NoRemoteBackup),
        Err(e) => {
            log::info!("backup: restore check skipped (probe failed): {e}");
            return RestoreOutcome::Skipped(RestoreSkipReason::ProbeFailed);
        }
    };
    let local_last_mutation = archive::read_local_last_mutation(&db_path);
    if remote.last_mutation <= local_last_mutation {
        return RestoreOutcome::Skipped(RestoreSkipReason::LocalFresh);
    }

    let zip_bytes = match target.download(now) {
        Ok(bytes) => bytes,
        Err(e) => return RestoreOutcome::Failed(format!("backup download failed: {e}")),
    };
    let verified = match archive::extract_and_verify(
        &zip_bytes,
        profile_dir,
        crate::db::migrations::current_version(),
    ) {
        Ok(path) => path,
        Err(e) => return RestoreOutcome::Failed(e.to_string()),
    };
    if let Err(e) = archive::swap_in_database(profile_dir, &verified) {
        let _ = fs::remove_file(&verified);
        return RestoreOutcome::Failed(e.to_string());
    }
    RestoreOutcome::Restored {
        backup_last_mutation: remote.last_mutation,
    }
}

/// # Errors
///
/// Returns [`AppError::Backup`] on snapshot/archive/write failure.
pub fn export_to_file(store: &dyn Store, dest: &Path, work_dir: &Path) -> Result<(), AppError> {
    let zip_bytes = archive::build_backup_zip(store, work_dir)?;
    fs::write(dest, zip_bytes)
        .map_err(|e| AppError::Backup(format!("could not write backup file: {e}")))
}

/// Verify an import archive and stage it for the next activation.
///
/// The manual flavor of restore: freshness is ignored (explicit user intent
/// wins). The staged database gets `last_mutation = now` so it becomes the
/// newest state — otherwise the stale-writer guard would wedge uploads
/// behind a Drive backup that the user just chose to abandon. The swap
/// itself happens in [`apply_pending_import`] before the store opens; the
/// caller restarts the app after staging.
///
/// # Errors
///
/// Returns [`AppError::Backup`] when the archive fails verification or
/// staging fails; the live database is untouched either way.
pub fn stage_import(
    zip_path: &Path,
    profile_dir: &Path,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let bytes = fs::read(zip_path)
        .map_err(|e| AppError::Backup(format!("could not read import file: {e}")))?;
    let verified = archive::extract_and_verify(
        &bytes,
        profile_dir,
        crate::db::migrations::current_version(),
    )?;
    if let Err(e) = stamp_last_mutation(&verified, now) {
        let _ = fs::remove_file(&verified);
        return Err(e);
    }
    let pending = profile_dir.join(archive::PENDING_IMPORT);
    if pending.exists() {
        if let Err(e) = fs::remove_file(&pending) {
            let _ = fs::remove_file(&verified);
            return Err(AppError::Backup(format!(
                "could not clear a previously staged import: {e}"
            )));
        }
    }
    fs::rename(&verified, &pending)
        .map_err(|e| AppError::Backup(format!("could not stage import: {e}")))
}

/// Mark a staged database as the newest state (see [`stage_import`]).
fn stamp_last_mutation(db_path: &Path, now: DateTime<Utc>) -> Result<(), AppError> {
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| AppError::Backup(format!("could not open staged import: {e}")))?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('last_mutation', ?1)",
        rusqlite::params![now.to_rfc3339()],
    )
    .map_err(|e| AppError::Backup(format!("could not stamp staged import: {e}")))?;
    Ok(())
}

/// Apply a staged manual import, if one exists. Runs during profile
/// activation before the store opens; returns whether a swap happened.
///
/// # Errors
///
/// Returns [`AppError::Backup`] when the swap fails (the staged file is
/// left in place for the next attempt).
pub fn apply_pending_import(profile_dir: &Path) -> Result<bool, AppError> {
    let pending = profile_dir.join(archive::PENDING_IMPORT);
    if !pending.exists() {
        return Ok(false);
    }
    archive::swap_in_database(profile_dir, &pending)?;
    Ok(true)
}

/// Everything the interval-export timer needs from the active profile.
///
/// The timer thread is process-scoped and re-resolves this per tick, so an
/// in-process profile switch redirects subsequent exports to the new
/// profile's store and target without restarting the thread.
#[derive(Clone)]
pub struct BackupContext {
    /// The active profile's store.
    pub store: Arc<dyn Store + Send + Sync>,
    /// The active profile's backup target.
    pub target: Arc<dyn BackupTarget>,
    /// The active profile's data directory (zip staging).
    pub profile_dir: PathBuf,
}

/// One timer tick against the resolved context (`None` skips the tick).
fn backup_timer_tick(context: Option<BackupContext>, now: DateTime<Utc>) {
    if let Some(ctx) = context {
        log_outcome(
            "interval",
            maybe_export(
                ctx.store.as_ref(),
                ctx.target.as_ref(),
                &ctx.profile_dir,
                now,
            ),
        );
    }
}

/// Spawn the interval-export daemon thread (Decision 5). Errors are logged,
/// never propagated — backup must not interrupt the user. `poll` is
/// [`BACKUP_TIMER_POLL`] in production, injected so tests can observe a tick.
/// Each tick resolves the active profile's [`BackupContext`] fresh; `None`
/// (no profile unlocked, or a switch in flight) skips the tick.
pub fn start_backup_timer<F>(resolve: F, poll: Duration)
where
    F: Fn() -> Option<BackupContext> + Send + 'static,
{
    std::thread::spawn(move || loop {
        std::thread::sleep(poll);
        backup_timer_tick(resolve(), Utc::now());
    });
}

/// Best-effort final export on graceful exit, bounded by `timeout`
/// (Decision 5): the export runs on a worker thread and the exit proceeds
/// when it finishes or the timeout elapses, whichever comes first.
pub fn export_on_exit(
    store: Arc<dyn Store + Send + Sync>,
    target: Arc<dyn BackupTarget>,
    profile_dir: PathBuf,
    timeout: Duration,
    now: DateTime<Utc>,
) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = export_if_dirty(store.as_ref(), target.as_ref(), &profile_dir, now);
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(timeout) {
        Ok(result) => log_outcome("exit", result),
        Err(_) => {
            log::warn!("backup: exit export timed out — the interval timer bounds the loss");
        }
    }
}

fn log_outcome(trigger: &str, result: Result<Option<ExportOutcome>, AppError>) {
    match result {
        Ok(Some(ExportOutcome::Uploaded)) => log::info!("backup: {trigger} export uploaded"),
        Ok(Some(ExportOutcome::SkippedStaleRemote)) => {
            log::warn!("backup: {trigger} export skipped — remote backup is newer");
        }
        Ok(None) => {}
        Err(e) => log::warn!("backup: {trigger} export failed: {e}"),
    }
}
