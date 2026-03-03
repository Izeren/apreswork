// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

/// Application-wide error type used across all layers.
///
/// Serialized as `{ "error": "<variant>", "message": "<display>" }` for Tauri IPC.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AppError {
    #[error("Not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Database error: {0}")]
    Database(String),

    // SECURITY: never include OAuth tokens or credentials in this message.
    // Only include human-readable descriptions of what went wrong.
    #[error("Calendar sync error: {0}")]
    CalendarSync(String),

    // SECURITY: same rule as CalendarSync — no token material, ever.
    #[error("Backup error: {0}")]
    Backup(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Profile mismatch: {0}")]
    ProfileMismatch(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let variant = match self {
            Self::NotFound { .. } => "not_found",
            Self::Validation(_) => "validation",
            Self::Database(_) => "database",
            Self::CalendarSync(_) => "calendar_sync",
            Self::Backup(_) => "backup",
            Self::Internal(_) => "internal",
            Self::ProfileMismatch(_) => "profile_mismatch",
        };

        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("error", variant)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Database(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn not_found_display() {
        let err = AppError::NotFound {
            entity: "Task".to_owned(),
            id: "abc-123".to_owned(),
        };
        assert_eq!(err.to_string(), "Not found: Task with id abc-123");
    }

    #[test]
    fn validation_display() {
        let err = AppError::Validation("title must not be empty".to_owned());
        assert_eq!(err.to_string(), "Validation error: title must not be empty");
    }

    #[test]
    fn database_display() {
        let err = AppError::Database("connection failed".to_owned());
        assert_eq!(err.to_string(), "Database error: connection failed");
    }

    #[test]
    fn calendar_sync_display() {
        let err = AppError::CalendarSync("API rate limit exceeded".to_owned());
        assert_eq!(
            err.to_string(),
            "Calendar sync error: API rate limit exceeded"
        );
    }

    #[test]
    fn backup_display() {
        let err = AppError::Backup("upload failed".to_owned());
        assert_eq!(err.to_string(), "Backup error: upload failed");
    }

    #[test]
    fn serialize_backup() {
        let err = AppError::Backup("stale writer".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "backup");
        assert_eq!(json["message"], "Backup error: stale writer");
    }

    #[test]
    fn internal_display() {
        let err = AppError::Internal("unexpected state".to_owned());
        assert_eq!(err.to_string(), "Internal error: unexpected state");
    }

    #[test]
    fn serialize_not_found() {
        let err = AppError::NotFound {
            entity: "Task".to_owned(),
            id: "abc-123".to_owned(),
        };
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "not_found");
        assert_eq!(json["message"], "Not found: Task with id abc-123");
    }

    #[test]
    fn serialize_validation() {
        let err = AppError::Validation("bad input".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "validation");
        assert_eq!(json["message"], "Validation error: bad input");
    }

    #[test]
    fn serialize_database() {
        let err = AppError::Database("disk full".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "database");
        assert_eq!(json["message"], "Database error: disk full");
    }

    #[test]
    fn serialize_calendar_sync() {
        let err = AppError::CalendarSync("timeout".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "calendar_sync");
        assert_eq!(json["message"], "Calendar sync error: timeout");
    }

    #[test]
    fn serialize_internal() {
        let err = AppError::Internal("panic recovered".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "internal");
        assert_eq!(json["message"], "Internal error: panic recovered");
    }

    #[test]
    fn profile_mismatch_display() {
        let err = AppError::ProfileMismatch("active profile is X, expected Y".to_owned());
        assert_eq!(
            err.to_string(),
            "Profile mismatch: active profile is X, expected Y"
        );
    }

    #[test]
    fn serialize_profile_mismatch() {
        let err = AppError::ProfileMismatch("wrong profile".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["error"], "profile_mismatch");
        assert_eq!(json["message"], "Profile mismatch: wrong profile");
    }

    #[test]
    fn from_rusqlite_error() {
        let rusqlite_err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".to_owned()),
        );
        let app_err = AppError::from(rusqlite_err);
        assert!(matches!(app_err, AppError::Database(_)));
        assert!(
            app_err.to_string().contains("locked"),
            "expected 'locked' in: {app_err}"
        );
    }

    #[test]
    fn serialize_has_exactly_two_fields() {
        let err = AppError::Internal("test".to_owned());
        let json = serde_json::to_value(&err).expect("serialize");
        let obj = json.as_object().expect("should be object");
        assert_eq!(obj.len(), 2, "expected exactly 2 fields: error and message");
        assert!(obj.contains_key("error"));
        assert!(obj.contains_key("message"));
    }

    #[test]
    fn clone_preserves_value() {
        let err = AppError::NotFound {
            entity: "Task".to_owned(),
            id: "x".to_owned(),
        };
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }
}
