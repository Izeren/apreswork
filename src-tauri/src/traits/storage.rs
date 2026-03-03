// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Storage trait interfaces for all domain entities.
//!
//! Each sub-trait covers one aggregate root. The [`Store`] supertrait
//! combines them so a single backend implementation can satisfy all
//! storage needs.
//!
//! None of these traits require `Send + Sync`: [`Store::with_tx`]'s closure
//! is handed a transaction-scoped view (`db::sqlite::TxStore`) that borrows a
//! `&Connection` directly and is neither `Send` nor `Sync` (rusqlite's
//! `Connection` itself is not `Sync`), so a blanket bound here would make
//! that view impossible to implement `Store` for. Long-lived, cross-thread
//! store handles add the bound at their own type instead — see
//! `AppState::store: Arc<dyn Store + Send + Sync>` and
//! `RescheduleTrigger::store: Arc<dyn Store + Send + Sync>` — which is where
//! the requirement actually belongs.

use chrono::{DateTime, Utc};

use crate::domain::inputs::{AgendaItem, LabelCount, TaskFilter};
use crate::domain::models::{
    AppConfig, Chunk, ChunkSyncState, Comment, ExternalEventRecord, GoogleAuthState,
    RecurringTemplate, Schedule, Task,
};
use crate::error::AppError;

/// Storage operations for [`Task`] entities.
///
/// All CRUD methods persist/read the `task_labels` join table alongside the
/// task. Labels on the [`Task`] model are denormalized from the join table on
/// read, and written to the join table on create/update.
pub trait TaskStore {
    /// Persist a new task and its labels.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn create_task(&self, task: &Task) -> Result<(), AppError>;

    /// Look up a task by ID, returning `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_task(&self, id: &str) -> Result<Option<Task>, AppError>;

    /// Overwrite an existing task and replace its labels.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn update_task(&self, task: &Task) -> Result<(), AppError>;

    /// Delete a task and its labels by ID.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_task(&self, id: &str) -> Result<(), AppError>;

    /// Query tasks matching the given filter criteria.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, AppError>;

    /// Returns tasks eligible for scheduling: status IN (Pending, Scheduled)
    /// AND `time_logged_minutes < duration_minutes` (remaining time > 0).
    /// Excludes Backlog, Completed, and Cancelled tasks.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_schedulable_tasks(&self) -> Result<Vec<Task>, AppError>;
}

/// Storage operations for [`Chunk`] entities.
pub trait ChunkStore {
    /// Persist a new chunk.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn create_chunk(&self, chunk: &Chunk) -> Result<(), AppError>;

    /// Look up a chunk by ID, returning `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_chunk(&self, id: &str) -> Result<Option<Chunk>, AppError>;

    /// Overwrite an existing chunk.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn update_chunk(&self, chunk: &Chunk) -> Result<(), AppError>;

    /// Delete a chunk by ID.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_chunk(&self, id: &str) -> Result<(), AppError>;

    /// Return all chunks belonging to the given task.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_chunks_for_task(&self, task_id: &str) -> Result<Vec<Chunk>, AppError>;

    /// Return all chunks whose time range overlaps `[start, end)`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_chunks_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Chunk>, AppError>;

    /// Return [`AgendaItem`]s (chunks enriched with task title, priority, and
    /// labels) whose time range overlaps `[start, end)`.
    ///
    /// Joins each chunk to its task in one query. A chunk whose task is absent
    /// is excluded by the inner join — a state the schema forbids (`task_id` is
    /// `NOT NULL` with `ON DELETE CASCADE`), so the join makes it structurally
    /// unrepresentable rather than something callers must guard.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_agenda_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<AgendaItem>, AppError>;

    /// Non-fixed, non-completed chunks (movable during reschedule).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_auto_chunks(&self) -> Result<Vec<Chunk>, AppError>;

    /// All fixed or completed chunks (immovable during reschedule).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_all_fixed_and_completed(&self) -> Result<Vec<Chunk>, AppError>;

    /// All chunks that are both fixed (`is_fixed = true`) and `Scheduled`.
    ///
    /// Excludes completed chunks — completed chunks must never be unlocked.
    /// Used by the stale-lock release pass to find candidates for unlocking.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_fixed_scheduled_chunks(&self) -> Result<Vec<Chunk>, AppError>;

    /// Return all chunks with `status = Scheduled` and `end_time < cutoff`.
    ///
    /// Used by the past-due detection logic to find chunks that were scheduled
    /// but whose end time has already passed (implying they were never completed).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_past_due_scheduled_chunks(&self, cutoff: DateTime<Utc>) -> Result<Vec<Chunk>, AppError>;
}

/// Storage operations for [`Schedule`] entities.
pub trait ScheduleStore {
    /// Persist a new schedule.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn create_schedule(&self, schedule: &Schedule) -> Result<(), AppError>;

    /// Look up a schedule by ID, returning `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, AppError>;

    /// Return the schedule marked as default.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no default schedule exists.
    /// Returns [`AppError::Database`] on storage failure.
    fn get_default_schedule(&self) -> Result<Schedule, AppError>;

    /// Overwrite an existing schedule.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn update_schedule(&self, schedule: &Schedule) -> Result<(), AppError>;

    /// Delete a schedule by ID.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_schedule(&self, id: &str) -> Result<(), AppError>;

    /// Return all schedules.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn list_schedules(&self) -> Result<Vec<Schedule>, AppError>;
}

/// Storage operations for [`RecurringTemplate`] entities.
///
/// All CRUD methods persist/read the `template_labels` join table alongside
/// the template. Labels are denormalized from the join table on read, and
/// written to the join table on create/update (same pattern as [`TaskStore`]).
pub trait RecurringTemplateStore {
    /// Persist a new template and its labels.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn create_template(&self, template: &RecurringTemplate) -> Result<(), AppError>;

    /// Look up a template by ID, returning `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_template(&self, id: &str) -> Result<Option<RecurringTemplate>, AppError>;

    /// Overwrite an existing template and replace its labels.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn update_template(&self, template: &RecurringTemplate) -> Result<(), AppError>;

    /// Delete a template and its labels by ID.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_template(&self, id: &str) -> Result<(), AppError>;

    /// Return all templates.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn list_templates(&self) -> Result<Vec<RecurringTemplate>, AppError>;
}

/// Read operations over the label join tables.
///
/// Labels are not an aggregate root — they live denormalized in the
/// `task_labels` and `template_labels` join tables owned by [`TaskStore`] and
/// [`RecurringTemplateStore`]. This trait covers the cross-entity reads that
/// belong to neither owner.
pub trait LabelStore {
    /// Return every distinct label with its task usage count, ordered by
    /// label.
    ///
    /// Labels are unioned across `task_labels` and `template_labels`;
    /// `task_count` counts tasks only (`0` for template-only labels).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn list_labels(&self) -> Result<Vec<LabelCount>, AppError>;
}

/// Read/update operations for the singleton [`AppConfig`].
pub trait ConfigStore {
    /// Return the current application configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_config(&self) -> Result<AppConfig, AppError>;

    /// Overwrite the application configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn update_config(&self, config: &AppConfig) -> Result<(), AppError>;

    /// Read a single raw config value by key. Unknown key → `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_config_value(&self, key: &str) -> Result<Option<String>, AppError>;

    /// Insert or overwrite a single raw config value.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn set_config_value(&self, key: &str, value: &str) -> Result<(), AppError>;
}

/// Storage for locally mirrored external calendar events.
pub trait ExternalEventStore {
    /// Refresh one calendar's mirror inside `[window_start, window_end)`:
    /// rows of `calendar_id` overlapping the window whose `event_id` is NOT
    /// in `events` are deleted (remote deletion), every record in `events`
    /// is upserted by `event_id` (original row id preserved on update).
    /// Rows outside the window and rows of other calendars are untouched.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn replace_external_events_in_window(
        &self,
        calendar_id: &str,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        events: &[ExternalEventRecord],
    ) -> Result<(), AppError>;

    /// Return mirrored events overlapping `[start, end)` across all calendars.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_external_events_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEventRecord>, AppError>;

    /// Delete the entire mirror (provider disconnect wipes local state only).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn clear_all_external_events(&self) -> Result<(), AppError>;

    /// Delete ALL mirrored rows for `calendar_id` regardless of time window.
    ///
    /// Used when a calendar is removed from the pull selection.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_external_events_for_calendar(&self, calendar_id: &str) -> Result<(), AppError>;

    /// Return the distinct `calendar_id` values present in `external_events`.
    ///
    /// Used to identify calendars that still have mirrored rows so that rows
    /// for deselected calendars can be cleaned up on the next pull.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_mirrored_calendar_ids(&self) -> Result<Vec<String>, AppError>;

    /// Insert or update a single mirror row, keyed by `(calendar_id, event_id)`.
    ///
    /// Used to echo an in-app user-event write (G11) into the local mirror
    /// immediately, before the next full pull confirms it. On conflict the
    /// existing row id is preserved (same shape as the bulk window upsert); the
    /// caller's `id` is only used when the row is newly inserted.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn upsert_external_event(&self, event: &ExternalEventRecord) -> Result<(), AppError>;

    /// Return the single mirror row for `(calendar_id, event_id)`, or `None`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_external_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<Option<ExternalEventRecord>, AppError>;

    /// Delete the single mirror row for `(calendar_id, event_id)`.
    ///
    /// Idempotent: deleting an absent row is a success.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_external_event(&self, calendar_id: &str, event_id: &str) -> Result<(), AppError>;
}

/// Storage operations for [`Comment`] entities (M12).
pub trait CommentStore {
    /// Persist a new comment.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure (including a
    /// foreign-key violation when the referenced task does not exist).
    fn create_comment(&self, comment: &Comment) -> Result<(), AppError>;

    /// Look up a comment by ID, returning `None` if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_comment(&self, id: &str) -> Result<Option<Comment>, AppError>;

    /// Persist a comment edit. Only the editable fields (`content`,
    /// `updated_at`) are written; `task_id`, `author`, and `created_at` are
    /// immutable after creation (M12.3).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no row matches the comment's id.
    /// Returns [`AppError::Database`] on storage failure.
    fn update_comment(&self, comment: &Comment) -> Result<(), AppError>;

    /// Delete a comment by ID.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_comment(&self, id: &str) -> Result<(), AppError>;

    /// Return all comments for the given task, newest first (M12.4).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn list_comments_for_task(&self, task_id: &str) -> Result<Vec<Comment>, AppError>;
}

/// Read/update the singleton provider link row (`google_auth`).
pub trait GoogleAuthStore {
    /// Return the link state, or `None` if no row exists (never connected).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_google_auth(&self) -> Result<Option<GoogleAuthState>, AppError>;

    /// Insert or overwrite the singleton link row.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn set_google_auth(&self, auth: &GoogleAuthState) -> Result<(), AppError>;

    /// Delete the link row (disconnect).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn clear_google_auth(&self) -> Result<(), AppError>;
}

/// Storage for per-chunk sync bases (three-way-merge state, push phase).
pub trait ChunkSyncStateStore {
    /// Return sync-base rows whose `synced_start < end` AND `synced_end > start`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn get_chunk_sync_states_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ChunkSyncState>, AppError>;

    /// Insert or update a sync-base row (conflict key: `chunk_id`).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn upsert_chunk_sync_state(&self, state: &ChunkSyncState) -> Result<(), AppError>;

    /// Delete the sync-base row for a chunk.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn delete_chunk_sync_state(&self, chunk_id: &str) -> Result<(), AppError>;

    /// Delete all sync-base rows (provider disconnect).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure.
    fn clear_all_chunk_sync_state(&self) -> Result<(), AppError>;
}

/// Combined storage trait — a single backend implements all sub-traits.
pub trait Store:
    TaskStore
    + ChunkStore
    + ScheduleStore
    + RecurringTemplateStore
    + LabelStore
    + ConfigStore
    + CommentStore
    + ExternalEventStore
    + GoogleAuthStore
    + ChunkSyncStateStore
{
    /// Run `f` inside a single storage transaction, committing on `Ok` and
    /// rolling back on `Err`.
    ///
    /// `f` receives a `&dyn Store` view scoped to the open transaction. **Call
    /// every store method through that parameter, never through the `&self`
    /// this method was invoked on.** The concrete backend serializes access
    /// behind a non-reentrant lock (e.g. `Mutex<Connection>`); calling back
    /// into the outer `self` from inside `f` re-acquires a lock already held
    /// by this call and deadlocks. Nested `with_tx` calls made through the
    /// closure's own store parameter are safe — they join the already-open
    /// transaction rather than starting a new one.
    ///
    /// # Errors
    ///
    /// Returns whatever error `f` returns (after rolling back), or an
    /// [`AppError`] if the transaction itself cannot be started, committed,
    /// or rolled back.
    fn with_tx(
        &self,
        f: &mut dyn FnMut(&dyn Store) -> Result<(), AppError>,
    ) -> Result<(), AppError>;

    /// Write a consistent single-file snapshot of the whole database to
    /// `dest` (backup export, M11). `dest` must not already exist.
    ///
    /// Default implementation refuses: snapshots only make sense on a
    /// top-level backend (`SQLite`'s `VACUUM INTO` cannot run inside the open
    /// transaction a `with_tx` view represents).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] on storage failure or when the backend
    /// does not support snapshots.
    fn vacuum_into(&self, dest: &std::path::Path) -> Result<(), AppError> {
        let _ = dest;
        Err(AppError::Database(
            "database snapshot is not supported by this store".into(),
        ))
    }
}
