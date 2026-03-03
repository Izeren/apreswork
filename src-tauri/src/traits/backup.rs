// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Provider-generic backup storage contract.
//!
//! The trait speaks only neutral types — nothing provider-specific (Drive file
//! ids, folder lookups, multipart uploads) crosses this boundary; those stay
//! private to the concrete impls in `crate::backup`. One backup file per
//! target; retention is the target's own version history (plans/drive-backup
//! Decision 8).

use chrono::{DateTime, Utc};

use crate::error::AppError;

/// Freshness metadata carried alongside the backup archive.
///
/// Written at upload time from the exporting database's config and compared
/// on both the read side (auto-restore) and the write side (stale-writer
/// guard). Comparisons use these embedded device-clock values, never the
/// target's server-side modification time (Decision 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBackupMeta {
    /// The exporting database's `last_mutation` at upload time.
    /// `None` means the database had never been mutated.
    pub last_mutation: Option<DateTime<Utc>>,
    /// The exporting database's schema version. Restore refuses a backup
    /// newer than the running binary understands.
    pub schema_version: i64,
}

/// Provider-generic backup storage: one archive per connected account.
///
/// Implementations must be cheap to construct — a target is built during
/// profile activation before the store opens (restore check) and again as
/// part of the long-lived `AppState`.
pub trait BackupTarget: Send + Sync {
    /// Cheap availability probe (no network): credentials present.
    #[must_use]
    fn is_available(&self) -> bool;

    /// Read the remote backup's freshness metadata.
    ///
    /// Returns `Ok(None)` when no backup exists on the target yet.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Backup`] on provider or network error, including
    /// unreadable metadata (callers must not guess at freshness).
    fn get_meta(&self, now: DateTime<Utc>) -> Result<Option<RemoteBackupMeta>, AppError>;

    /// Upload the archive, replacing any previous backup, and stamp `meta`
    /// as its freshness metadata.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Backup`] on provider or network error.
    fn upload(
        &self,
        now: DateTime<Utc>,
        zip_bytes: &[u8],
        meta: &RemoteBackupMeta,
    ) -> Result<(), AppError>;

    /// # Errors
    ///
    /// Returns [`AppError::Backup`] on provider or network error, or when no
    /// backup exists (callers check [`BackupTarget::get_meta`] first).
    fn download(&self, now: DateTime<Utc>) -> Result<Vec<u8>, AppError>;
}
