// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for application configuration.
//!
//! There is no config service — the `update_config` command applies trivial
//! patch semantics directly. This is the one permitted exception to the
//! "no business logic in commands" rule.

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use crate::domain::inputs::UpdateConfigInput;
use crate::domain::models::AppConfig;
use crate::domain::validation::validate_config;
use crate::error::AppError;
use crate::services::trigger::Mutation;
use crate::state::ActiveState;

/// Retrieve the current application configuration.
///
/// # Errors
///
/// Propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn get_config(active: tauri::State<'_, ActiveState>) -> Result<AppConfig, AppError> {
    let state = active.get()?;
    state.store.get_config()
}

/// Apply a partial update to the application configuration.
///
/// Each field in [`UpdateConfigInput`] is `Option<T>` — only `Some` values
/// are applied. Internal timestamps (`last_reschedule`, `last_mutation`,
/// `last_sync`, `last_busy_sync`) are not user-editable.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the patched config fails
/// [`validate_config`] (out-of-range values, invalid timezone).
/// Propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn update_config(
    active: tauri::State<'_, ActiveState>,
    input: UpdateConfigInput,
) -> Result<AppConfig, AppError> {
    let state = active.get()?;
    // The guard covers the whole read-patch-write so concurrent updates
    // cannot interleave and drop each other's fields.
    let config = {
        let _guard = state.trigger.mutation_guard()?;
        let mut config = state.store.get_config()?;

        if let Some(v) = input.planning_horizon_days {
            config.planning_horizon_days = v;
        }
        if let Some(v) = input.timezone {
            config.timezone = v;
        }
        if let Some(v) = input.max_continuous_minutes {
            config.max_continuous_minutes = v;
        }
        if let Some(v) = input.min_break_minutes {
            config.min_break_minutes = v;
        }

        // Trust boundary: reject out-of-range values and invalid timezones
        // before they reach the store (validates the patched result, so the
        // stored state stays valid too).
        validate_config(&config)?;

        state.store.update_config(&config)?;
        config
    };
    state.trigger.trigger_mutation(Mutation::ConfigUpdated)?;
    Ok(config)
}
