// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for chunk operations and the external-event
//! mirror query.
//!
//! Each function resolves the active profile's state from [`ActiveState`],
//! delegates to the corresponding service or store function, and returns
//! the result. No business logic lives here.

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use chrono::{DateTime, Utc};

use crate::domain::inputs::AgendaItem;
use crate::domain::models::{Chunk, ExternalEventRecord, Task};
use crate::error::AppError;
use crate::services::trigger::Mutation;
use crate::state::{ActiveState, AppState};
use crate::traits::storage::Store;

/// Parse an ISO 8601 datetime string into `DateTime<Utc>`.
fn parse_datetime(s: &str) -> Result<DateTime<Utc>, AppError> {
    s.parse::<DateTime<Utc>>()
        .map_err(|e| AppError::Validation(format!("invalid datetime: {e}")))
}

fn parse_datetime_range(
    start: &str,
    end: &str,
) -> Result<(DateTime<Utc>, DateTime<Utc>), AppError> {
    Ok((parse_datetime(start)?, parse_datetime(end)?))
}

/// Run `f` under `state`'s mutation guard, returning its result. Shared by
/// every mutating command below: the guard only needs to be held for the
/// service call itself.
fn with_mutation_guard<T>(
    state: &AppState,
    f: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = state.trigger.mutation_guard()?;
    f()
}

/// Resolve state, run `service_call` under the mutation guard, fire
/// `mutation` with the resulting chunk's task id, and return the chunk.
/// Shared by the single-chunk-id commands below that report a
/// task-id-only mutation.
fn mutate_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: &str,
    service_call: impl FnOnce(&dyn Store, &str, DateTime<Utc>) -> Result<Chunk, AppError>,
    mutation: impl FnOnce(String) -> Mutation,
) -> Result<Chunk, AppError> {
    let state = active.get()?;
    let now = Utc::now();
    let chunk = with_mutation_guard(&state, || service_call(state.store.as_ref(), chunk_id, now))?;
    state
        .trigger
        .trigger_mutation(mutation(chunk.task_id.clone()))?;
    Ok(chunk)
}

/// # Errors
///
/// Propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn list_chunks_for_task(
    active: tauri::State<'_, ActiveState>,
    task_id: String,
) -> Result<Vec<Chunk>, AppError> {
    let state = active.get()?;
    state.store.get_chunks_for_task(&task_id)
}

/// # Errors
///
/// Returns [`AppError::Validation`] if datetime strings are invalid, or
/// propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn list_chunks_in_range(
    active: tauri::State<'_, ActiveState>,
    start: String,
    end: String,
) -> Result<Vec<Chunk>, AppError> {
    let state = active.get()?;
    let (start, end) = parse_datetime_range(&start, &end)?;
    state.store.get_chunks_in_range(start, end)
}

/// Return agenda items (chunks enriched with task metadata) for a time range,
/// optionally filtered by label.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if datetime strings are invalid, or
/// propagates any error from [`crate::services::task::get_agenda`].
#[tauri::command]
pub fn get_agenda(
    active: tauri::State<'_, ActiveState>,
    start: String,
    end: String,
    label: Option<String>,
) -> Result<Vec<AgendaItem>, AppError> {
    let state = active.get()?;
    let (start, end) = parse_datetime_range(&start, &end)?;
    let label_vec = label.map(|l| vec![l]);
    let label_filter = label_vec.as_deref();
    crate::services::task::get_agenda(state.store.as_ref(), start, end, label_filter)
}

/// List locally mirrored external calendar events overlapping `[start, end)`.
///
/// Read-only mirror query for the calendar render — no network, no provider
/// involvement; safe while disconnected (returns whatever the mirror holds).
///
/// # Errors
///
/// Returns [`AppError::Validation`] if datetime strings are invalid, or
/// propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn list_external_events(
    active: tauri::State<'_, ActiveState>,
    start: String,
    end: String,
) -> Result<Vec<ExternalEventRecord>, AppError> {
    let state = active.get()?;
    let (start, end) = parse_datetime_range(&start, &end)?;
    state.store.get_external_events_in_range(start, end)
}

/// Create a fixed (manually-placed) chunk for a task.
///
/// Triggers an immediate full reschedule so any tasks displaced by the new
/// fixed chunk can be repositioned.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if datetime strings are invalid, or
/// propagates any error from [`crate::services::task::create_fixed_chunk`].
#[tauri::command]
pub fn create_fixed_chunk(
    active: tauri::State<'_, ActiveState>,
    task_id: String,
    start_time: String,
    end_time: String,
) -> Result<(Chunk, Task), AppError> {
    let state = active.get()?;
    let (start_time, end_time) = parse_datetime_range(&start_time, &end_time)?;
    let result = with_mutation_guard(&state, || {
        crate::services::task::create_fixed_chunk(
            state.store.as_ref(),
            &task_id,
            start_time,
            end_time,
            Utc::now(),
        )
    })?;
    state
        .trigger
        .trigger_mutation(Mutation::FixedChunkCreated)?;
    Ok(result)
}

/// Mark a chunk as completed, optionally overriding its duration.
///
/// Triggers an immediate incremental reschedule for the affected task.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::complete_chunk`].
#[tauri::command]
pub fn complete_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
    duration_override: Option<i64>,
) -> Result<(Chunk, Task), AppError> {
    let state = active.get()?;
    let result = with_mutation_guard(&state, || {
        crate::services::task::complete_chunk(
            state.store.as_ref(),
            &chunk_id,
            duration_override,
            Utc::now(),
        )
    })?;
    state.trigger.trigger_mutation(Mutation::ChunkCompleted {
        task_id: result.1.id.clone(),
    })?;
    Ok(result)
}

/// Triggers an immediate incremental reschedule for the affected task.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::reopen_chunk`].
#[tauri::command]
pub fn reopen_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
) -> Result<(Chunk, Task), AppError> {
    let state = active.get()?;
    let result = with_mutation_guard(&state, || {
        crate::services::task::reopen_chunk(state.store.as_ref(), &chunk_id, Utc::now())
    })?;
    state.trigger.trigger_mutation(Mutation::ChunkReopened {
        task_id: result.1.id.clone(),
    })?;
    Ok(result)
}

/// Triggers a debounced incremental reschedule for the affected task.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if datetime strings are invalid, or
/// propagates any error from [`crate::services::task::move_chunk`].
#[tauri::command]
pub fn move_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
    new_start: String,
    new_end: String,
) -> Result<Chunk, AppError> {
    let state = active.get()?;
    let (new_start, new_end) = parse_datetime_range(&new_start, &new_end)?;
    let now = Utc::now();
    let chunk = with_mutation_guard(&state, || {
        crate::services::task::move_chunk(state.store.as_ref(), &chunk_id, new_start, new_end, now)
    })?;
    state.trigger.trigger_mutation(Mutation::ChunkMoved {
        task_id: chunk.task_id.clone(),
    })?;
    Ok(chunk)
}

/// Triggers a debounced incremental reschedule for the affected task.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the datetime string is invalid, or
/// propagates any error from [`crate::services::task::resize_chunk`].
#[tauri::command]
pub fn resize_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
    new_end: String,
) -> Result<(Chunk, Task), AppError> {
    let state = active.get()?;
    let new_end = parse_datetime(&new_end)?;
    let now = Utc::now();
    let result = with_mutation_guard(&state, || {
        crate::services::task::resize_chunk(state.store.as_ref(), &chunk_id, new_end, now)
    })?;
    state.trigger.trigger_mutation(Mutation::ChunkResized {
        task_id: result.1.id.clone(),
    })?;
    Ok(result)
}

/// Triggers a debounced incremental reschedule for the affected task.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::lock_chunk`].
#[tauri::command]
pub fn lock_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
) -> Result<Chunk, AppError> {
    mutate_chunk(
        active,
        &chunk_id,
        crate::services::task::lock_chunk,
        |task_id| Mutation::ChunkLocked { task_id },
    )
}

/// Triggers a debounced incremental reschedule for the affected task.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::unlock_chunk`].
#[tauri::command]
pub fn unlock_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
) -> Result<Chunk, AppError> {
    mutate_chunk(
        active,
        &chunk_id,
        crate::services::task::unlock_chunk,
        |task_id| Mutation::ChunkUnlocked { task_id },
    )
}

/// Triggers a debounced incremental reschedule for the affected task.
///
/// # Errors
///
/// Propagates any error from [`crate::services::task::delete_fixed_chunk`].
#[tauri::command]
pub fn delete_fixed_chunk(
    active: tauri::State<'_, ActiveState>,
    chunk_id: String,
) -> Result<Chunk, AppError> {
    mutate_chunk(
        active,
        &chunk_id,
        crate::services::task::delete_fixed_chunk,
        |task_id| Mutation::FixedChunkDeleted { task_id },
    )
}
