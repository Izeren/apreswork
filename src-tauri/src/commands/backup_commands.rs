// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Backup Tauri commands: status card feed, the enable toggle, manual
//! "Back up now", and file export/import. Thin wrappers per the layering
//! invariant — parse, call service, return.

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use std::path::PathBuf;

use chrono::Utc;

use crate::error::AppError;
use crate::services::backup as service;
use crate::services::backup::BackupStatus;
use crate::state::{ActiveState, BackupWriteHandles};

fn read_backup_status(state: &crate::state::AppState) -> Result<BackupStatus, AppError> {
    service::get_backup_status(
        state.store.as_ref(),
        state.backup.as_ref(),
        state.restore_notice.as_deref(),
    )
}

/// Read backup bookkeeping for the Settings card. No network call.
///
/// # Errors
///
/// Returns [`AppError::Validation`] when no profile is active,
/// [`AppError::Database`] on storage failure.
#[tauri::command]
pub fn get_backup_status(active: tauri::State<'_, ActiveState>) -> Result<BackupStatus, AppError> {
    let state = active.get()?;
    read_backup_status(&state)
}

/// # Errors
///
/// Returns [`AppError::Validation`] when enabling without a connected
/// target, [`AppError::Database`] on storage failure.
#[tauri::command]
pub fn set_backup_enabled(
    active: tauri::State<'_, ActiveState>,
    enabled: bool,
) -> Result<BackupStatus, AppError> {
    let state = active.get()?;
    service::set_backup_enabled(state.store.as_ref(), state.backup.as_ref(), enabled)?;
    read_backup_status(&state)
}

/// Run the manual-backup snapshot/upload, then re-read fresh status.
///
/// Shared by the `backup_now` Tauri command and the REST
/// `POST /api/backup/now` handler — each awaits this via its own
/// `spawn_blocking`-flavored helper (see the "Provider / async lore" note in
/// `src-tauri/CLAUDE.md` for why those stay separate).
///
/// # Errors
///
/// Returns [`AppError::Validation`] when backup is not connected,
/// [`AppError::Backup`] on snapshot/upload failure (also recorded in the
/// status).
pub fn run_backup_now(handles: BackupWriteHandles) -> Result<BackupStatus, AppError> {
    let (store, target, work_dir, restore_notice) = handles;
    service::export_now(store.as_ref(), target.as_ref(), &work_dir, Utc::now())?;
    service::get_backup_status(store.as_ref(), target.as_ref(), restore_notice.as_deref())
}

/// Manual "Back up now": bypasses the dirty/interval gates but keeps the
/// stale-writer guard (Decision 5); returns fresh status for the card. See
/// [`run_backup_now`] for the snapshot/upload behavior.
///
/// Uses `spawn_blocking` because the upload is blocking HTTP.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if the blocking task is cancelled.
/// Propagates any error from [`run_backup_now`].
#[tauri::command]
pub async fn backup_now(active: tauri::State<'_, ActiveState>) -> Result<BackupStatus, AppError> {
    let handles = active.backup_write_handles()?;
    tauri::async_runtime::spawn_blocking(move || run_backup_now(handles))
        .await
        .map_err(|e| AppError::Internal(format!("backup task failed: {e}")))?
}

/// Write a backup archive of the live database to `path` (manual export,
/// M11.3). The path comes from the frontend's save dialog.
///
/// Uses `spawn_blocking` because the snapshot + write are blocking I/O.
///
/// # Errors
///
/// Returns [`AppError::Backup`] on snapshot/write failure,
/// [`AppError::Internal`] if the blocking task is cancelled.
#[tauri::command]
pub async fn export_backup_to_file(
    active: tauri::State<'_, ActiveState>,
    path: String,
) -> Result<(), AppError> {
    let state = active.get()?;
    let store = state.store.clone();
    let work_dir = state.profile_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::export_to_file(store.as_ref(), &PathBuf::from(path), &work_dir)
    })
    .await
    .map_err(|e| AppError::Internal(format!("export task failed: {e}")))?
}

/// Verify + stage a manual import, then restart the app; the swap happens
/// during the next activation, before the store opens (restarting is the
/// safe way to replace the database file under the live store).
///
/// On success this never returns: the process restarts.
///
/// # Errors
///
/// Returns [`AppError::Backup`] when the archive fails verification or
/// staging fails (the live database is untouched, no restart),
/// [`AppError::Internal`] if the blocking task is cancelled.
#[tauri::command]
pub async fn import_backup_from_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    active: tauri::State<'_, ActiveState>,
    path: String,
) -> Result<(), AppError> {
    let state = active.get()?;
    let profile_dir = state.profile_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::stage_import(&PathBuf::from(path), &profile_dir, Utc::now())
    })
    .await
    .map_err(|e| AppError::Internal(format!("import task failed: {e}")))??;
    app.restart()
}

#[cfg(test)]
mod tests {
    use crate::commands::test_support::{tempdir, Arc, Manager as _, MockRuntime, TempDir};

    use super::{backup_now, export_backup_to_file, get_backup_status, set_backup_enabled};
    use crate::backup::noop::NoopBackupTarget;
    use crate::error::AppError;
    use crate::state::{ActiveState, AppState};
    use crate::test_support::make_scheduler_stack;

    /// A mock app with a managed, pre-filled `ActiveState` on a real
    /// (temp-dir) database and the noop backup target — deterministic
    /// regardless of compiled creds.
    fn backup_app(restore_notice: Option<String>) -> (TempDir, tauri::App<MockRuntime>) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("apreswork.db");
        let store = Arc::new(
            crate::db::sqlite::SqliteStore::new(db_path.to_str().expect("utf-8 path"))
                .expect("open store"),
        );
        let (scheduler, trigger) = make_scheduler_stack(store.clone());
        let state = AppState {
            store,
            scheduler,
            trigger,
            calendar_sync: Arc::new(crate::calendar::noop::NoopCalendarSync),
            backup: Arc::new(NoopBackupTarget),
            profile_dir: dir.path().to_path_buf(),
            restore_notice,
            profile: crate::profiles::ActiveProfile {
                id: "p-backup".to_owned(),
                name: "Backup".to_owned(),
            },
        };
        let app = tauri::test::mock_app();
        app.manage(ActiveState::from(Arc::new(state)));
        (dir, app)
    }

    #[test]
    fn backup_commands_reject_an_empty_active_slot() {
        let app = tauri::test::mock_app();
        app.manage(ActiveState::new());
        let err = get_backup_status(app.state()).expect_err("must reject");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn get_backup_status_reports_disconnected_and_the_restore_notice() {
        let (_dir, app) = backup_app(Some("2026-07-12T10:00:00+00:00".to_owned()));
        let status = get_backup_status(app.state()).expect("status");
        assert!(!status.enabled);
        assert!(!status.connected);
        assert_eq!(
            status.restored_this_run.as_deref(),
            Some("2026-07-12T10:00:00+00:00")
        );
    }

    #[test]
    fn set_backup_enabled_rejects_enabling_without_a_connection() {
        let (_dir, app) = backup_app(None);
        let err = set_backup_enabled(app.state(), true).expect_err("must reject");
        assert!(matches!(err, AppError::Validation(_)));
        let status = set_backup_enabled(app.state(), false).expect("disable is always fine");
        assert!(!status.enabled);
    }

    #[tokio::test]
    async fn backup_now_without_a_connection_is_a_validation_error() {
        let (_dir, app) = backup_app(None);
        let err = backup_now(app.state()).await.expect_err("must reject");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn export_backup_to_file_writes_a_zip() {
        let (dir, app) = backup_app(None);
        let dest = dir.path().join("manual-export.zip");
        export_backup_to_file(app.state(), dest.to_str().expect("utf-8").to_owned())
            .await
            .expect("export");
        let bytes = std::fs::read(&dest).expect("read export");
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..2], b"PK", "must be a zip archive");
    }

    // `import_backup_from_file`'s happy path calls `app.restart()` (process
    // control) — exercised manually; the verification/staging logic it
    // delegates to is covered in services::backup::tests.
    #[tokio::test]
    async fn import_backup_from_file_rejects_a_missing_file_without_restart() {
        let (dir, app) = backup_app(None);
        let missing = dir.path().join("nope.zip");
        let err = super::import_backup_from_file(
            app.handle().clone(),
            app.state(),
            missing.to_str().expect("utf-8").to_owned(),
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, AppError::Backup(_)));
    }
}
