// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Input DTOs for creating/updating domain entities.
//!
//! These are pure data containers — validation logic lives in
//! [`super::validation`].

use chrono::{DateTime, NaiveTime, Utc, Weekday};
use serde::{Deserialize, Serialize};

use super::cadence::Cadence;
use super::enums::{Priority, TaskStatus};
use super::models::{Chunk, EntityId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    pub duration_minutes: i64,
    /// Defaults to `Medium` when `None`.
    pub priority: Option<Priority>,
    pub start_date: Option<DateTime<Utc>>,
    pub deadline: DateTime<Utc>,
    // jscpd:ignore-start — these fields also appear in UpdateTaskInput; the types
    // are semantically distinct (Option<T> here = "optional with default", not "no change").
    /// Defaults to the default schedule when `None`.
    pub schedule_id: Option<EntityId>,
    /// Defaults to 30 when `None`.
    pub min_chunk_minutes: Option<i64>,
    /// Defaults to `false` when `None`.
    pub no_split: Option<bool>,
    /// Defaults to `[]` when `None`.
    pub labels: Option<Vec<String>>,
    /// Defaults to `Pending` when `None`.
    pub status: Option<TaskStatus>,
    // jscpd:ignore-end
}

/// Uses `Option<Option<T>>` for nullable fields: `None` = don't change,
/// `Some(None)` = clear the field, `Some(Some(v))` = set to `v`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTaskInput {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub duration_minutes: Option<i64>,
    pub priority: Option<Priority>,
    pub start_date: Option<Option<DateTime<Utc>>>,
    pub deadline: Option<DateTime<Utc>>,
    pub schedule_id: Option<EntityId>,
    pub min_chunk_minutes: Option<i64>,
    pub no_split: Option<bool>,
    /// Replaces all labels when `Some`.
    pub labels: Option<Vec<String>>,
    /// For backlog <-> pending transitions.
    pub status: Option<TaskStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTemplateInput {
    pub title: String,
    pub description: Option<String>,
    pub duration_minutes: i64,
    /// Defaults to `Medium` when `None`.
    pub priority: Option<Priority>,
    /// Defaults to the default schedule when `None`.
    pub schedule_id: Option<EntityId>,
    pub cadence: Cadence,
    /// Defaults to `[]` when `None`.
    pub labels: Option<Vec<String>>,
    /// Defaults to the current time (injected) when `None`.
    pub start_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateTemplateInput {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub duration_minutes: Option<i64>,
    pub priority: Option<Priority>,
    pub schedule_id: Option<EntityId>,
    pub cadence: Option<Cadence>,
    pub labels: Option<Vec<String>>,
    pub is_active: Option<bool>,
    pub start_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduleInput {
    pub name: String,
    pub windows: Vec<ScheduleWindowInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateScheduleInput {
    pub name: Option<String>,
    /// Replaces all windows when `Some`.
    pub windows: Option<Vec<ScheduleWindowInput>>,
}

/// A single time window within a schedule, without an ID.
///
/// Validation (in [`super::validation`]):
/// - `start_time < end_time` (overnight windows not supported; model as two).
/// - No overlapping windows on the same `day_of_week` within a schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleWindowInput {
    pub day_of_week: Weekday,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
}

/// Patch input for updating the global [`super::models::AppConfig`].
///
/// Internal timestamps (`last_reschedule`, `last_mutation`, `last_sync`,
/// `last_busy_sync`) are not user-editable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfigInput {
    pub planning_horizon_days: Option<i64>,
    pub timezone: Option<String>,
    pub max_continuous_minutes: Option<i64>,
    pub min_break_minutes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCommentInput {
    pub task_id: EntityId,
    /// Markdown content; must be non-empty after trimming.
    pub content: String,
    /// Defaults to `"User"` when `None`. `"SYSTEM"` is reserved for
    /// auto-generated comments and is rejected here.
    pub author: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateCommentInput {
    pub content: Option<String>,
}

/// A [`Chunk`] enriched with denormalized task metadata for agenda display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgendaItem {
    pub chunk: Chunk,
    pub task_title: String,
    pub task_priority: Priority,
    pub task_labels: Vec<String>,
    /// Template id when the task is a recurring instance — lets calendar
    /// surfaces offer "Edit template" without an extra task fetch.
    pub task_recurring_template_id: Option<String>,
    /// Task deadline — lets the calendar mark chunks scheduled past it.
    pub task_deadline: Option<DateTime<Utc>>,
}

/// A distinct label with the number of tasks currently carrying it.
///
/// Labels are unioned across tasks and recurring templates so template-only
/// labels stay visible in suggestion UIs; `task_count` stays task-centric
/// (`0` for template-only labels) so task-list filter chips can show it
/// directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelCount {
    pub label: String,
    pub task_count: i64,
}

/// Filter criteria for querying [`super::models::Task`]s.
///
/// Fields are combined with AND. `labels` is match-all: a task matches only
/// if it carries *every* listed label. `excluded_labels` is match-none: a
/// task carrying *any* listed label is dropped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFilter {
    /// Case-insensitive substring match on title + description.
    pub search_text: Option<String>,
    pub statuses: Option<Vec<TaskStatus>>,
    pub labels: Option<Vec<String>>,
    /// Match-none semantics: the task must carry none of the listed labels.
    pub excluded_labels: Option<Vec<String>>,
    /// `Some(true)` keeps only tasks with no labels at all; `Some(false)`
    /// keeps only labeled tasks; `None` leaves label presence unconstrained.
    pub unlabeled: Option<bool>,
    /// Match-any (IN) semantics, like `statuses`; empty means unconstrained.
    pub priorities: Option<Vec<Priority>>,
    pub deadline_before: Option<DateTime<Utc>>,
    pub deadline_after: Option<DateTime<Utc>>,
    pub schedule_id: Option<EntityId>,
    pub recurring_template_id: Option<EntityId>,
}

#[cfg(test)]
impl CreateTaskInput {
    pub(crate) fn test_default() -> Self {
        Self {
            title: "Test task".to_owned(),
            description: None,
            duration_minutes: 60,
            priority: None,
            start_date: None,
            deadline: crate::test_support::fixture_base(),
            schedule_id: None,
            min_chunk_minutes: None,
            no_split: None,
            labels: None,
            status: None,
        }
    }
}

#[cfg(test)]
impl CreateTemplateInput {
    pub(crate) fn test_default() -> Self {
        Self {
            title: "Test template".to_owned(),
            description: None,
            duration_minutes: 60,
            priority: None,
            schedule_id: None,
            cadence: Cadence::weekly(vec![chrono::Weekday::Mon]),
            labels: None,
            start_date: None,
        }
    }
}
