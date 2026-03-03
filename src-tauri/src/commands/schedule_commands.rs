// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for schedule operations.
//!
//! Each function resolves the active profile's state from [`ActiveState`],
//! delegates to the corresponding service function, and returns the result.
//! No business logic lives here.

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use chrono::Utc;

use crate::domain::inputs::{CreateScheduleInput, UpdateScheduleInput};
use crate::domain::models::Schedule;
use crate::error::AppError;
use crate::services::trigger::Mutation;
use crate::state::ActiveState;

/// Create a new schedule.
///
/// Triggers an immediate full reschedule since the available slot windows have
/// changed.
///
/// # Errors
///
/// Propagates any error from [`crate::services::schedule::create_schedule`].
#[tauri::command]
pub fn create_schedule(
    active: tauri::State<'_, ActiveState>,
    input: CreateScheduleInput,
) -> Result<Schedule, AppError> {
    let state = active.get()?;
    let schedule = {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::schedule::create_schedule(state.store.as_ref(), input, Utc::now())?
    };
    state.trigger.trigger_mutation(Mutation::ScheduleCreated)?;
    Ok(schedule)
}

/// Retrieve a schedule by ID.
///
/// # Errors
///
/// Propagates any error from [`crate::services::schedule::get_schedule`].
#[tauri::command]
pub fn get_schedule(
    active: tauri::State<'_, ActiveState>,
    id: String,
) -> Result<Schedule, AppError> {
    let state = active.get()?;
    crate::services::schedule::get_schedule(state.store.as_ref(), &id)
}

/// Apply a partial update to an existing schedule.
///
/// Triggers an immediate full reschedule since the available slot windows may
/// have changed.
///
/// # Errors
///
/// Propagates any error from [`crate::services::schedule::update_schedule`].
#[tauri::command]
pub fn update_schedule(
    active: tauri::State<'_, ActiveState>,
    id: String,
    input: UpdateScheduleInput,
) -> Result<Schedule, AppError> {
    let state = active.get()?;
    let schedule = {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::schedule::update_schedule(state.store.as_ref(), &id, input, Utc::now())?
    };
    state.trigger.trigger_mutation(Mutation::ScheduleUpdated)?;
    Ok(schedule)
}

/// Delete a schedule by ID.
///
/// Triggers an immediate full reschedule since the available slot windows have
/// changed.
///
/// # Errors
///
/// Propagates any error from [`crate::services::schedule::delete_schedule`].
#[tauri::command]
pub fn delete_schedule(active: tauri::State<'_, ActiveState>, id: String) -> Result<(), AppError> {
    let state = active.get()?;
    {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::schedule::delete_schedule(state.store.as_ref(), &id, Utc::now())?;
    }
    state.trigger.trigger_mutation(Mutation::ScheduleDeleted)?;
    Ok(())
}

/// List all schedules.
///
/// # Errors
///
/// Propagates any error from [`crate::services::schedule::list_schedules`].
#[tauri::command]
pub fn list_schedules(active: tauri::State<'_, ActiveState>) -> Result<Vec<Schedule>, AppError> {
    let state = active.get()?;
    crate::services::schedule::list_schedules(state.store.as_ref())
}
