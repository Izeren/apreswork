// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Task CRUD — create, read, list, update, delete.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::enums::{ChunkStatus, Priority, TaskStatus};
use crate::domain::inputs::{CreateTaskInput, LabelCount, TaskFilter, UpdateTaskInput};
use crate::domain::models::Task;
use crate::domain::validation::{
    validate_create_task, validate_task_dates, validate_task_fits_schedule, validate_update_task,
};
use crate::error::AppError;
use crate::traits::storage::Store;

/// Create a new task, applying defaults and persisting it.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the input fails validation.
/// Returns [`AppError::NotFound`] if no default schedule exists and
/// `schedule_id` is `None`.
/// Returns [`AppError::Database`] on storage failure.
pub fn create_task(
    store: &dyn Store,
    input: CreateTaskInput,
    now: DateTime<Utc>,
) -> Result<Task, AppError> {
    validate_create_task(&input)?;

    let schedule_id = match input.schedule_id {
        Some(id) => id,
        None => store.get_default_schedule()?.id,
    };

    let min_chunk_minutes = input.min_chunk_minutes.unwrap_or(30);
    let no_split = input.no_split.unwrap_or(false) || input.duration_minutes <= min_chunk_minutes;

    // Validate schedule capacity. An explicit bogus schedule_id returns NotFound
    // (previously it propagated as a DB FK error from store.create_task).
    let schedule = crate::services::schedule::get_schedule(store, &schedule_id)?;
    validate_task_fits_schedule(
        input.duration_minutes,
        min_chunk_minutes,
        no_split,
        schedule.largest_window_minutes(),
        &schedule.name,
    )?;

    let task = Task {
        id: Uuid::now_v7().to_string(),
        title: input.title,
        description: input.description,
        duration_minutes: input.duration_minutes,
        time_logged_minutes: 0,
        priority: input.priority.unwrap_or(Priority::Medium),
        status: input.status.unwrap_or(TaskStatus::Pending),
        start_date: input.start_date,
        deadline: Some(input.deadline),
        schedule_id,
        min_chunk_minutes,
        no_split,
        recurring_template_id: None,
        expire_at: None,
        is_pinned: false,
        labels: input.labels.unwrap_or_default(),
        created_at: now,
        updated_at: now,
    };

    store.create_task(&task)?;
    Ok(task)
}

/// Retrieve a task by ID.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if no task with the given ID exists.
/// Returns [`AppError::Database`] on storage failure.
pub fn get_task(store: &dyn Store, id: &str) -> Result<Task, AppError> {
    store.get_task(id)?.ok_or_else(|| AppError::NotFound {
        entity: "Task".into(),
        id: id.to_owned(),
    })
}

/// List tasks matching the given filter criteria.
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn list_tasks(store: &dyn Store, filter: &TaskFilter) -> Result<Vec<Task>, AppError> {
    store.list_tasks(filter)
}

/// List every distinct label with its task usage count, ordered by label.
///
/// Labels are unioned across tasks and recurring templates; `task_count`
/// counts tasks only (`0` for template-only labels).
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn list_labels(store: &dyn Store) -> Result<Vec<LabelCount>, AppError> {
    store.list_labels()
}

/// Apply a partial update to an existing task.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the input fails validation.
/// Returns [`AppError::NotFound`] if the task does not exist.
/// Returns [`AppError::Validation`] if `duration_minutes` would be less than
/// `time_logged_minutes`.
/// Returns [`AppError::Database`] on storage failure.
// The function length is driven by the sequential status-transition, chunk-cleanup,
// and capacity-validation steps — each is a distinct policy that belongs here.
#[allow(clippy::too_many_lines)]
pub fn update_task(
    store: &dyn Store,
    task_id: &str,
    input: UpdateTaskInput,
    now: DateTime<Utc>,
) -> Result<Task, AppError> {
    validate_update_task(&input)?;

    let mut task = get_task(store, task_id)?;

    // Recurring instances are anchored to their template's cadence. The window
    // anchor (`start_date`) is immutable, and a manual deadline override must stay
    // inside the instance's window `[start_date, expire_at]` so it cannot escape
    // into a neighbouring occurrence. The upper bound is best-effort: a legacy
    // instance with `expire_at == None` skips it, but the next reconcile refreshes
    // `expire_at` and auto-cancel then bounds the override.
    //
    // Note: for monthly cadences reconcile always refreshes the deadline from the
    // occurrence (the widened schedulable span is authoritative). A monthly deadline
    // override is valid until the next reconcile, where it is reset to the period-aware
    // ceiling (28th for the last window in the period; day before the next window opens
    // for earlier windows).
    if task.recurring_template_id.is_some() {
        if let Some(new_start) = input.start_date {
            if new_start != task.start_date {
                return Err(AppError::Validation(
                    "start_date cannot be changed on a recurring task instance".into(),
                ));
            }
        }
        if let Some(new_deadline) = input.deadline {
            if task.start_date.is_some_and(|start| new_deadline < start) {
                return Err(AppError::Validation(
                    "deadline cannot be earlier than the recurring instance's start date".into(),
                ));
            }
            if task.expire_at.is_some_and(|expire| new_deadline > expire) {
                return Err(AppError::Validation(
                    "deadline cannot be later than the recurring instance's expiry".into(),
                ));
            }
        }
    }

    if let Some(d) = input.duration_minutes {
        if d < task.time_logged_minutes {
            return Err(AppError::Validation(
                "duration_minutes cannot be less than time_logged_minutes".into(),
            ));
        }
    }

    // TODO(1.8.1): set config.last_mutation on title change
    if let Some(v) = input.title {
        task.title = v;
    }
    if let Some(v) = input.description {
        task.description = v;
    }
    if let Some(v) = input.duration_minutes {
        task.duration_minutes = v;
    }
    if let Some(v) = input.priority {
        task.priority = v;
    }
    if let Some(v) = input.start_date {
        task.start_date = v;
    }
    if let Some(v) = input.deadline {
        task.deadline = Some(v);
    }
    if let Some(v) = input.schedule_id {
        task.schedule_id = v;
    }
    if let Some(v) = input.min_chunk_minutes {
        task.min_chunk_minutes = v;
    }
    if let Some(v) = input.no_split {
        task.no_split = v;
    }
    if let Some(v) = input.labels {
        task.labels = v;
    }
    let mut chunks_to_delete: Vec<String> = Vec::new();
    if let Some(new_status) = input.status {
        let allowed = matches!(
            (task.status, new_status),
            (TaskStatus::Backlog, TaskStatus::Pending)
                | (
                    TaskStatus::Pending | TaskStatus::Scheduled,
                    TaskStatus::Backlog
                )
        );
        if !allowed {
            return Err(AppError::Validation(format!(
                "status transition from {:?} to {:?} is not allowed via update_task",
                task.status, new_status
            )));
        }

        // Leaving Scheduled for Backlog must give up the task's auto-scheduled
        // slots: the incremental reschedule this transition triggers excludes
        // Backlog tasks (`get_schedulable_tasks`), so ghost chunks would
        // otherwise linger on the calendar until the next full reschedule.
        // Fixed (user-pinned) chunks and completed chunks (history) are kept —
        // unlike `cancel_task` (lifecycle.rs), which drops all scheduled
        // chunks regardless of `is_fixed`.
        if new_status == TaskStatus::Backlog {
            chunks_to_delete = store
                .get_chunks_for_task(task_id)?
                .into_iter()
                .filter(|chunk| chunk.status == ChunkStatus::Scheduled && !chunk.is_fixed)
                .map(|chunk| chunk.id)
                .collect();
        }

        task.status = new_status;
    }

    // Resolve the (possibly reassigned) schedule unconditionally: a bogus
    // schedule_id must surface as NotFound rather than a DB FK error, even
    // when the task is terminal.
    let schedule = crate::services::schedule::get_schedule(store, &task.schedule_id)?;

    // Capacity and date validation: skip for terminal tasks so cosmetic edits
    // to history records never trip these checks (status is already final).
    let is_terminal = matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled);
    if !is_terminal {
        validate_task_dates(task.start_date, task.deadline)?;
        validate_task_fits_schedule(
            task.duration_minutes,
            task.min_chunk_minutes,
            task.no_split,
            schedule.largest_window_minutes(),
            &schedule.name,
        )?;
    }

    super::stamp_and_persist(store, &mut task, &chunks_to_delete, now)?;
    Ok(task)
}

/// Delete a task, or cancel it if it belongs to a recurring template.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the task does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn delete_task(store: &dyn Store, task_id: &str, now: DateTime<Utc>) -> Result<(), AppError> {
    let mut task = get_task(store, task_id)?;

    // TODO(1.8.1): set config.last_mutation on delete
    if task.recurring_template_id.is_some() {
        task.status = TaskStatus::Cancelled;
        task.updated_at = now;
        store.update_task(&task)?;
    } else {
        store.delete_task(task_id)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
