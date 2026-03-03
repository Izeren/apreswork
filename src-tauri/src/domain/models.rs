// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Core domain structs. Field definitions follow DESIGN.md §3.1.

use chrono::{DateTime, NaiveTime, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::cadence::Cadence;
use super::enums::{ChunkStatus, Priority, TaskStatus};
use crate::error::AppError;

/// All entity IDs are UUID v7 strings for time-ordering.
pub type EntityId = String;

/// A schedulable unit of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: EntityId,
    pub title: String,
    pub description: Option<String>,
    pub duration_minutes: i64,
    pub time_logged_minutes: i64,
    pub priority: Priority,
    pub status: TaskStatus,
    pub start_date: Option<DateTime<Utc>>,
    /// Always required for persisted tasks (enforced by validation). `Option`
    /// only because the Rust type is reused for virtual orphaned-template
    /// instances, which compute a deadline but are never persisted.
    pub deadline: Option<DateTime<Utc>>,
    pub schedule_id: EntityId,
    /// Minimum chunk size when splitting. Default 30, minimum 5.
    pub min_chunk_minutes: i64,
    pub no_split: bool,
    pub recurring_template_id: Option<EntityId>,
    pub expire_at: Option<DateTime<Utc>>,
    /// User has manually placed (pinned) this task's schedule by dragging or
    /// resizing a chunk. Invariant: `is_pinned` ⇔ the task has at least one
    /// fixed chunk. Pinned recurring instances are sticky — reconcile never
    /// repositions or deletes them, and auto-cancellation skips them.
    pub is_pinned: bool,
    /// Denormalized from join table on read.
    pub labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A time block allocated to a [`Task`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: EntityId,
    pub task_id: EntityId,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub status: ChunkStatus,
    pub is_fixed: bool,
    /// Actual time logged on completion (override or scheduled duration).
    /// Used by `reopen_chunk` to subtract the correct amount.
    pub logged_minutes: Option<i64>,
    pub completed_at: Option<DateTime<Utc>>,
    pub google_event_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Template for generating recurring [`Task`] instances.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurringTemplate {
    pub id: EntityId,
    pub title: String,
    pub description: Option<String>,
    pub duration_minutes: i64,
    pub priority: Priority,
    pub schedule_id: EntityId,
    pub cadence: Cadence,
    pub labels: Vec<String>,
    pub is_active: bool,
    pub start_date: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A named set of [`ScheduleWindow`]s defining available time slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: EntityId,
    pub name: String,
    pub is_default: bool,
    pub windows: Vec<ScheduleWindow>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A single time window within a [`Schedule`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleWindow {
    pub id: EntityId,
    pub schedule_id: EntityId,
    pub day_of_week: Weekday,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
}

impl Schedule {
    /// Largest single window duration in minutes — the schedule's capacity
    /// ceiling for any unsplittable block of work.
    ///
    /// Returns `0` when the schedule has no windows (a window-less schedule
    /// cannot accommodate any work block).
    #[must_use]
    pub fn largest_window_minutes(&self) -> i64 {
        self.windows
            .iter()
            .map(|w| (w.end_time - w.start_time).num_minutes())
            .max()
            .unwrap_or(0)
    }
}

/// Global application configuration (singleton row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Number of days into the future to schedule. Default 30.
    pub planning_horizon_days: i64,
    /// IANA timezone string, e.g. `"Europe/Berlin"`.
    pub timezone: String,
    /// Max back-to-back scheduled time (minutes) before a break is required.
    /// Default 120.
    pub max_continuous_minutes: i64,
    /// Minimum break duration (minutes) inserted between continuous blocks.
    /// Default 5.
    pub min_break_minutes: i64,
    pub last_reschedule: Option<DateTime<Utc>>,
    /// Tracks when chunks were last changed (for sync debounce).
    pub last_mutation: Option<DateTime<Utc>>,
    /// Tracks when Google Calendar chunk sync last ran.
    pub last_sync: Option<DateTime<Utc>>,
    /// Tracks when Google Calendar busy times were last cached.
    pub last_busy_sync: Option<DateTime<Utc>>,
}

impl AppConfig {
    /// Parse the configured [`timezone`](Self::timezone) into a [`Tz`].
    ///
    /// # Errors
    ///
    /// [`AppError::Validation`] if the stored string is not a valid IANA zone.
    pub fn timezone_tz(&self) -> Result<Tz, AppError> {
        self.timezone
            .parse()
            .map_err(|_| AppError::Validation(format!("invalid timezone: {}", self.timezone)))
    }
}

/// A locally mirrored external calendar event (provider-owned, read-only).
///
/// Rows inside the pulled window are refreshed on every pull; rows that have
/// aged out of the window are retained indefinitely as history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalEventRecord {
    pub id: EntityId,
    /// Provider calendar the event lives in.
    pub calendar_id: String,
    /// Provider event id (unique across all calendars).
    pub event_id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    /// False for provider "free"/transparent events — they never block slots.
    pub busy: bool,
    /// True when the user declined this invitation (rendered dimmed, not busy).
    pub declined: bool,
    /// True for a date-only (all-day) event; `start_time`/`end_time` still carry
    /// the full local-day UTC span. Drives all-day rendering and write-back.
    pub all_day: bool,
    pub updated_at: DateTime<Utc>,
}

/// Provider link state (singleton `google_auth` row — legacy table name kept
/// until a second provider forces a rename).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleAuthState {
    /// Dedicated app calendar id (set by the push phase; None until then).
    pub calendar_id: Option<String>,
    /// When the account was connected (None = never / disconnected).
    pub connected_at: Option<DateTime<Utc>>,
}

/// A comment attached to a [`Task`] (M12).
///
/// `author` is a plain string; `"SYSTEM"` is reserved for auto-generated
/// progress comments (M12.2). Comments are cascade-deleted with their task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: EntityId,
    pub task_id: EntityId,
    pub author: String,
    /// Markdown content.
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Sync base for a chunk event — last successfully pushed/accepted state.
/// Used as the merge base in three-way reconciliation (G5 push sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkSyncState {
    pub chunk_id: EntityId,
    pub event_id: String,
    pub etag: Option<String>,
    pub synced_start: DateTime<Utc>,
    pub synced_end: DateTime<Utc>,
    pub synced_title: String,
    pub synced_description: String,
    pub updated_at: DateTime<Utc>,
}

/// Generate a test-only `with_id` builder for `$ty`, which has a
/// `pub id: EntityId` field. Every domain struct shares this exact override,
/// so this is the one definition each `impl $ty` block below relies on.
macro_rules! with_id_builder {
    ($ty:ty) => {
        #[cfg(test)]
        impl $ty {
            pub(crate) fn with_id(mut self, id: &str) -> Self {
                self.id = id.to_owned();
                self
            }
        }
    };
}

with_id_builder!(Task);
with_id_builder!(RecurringTemplate);
with_id_builder!(Chunk);
with_id_builder!(Comment);
with_id_builder!(Schedule);

#[cfg(test)]
impl Task {
    pub(crate) fn test_default() -> Self {
        let now = crate::test_support::fixture_base();
        Self {
            id: "test-task-1".to_owned(),
            title: "Test task".to_owned(),
            description: None,
            duration_minutes: 60,
            time_logged_minutes: 0,
            priority: Priority::Medium,
            status: TaskStatus::Pending,
            start_date: None,
            deadline: Some(now + chrono::Duration::days(7)),
            schedule_id: "default-schedule-id".to_owned(),
            min_chunk_minutes: 30,
            no_split: false,
            recurring_template_id: None,
            expire_at: None,
            is_pinned: false,
            labels: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn with_status(mut self, status: TaskStatus) -> Self {
        self.status = status;
        self
    }

    pub(crate) fn with_template(mut self, template_id: &str) -> Self {
        self.recurring_template_id = Some(template_id.to_owned());
        self
    }

    pub(crate) fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub(crate) fn with_expire_at(mut self, expire_at: DateTime<Utc>) -> Self {
        self.expire_at = Some(expire_at);
        self
    }

    pub(crate) fn with_pinned(mut self, is_pinned: bool) -> Self {
        self.is_pinned = is_pinned;
        self
    }

    pub(crate) fn with_schedule(mut self, schedule_id: &str) -> Self {
        self.schedule_id = schedule_id.to_owned();
        self
    }
}

#[cfg(test)]
impl RecurringTemplate {
    pub(crate) fn test_default() -> Self {
        let now = crate::test_support::fixture_base();
        Self {
            id: "test-tmpl-1".to_owned(),
            title: "Test template".to_owned(),
            description: None,
            duration_minutes: 60,
            priority: Priority::Medium,
            schedule_id: "default-schedule-id".to_owned(),
            cadence: Cadence::weekly(vec![chrono::Weekday::Mon]),
            labels: Vec::new(),
            is_active: true,
            start_date: now,
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn with_start_date(mut self, start_date: DateTime<Utc>) -> Self {
        self.start_date = start_date;
        self
    }

    pub(crate) fn with_active(mut self, is_active: bool) -> Self {
        self.is_active = is_active;
        self
    }

    pub(crate) fn with_cadence(mut self, cadence: Cadence) -> Self {
        self.cadence = cadence;
        self
    }

    pub(crate) fn with_schedule(mut self, schedule_id: &str) -> Self {
        self.schedule_id = schedule_id.to_owned();
        self
    }
}

#[cfg(test)]
impl Chunk {
    pub(crate) fn test_default() -> Self {
        let now = crate::test_support::fixture_base();
        Self {
            id: "test-chunk-1".to_owned(),
            task_id: "test-task-1".to_owned(),
            start_time: now,
            end_time: now + chrono::Duration::hours(1),
            status: ChunkStatus::Scheduled,
            is_fixed: false,
            logged_minutes: None,
            completed_at: None,
            google_event_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn with_task(mut self, task_id: &str) -> Self {
        self.task_id = task_id.to_owned();
        self
    }

    pub(crate) fn with_status(mut self, status: ChunkStatus) -> Self {
        self.status = status;
        self
    }

    pub(crate) fn with_fixed(mut self, is_fixed: bool) -> Self {
        self.is_fixed = is_fixed;
        self
    }

    pub(crate) fn with_times(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_time = start;
        self.end_time = end;
        self
    }
}

#[cfg(test)]
impl Comment {
    pub(crate) fn test_default() -> Self {
        let now = crate::test_support::fixture_base();
        Self {
            id: "test-comment-1".to_owned(),
            task_id: "test-task-1".to_owned(),
            author: "User".to_owned(),
            content: "A test comment".to_owned(),
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn with_task(mut self, task_id: &str) -> Self {
        self.task_id = task_id.to_owned();
        self
    }

    pub(crate) fn with_author(mut self, author: &str) -> Self {
        self.author = author.to_owned();
        self
    }

    pub(crate) fn with_timestamps(
        mut self,
        created: DateTime<Utc>,
        updated: DateTime<Utc>,
    ) -> Self {
        self.created_at = created;
        self.updated_at = updated;
        self
    }
}

#[cfg(test)]
impl Schedule {
    /// A non-default, window-less schedule. Window-less is fine for FK parents;
    /// tests that exercise slot-finding build their own windowed schedules.
    pub(crate) fn test_default() -> Self {
        let now = crate::test_support::fixture_base();
        Self {
            id: "test-schedule-1".to_owned(),
            name: "Test schedule".to_owned(),
            is_default: false,
            windows: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn with_default(mut self, is_default: bool) -> Self {
        self.is_default = is_default;
        self
    }

    pub(crate) fn with_windows(mut self, windows: Vec<ScheduleWindow>) -> Self {
        self.windows = windows;
        self
    }
}
