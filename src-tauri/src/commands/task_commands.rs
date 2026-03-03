// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for task operations.
//!
//! Each function resolves the active profile's state from [`ActiveState`],
//! delegates to the corresponding service function, and returns the result.
//! No business logic lives here.

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use chrono::Utc;

use crate::domain::enums::TaskStatus;
use crate::domain::inputs::{CreateTaskInput, LabelCount, TaskFilter, UpdateTaskInput};
use crate::domain::models::Task;
use crate::error::AppError;
use crate::services::trigger::Mutation;
use crate::state::{ActiveState, AppState};

/// Create a new task, guarded by the reschedule trigger's mutation lock.
///
/// Shared by the `create_task` Tauri command and the REST `POST /api/tasks`
/// handler — both report the same [`Mutation::TaskCreated`].
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::create_task`].
pub fn create_task_guarded(state: &AppState, input: CreateTaskInput) -> Result<Task, AppError> {
    state.trigger.run_guarded(
        || crate::services::task::create_task(state.store.as_ref(), input, Utc::now()),
        |task| Mutation::TaskCreated {
            task_id: task.id.clone(),
        },
    )
}

/// Create a new task.
///
/// Triggers a debounced incremental reschedule for the new task.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::create_task`].
#[tauri::command]
pub fn create_task(
    active: tauri::State<'_, ActiveState>,
    input: CreateTaskInput,
) -> Result<Task, AppError> {
    create_task_guarded(active.get()?.as_ref(), input)
}

/// # Errors
///
/// Propagates any error from [`crate::services::task::get_task`].
#[tauri::command]
pub fn get_task(active: tauri::State<'_, ActiveState>, id: String) -> Result<Task, AppError> {
    let state = active.get()?;
    crate::services::task::get_task(state.store.as_ref(), &id)
}

/// Apply a partial update to an existing task, guarded by the reschedule
/// trigger's mutation lock.
///
/// Shared by the `update_task` Tauri command and the REST
/// `PATCH /api/tasks/:id` handler — both report the same
/// [`Mutation::TaskUpdated`]. A transition to `Backlog` frees the task's
/// auto-scheduled slots (see [`crate::services::task::update_task`]); the
/// trigger policy maps that to a full reschedule so other tasks can claim
/// them. Backlog→Backlog is rejected by the service's allowed-transition
/// matrix, so `to_backlog` only fires on a real transition.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::update_task`].
pub fn update_task_guarded(
    state: &AppState,
    id: &str,
    input: UpdateTaskInput,
) -> Result<Task, AppError> {
    let to_backlog = matches!(input.status, Some(TaskStatus::Backlog));
    state.trigger.run_guarded(
        || crate::services::task::update_task(state.store.as_ref(), id, input, Utc::now()),
        |task| Mutation::TaskUpdated {
            task_id: task.id.clone(),
            to_backlog,
        },
    )
}

/// Apply a partial update to an existing task. See
/// [`update_task_guarded`] for the reschedule-trigger behavior.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::update_task`].
#[tauri::command]
pub fn update_task(
    active: tauri::State<'_, ActiveState>,
    id: String,
    input: UpdateTaskInput,
) -> Result<Task, AppError> {
    update_task_guarded(active.get()?.as_ref(), &id, input)
}

/// Delete a task by ID, guarded by the reschedule trigger's mutation lock.
///
/// Shared by the `delete_task` Tauri command and the REST
/// `DELETE /api/tasks/:id` handler — both report [`Mutation::TaskDeleted`]
/// and the shared trigger policy runs a full reschedule so freed slots can
/// be reallocated.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::delete_task`].
pub fn delete_task_guarded(state: &AppState, id: &str) -> Result<(), AppError> {
    state.trigger.run_guarded(
        || crate::services::task::delete_task(state.store.as_ref(), id, Utc::now()),
        |()| Mutation::TaskDeleted,
    )
}

/// Delete a task by ID. See [`delete_task_guarded`] for the
/// reschedule-trigger behavior.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::delete_task`].
#[tauri::command]
pub fn delete_task(active: tauri::State<'_, ActiveState>, id: String) -> Result<(), AppError> {
    delete_task_guarded(active.get()?.as_ref(), &id)
}

/// Cancel a task, triggering a debounced full reschedule so freed slots can be reallocated.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::cancel_task`].
#[tauri::command]
pub fn cancel_task(active: tauri::State<'_, ActiveState>, id: String) -> Result<Task, AppError> {
    let state = active.get()?;
    state.trigger.run_guarded(
        || crate::services::task::cancel_task(state.store.as_ref(), &id, Utc::now()),
        |_task| Mutation::TaskCancelled,
    )
}

/// Complete a task by completing all of its scheduled chunks, guarded by the
/// reschedule trigger's mutation lock.
///
/// Shared by the `complete_task` Tauri command and the REST
/// `POST /api/tasks/:id/complete` handler — both report the same
/// [`Mutation::TaskCompleted`] and trigger an immediate full reschedule
/// so waiting tasks can reclaim the freed slots.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::complete_task`].
pub fn complete_task_guarded(state: &AppState, id: &str) -> Result<Task, AppError> {
    state.trigger.run_guarded(
        || crate::services::task::complete_task(state.store.as_ref(), id, Utc::now()),
        |task| Mutation::TaskCompleted {
            task_id: task.id.clone(),
        },
    )
}

/// Complete a task by completing all of its scheduled chunks. See
/// [`complete_task_guarded`] for the reschedule-trigger behavior.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::complete_task`].
#[tauri::command]
pub fn complete_task(active: tauri::State<'_, ActiveState>, id: String) -> Result<Task, AppError> {
    complete_task_guarded(active.get()?.as_ref(), &id)
}

/// When `filter` is `None`, returns all tasks (uses default empty filter).
///
/// # Errors
///
/// Propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn list_tasks(
    active: tauri::State<'_, ActiveState>,
    filter: Option<TaskFilter>,
) -> Result<Vec<Task>, AppError> {
    let state = active.get()?;
    let filter = filter.unwrap_or_default();
    state.store.list_tasks(&filter)
}

/// List every distinct label with its task usage count.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::list_labels`].
#[tauri::command]
pub fn list_labels(active: tauri::State<'_, ActiveState>) -> Result<Vec<LabelCount>, AppError> {
    let state = active.get()?;
    crate::services::task::list_labels(state.store.as_ref())
}

/// Return virtual tasks for active recurring templates that have no pending or scheduled instances.
///
/// # Errors
///
/// Propagates any error from
/// [`crate::services::recurring::get_orphaned_template_instances`].
#[tauri::command]
pub fn get_orphaned_template_instances(
    active: tauri::State<'_, ActiveState>,
) -> Result<Vec<Task>, AppError> {
    let state = active.get()?;
    crate::services::recurring::get_orphaned_template_instances(state.store.as_ref(), Utc::now())
}
