// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for recurring template operations.
//!
//! Each function resolves the active profile's state from [`ActiveState`],
//! delegates to the corresponding service or store function, and returns
//! the result. No business logic lives here.

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use chrono::Utc;

use crate::domain::inputs::{CreateTemplateInput, UpdateTemplateInput};
use crate::domain::models::RecurringTemplate;
use crate::error::AppError;
use crate::services::trigger::Mutation;
use crate::state::ActiveState;

/// Create a new recurring template.
///
/// Triggers an immediate full reschedule so that `reconcile` generates the
/// first batch of task instances for the new template.
///
/// # Errors
///
/// Propagates any error from [`crate::services::recurring::create_template`].
#[tauri::command]
pub fn create_template(
    active: tauri::State<'_, ActiveState>,
    input: CreateTemplateInput,
) -> Result<RecurringTemplate, AppError> {
    let state = active.get()?;
    let template = {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::recurring::create_template(state.store.as_ref(), input, Utc::now())?
    };
    state.trigger.trigger_mutation(Mutation::TemplateCreated)?;
    Ok(template)
}

/// Retrieve a recurring template by ID.
///
/// # Errors
///
/// Propagates any error from [`crate::services::recurring::get_template`].
#[tauri::command]
pub fn get_template(
    active: tauri::State<'_, ActiveState>,
    id: String,
) -> Result<RecurringTemplate, AppError> {
    let state = active.get()?;
    crate::services::recurring::get_template(state.store.as_ref(), &id)
}

/// Apply a partial update to an existing recurring template.
///
/// Triggers an immediate full reschedule since instance parameters may have
/// changed.
///
/// # Errors
///
/// Propagates any error from [`crate::services::recurring::update_template`].
#[tauri::command]
pub fn update_template(
    active: tauri::State<'_, ActiveState>,
    id: String,
    input: UpdateTemplateInput,
) -> Result<RecurringTemplate, AppError> {
    let state = active.get()?;
    let template = {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::recurring::update_template(state.store.as_ref(), &id, input, Utc::now())?
    };
    state.trigger.trigger_mutation(Mutation::TemplateUpdated)?;
    Ok(template)
}

/// Delete a recurring template by ID.
///
/// Triggers an immediate full reschedule since instances of the deleted template
/// need to be cleaned up.
///
/// # Errors
///
/// Propagates any error from [`crate::services::recurring::delete_template`].
#[tauri::command]
pub fn delete_template(active: tauri::State<'_, ActiveState>, id: String) -> Result<(), AppError> {
    let state = active.get()?;
    {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::recurring::delete_template(state.store.as_ref(), &id, Utc::now())?;
    }
    state.trigger.trigger_mutation(Mutation::TemplateDeleted)?;
    Ok(())
}

/// List all recurring templates.
///
/// # Errors
///
/// Propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn list_templates(
    active: tauri::State<'_, ActiveState>,
) -> Result<Vec<RecurringTemplate>, AppError> {
    let state = active.get()?;
    state.store.list_templates()
}
