// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! No-op backup target used when no cloud provider is configured.

use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::traits::backup::{BackupTarget, RemoteBackupMeta};

/// Fallback [`BackupTarget`]: never available, never holds a backup.
//
// BackupTarget suffix repeats the module family name; kept for symmetry with
// NoopCalendarSync.
#[allow(clippy::module_name_repetitions)]
pub struct NoopBackupTarget;

impl BackupTarget for NoopBackupTarget {
    fn is_available(&self) -> bool {
        false
    }

    fn get_meta(&self, _now: DateTime<Utc>) -> Result<Option<RemoteBackupMeta>, AppError> {
        Ok(None)
    }

    fn upload(
        &self,
        _now: DateTime<Utc>,
        _zip_bytes: &[u8],
        _meta: &RemoteBackupMeta,
    ) -> Result<(), AppError> {
        Err(AppError::Backup("no backup target configured".into()))
    }

    fn download(&self, _now: DateTime<Utc>) -> Result<Vec<u8>, AppError> {
        Err(AppError::Backup("no backup target configured".into()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::NoopBackupTarget;
    use crate::traits::backup::{BackupTarget, RemoteBackupMeta};

    #[test]
    fn noop_is_never_available_and_never_holds_a_backup() {
        let target = NoopBackupTarget;
        assert!(!target.is_available());
        let now = chrono::DateTime::<Utc>::UNIX_EPOCH;
        assert_eq!(target.get_meta(now).expect("meta"), None);
        let meta = RemoteBackupMeta {
            last_mutation: None,
            schema_version: 1,
        };
        assert!(target.upload(now, b"zip", &meta).is_err());
        assert!(target.download(now).is_err());
    }
}
