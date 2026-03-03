// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Provider-generic calendar synchronization contract.
//!
//! The trait speaks only neutral types — nothing provider-specific (OAuth
//! details, etags, batch endpoints) crosses this boundary; those stay private
//! to the concrete impls in `crate::calendar`. Push operations (`SyncOp`,
//! batch execution) are introduced with the push phase; v-now is the read +
//! auth surface needed by the event-pull phase.

use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// An event read from the dedicated app calendar (with etag for three-way merge).
///
/// `list_app_calendar_events` only returns events bearing the
/// `extendedProperties.private.apreswork_chunk_id` marker (app-owned events).
/// Events without this marker (user-created events) are excluded and handled by G11.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChunkEvent {
    pub event_id: String,
    pub etag: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub title: String,
    pub description: Option<String>,
}

/// The content to write for a chunk event (provider-neutral).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkEventPayload {
    /// Chunk ID stamped in `extendedProperties.private.apreswork_chunk_id` on Create.
    pub chunk_id: String,
    pub title: String,
    /// Multi-chunk format (trimmed): `"Part N of M — Après Work\n\n{task_description}"`.
    /// Single-chunk format (trimmed): `"Après Work\n\n{task_description}"`.
    pub description: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// The content to write for a user-owned calendar event (provider-neutral).
///
/// Distinct from [`ChunkEventPayload`]: a user event carries no chunk marker
/// and its description is optional (a chunk event always has a formatted body).
/// Used by the in-app create/edit flow (G11), which writes ordinary events the
/// user owns rather than app-scheduled chunks.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UserEventPayload {
    pub title: String,
    pub description: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// When true, write the event as all-day (Google `date` fields). `start`/`end`
    /// are the local-midnight instants of the inclusive first and exclusive last
    /// day, matching the all-day pull convention (`parse_event_datetime`).
    pub all_day: bool,
}

/// A sync operation to execute on the provider.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncOp {
    Create(ChunkEventPayload),
    Update {
        event_id: String,
        payload: ChunkEventPayload,
    },
    Delete {
        event_id: String,
    },
}

/// Type-safe per-operation result, positionally aligned with the `ops` slice.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncOpResult {
    Created {
        chunk_id: String,
        event_id: String,
        etag: Option<String>,
    },
    Updated {
        chunk_id: String,
        event_id: String,
        etag: Option<String>,
    },
    Deleted,
}

/// Authentication state of the active calendar provider.
///
/// Serialized as a discriminated union with a `type` field (frontend
/// contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthStatus {
    /// No credentials stored (or no real provider configured).
    NotConnected,
    /// Consent flow started; waiting for the browser redirect.
    Pending,
    /// Valid credentials stored.
    Connected {
        /// The email address of the connected account, if known.
        email: Option<String>,
    },
}

/// A calendar visible to the connected account (Settings picker row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalCalendar {
    /// Provider-assigned calendar identifier.
    pub id: String,
    /// Human-readable calendar name.
    pub title: String,
    /// The account's primary calendar (shown first in pickers).
    pub primary: bool,
}

/// A provider event, normalized to UTC instants.
///
/// All-day events are still converted by the provider impl to their full
/// local-day UTC span — `start`/`end` are always instants — but `all_day` marks
/// them so the UI can render them as all-day and a write-back can round-trip
/// them as date-only rather than timed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalEvent {
    /// Provider-assigned calendar identifier this event belongs to.
    pub calendar_id: String,
    /// Provider-assigned event identifier.
    pub event_id: String,
    /// Event title/summary.
    pub title: String,
    /// Optional event description.
    pub description: Option<String>,
    /// Inclusive start of the event (UTC).
    pub start: DateTime<Utc>,
    /// Exclusive end of the event (UTC).
    pub end: DateTime<Utc>,
    /// False for provider "free"/transparent events — they never block slots.
    pub busy: bool,
    /// True when the user declined this invitation (rendered dimmed, not busy).
    pub declined: bool,
    /// True for a date-only (all-day) event. `start`/`end` still carry the full
    /// local-day UTC span; this flags the original date-only representation so a
    /// write-back round-trips it as all-day (Google `date` fields).
    pub all_day: bool,
}

/// Provider-generic calendar synchronization.
///
/// The trait speaks only neutral types — nothing provider-specific crosses
/// this boundary. Auth is a loopback-redirect flow: `begin_auth` returns the
/// consent URL and the exchange completes in the background; there is no
/// `complete_auth(code)` (the oob copy-paste flow was removed by providers).
/// Push operations (`SyncOp`, batched execution) are added with the push
/// phase. Event pull is full-content per selected calendar (supersedes the
/// old `list_busy_times` freebusy sketch).
//
// CalendarSync repeats the module name; it is the primary export of this module.
#[allow(clippy::module_name_repetitions)]
pub trait CalendarSync: Send + Sync {
    /// Start the interactive auth flow. Returns the consent URL for the UI to
    /// open in the system browser; the exchange completes in the background.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] if no provider is configured or the
    /// flow cannot be started.
    fn begin_auth(&self, now: DateTime<Utc>, now_instant: Instant) -> Result<String, AppError>;

    /// Current authentication state (cheap, no network).
    #[must_use]
    fn auth_status(&self, now_instant: Instant) -> AuthStatus;

    /// Wipe local auth state only (token file, pending flow). Remote data is
    /// never touched by disconnect.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] if the local credential store cannot
    /// be cleared.
    fn disconnect(&self) -> Result<(), AppError>;

    /// Cheap availability probe (no network): credentials present and the
    /// provider is not in an error backoff.
    #[must_use]
    fn is_available(&self) -> bool;

    /// List calendars visible to the connected account.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn list_calendars(&self, now: DateTime<Utc>) -> Result<Vec<ExternalCalendar>, AppError>;

    /// List events on one calendar overlapping `[start, end)`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn list_events(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEvent>, AppError>;

    /// Get or create the dedicated "Après Work" calendar and return its provider ID.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn ensure_app_calendar(&self, now: DateTime<Utc>) -> Result<String, AppError>;

    /// List events on the dedicated app calendar overlapping `[start, end)`.
    ///
    /// **MUST** filter to events bearing `extendedProperties.private.apreswork_chunk_id`.
    /// Events without this marker (user-created events) are excluded and handled by G11.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn list_app_calendar_events(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<RemoteChunkEvent>, AppError>;

    /// Execute sync operations on the provider via the multipart batch API.
    ///
    /// Returns one `SyncOpResult` per entry in `ops`, positionally aligned.
    /// Ops are pushed in fixed-size batches; throttled or transient inner
    /// failures are retried with exponential backoff. A permanent inner failure
    /// fails the whole call — prior ops are not rolled back, and Phase B
    /// reconciles any resulting orphans on the next sync.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn execute_sync_ops(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        ops: &[SyncOp],
    ) -> Result<Vec<SyncOpResult>, AppError>;

    /// Create a user-owned event on `calendar_id` and return the resulting
    /// mirror event.
    ///
    /// The event carries NO chunk marker — it is an ordinary event the user
    /// owns (G11), distinct from app-scheduled chunk events. `busy` / `declined`
    /// on the returned [`ExternalEvent`] are derived from the provider response
    /// (Decision 5), never assumed by the caller.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn create_user_event(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError>;

    /// Update a user-owned event's time/content and return the resulting mirror
    /// event.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn update_user_event(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        event_id: &str,
        payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError>;

    /// Delete a user-owned event. Idempotent: an already-deleted event is a
    /// success.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] on provider or network error.
    fn delete_user_event(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<(), AppError>;
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::{AuthStatus, ExternalCalendar, ExternalEvent};

    #[test_case(
        AuthStatus::NotConnected,
        r#"{"type":"not_connected"}"#;
        "not_connected_serializes"
    )]
    #[test_case(
        AuthStatus::Pending,
        r#"{"type":"pending"}"#;
        "pending_serializes"
    )]
    #[test_case(
        AuthStatus::Connected { email: None },
        r#"{"type":"connected","email":null}"#;
        "connected_no_email_serializes"
    )]
    #[test_case(
        AuthStatus::Connected { email: Some("user@example.com".into()) },
        r#"{"type":"connected","email":"user@example.com"}"#;
        "connected_with_email_serializes"
    )]
    // test_case passes values directly; taking AuthStatus by value reads
    // naturally in the annotations without requiring `&` on every entry.
    #[allow(clippy::needless_pass_by_value)]
    fn auth_status_serializes(status: AuthStatus, expected_json: &str) {
        let json = serde_json::to_string(&status).expect("serialize");
        assert_eq!(json, expected_json);
    }

    #[test]
    fn external_calendar_roundtrip() {
        let cal = ExternalCalendar {
            id: "cal1".into(),
            title: "Work".into(),
            primary: true,
        };
        let json = serde_json::to_string(&cal).expect("serialize");
        let back: ExternalCalendar = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cal);
    }

    #[test]
    fn external_event_roundtrip_and_datetime_format() {
        let event = ExternalEvent {
            calendar_id: "cal1".into(),
            event_id: "ev1".into(),
            title: "Meeting".into(),
            description: None,
            start: crate::test_support::utc(2026, 7, 11, 10, 0),
            end: crate::test_support::utc(2026, 7, 11, 11, 0),
            busy: true,
            declined: false,
            all_day: false,
        };
        let json = serde_json::to_value(&event).expect("serialize");
        assert_eq!(json["start"], "2026-07-11T10:00:00Z");
        assert_eq!(json["end"], "2026-07-11T11:00:00Z");
        assert_eq!(json["all_day"], false);
        let back: ExternalEvent = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, event);
    }
}
