// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for scheduling operations.

#![allow(clippy::needless_pass_by_value)]

use chrono::Utc;

use crate::error::AppError;
use crate::state::ActiveState;
use crate::traits::scheduling::ScheduleResult;

/// Reschedules all pending/scheduled tasks (full pass).
///
/// # Errors
///
/// Returns [`AppError`] if the scheduling service or storage layer fails.
#[tauri::command]
pub fn trigger_reschedule(
    active: tauri::State<'_, ActiveState>,
) -> Result<ScheduleResult, AppError> {
    let state = active.get()?;
    // Direct pipeline run (bypasses the trigger), so the guard is held for
    // the whole reschedule, mirroring RescheduleTrigger::execute.
    let _guard = state.trigger.mutation_guard()?;
    crate::services::scheduling::reschedule(
        state.store.as_ref(),
        state.scheduler.as_ref(),
        Utc::now(),
    )
}

/// Reschedules specific tasks incrementally.
///
/// # Errors
///
/// Returns [`AppError`] if the scheduling service or storage layer fails.
#[tauri::command]
pub fn trigger_reschedule_incremental(
    active: tauri::State<'_, ActiveState>,
    task_ids: Vec<String>,
) -> Result<ScheduleResult, AppError> {
    let state = active.get()?;
    let _guard = state.trigger.mutation_guard()?;
    crate::services::scheduling::reschedule_incremental(
        state.store.as_ref(),
        state.scheduler.as_ref(),
        &task_ids,
        Utc::now(),
    )
}
