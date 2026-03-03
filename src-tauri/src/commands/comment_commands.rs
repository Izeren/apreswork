// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tauri command thin wrappers for task comments (M12).
//!
//! Each function resolves the active profile's state from [`ActiveState`],
//! delegates to the corresponding service function, and returns the result.
//! Comments never affect scheduling, so there is no trigger plumbing here.
//! Until authentication lands, the acting author for edits/deletes is the
//! hardcoded default author (M12.10).

// Tauri command signatures require by-value `State` and `String` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]
use chrono::Utc;

use crate::domain::inputs::{CreateCommentInput, UpdateCommentInput};
use crate::domain::models::Comment;
use crate::error::AppError;
use crate::services::comment::DEFAULT_AUTHOR;
use crate::state::ActiveState;

/// Create a comment on a task.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if content or author is invalid, or
/// propagates any [`AppError::Database`] or [`AppError::NotFound`] from the
/// store.
#[tauri::command]
pub fn create_comment(
    active: tauri::State<'_, ActiveState>,
    input: CreateCommentInput,
) -> Result<Comment, AppError> {
    let state = active.get()?;
    crate::services::comment::create_comment(state.store.as_ref(), input, Utc::now())
}

/// Update a comment's content, acting as the default author.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the caller is not the comment's author
/// or the comment is a system comment; propagates any [`AppError::NotFound`]
/// or [`AppError::Database`] from the store.
#[tauri::command]
pub fn update_comment(
    active: tauri::State<'_, ActiveState>,
    id: String,
    input: UpdateCommentInput,
) -> Result<Comment, AppError> {
    let state = active.get()?;
    crate::services::comment::update_comment(
        state.store.as_ref(),
        &id,
        &input,
        DEFAULT_AUTHOR,
        Utc::now(),
    )
}

/// Delete a comment, acting as the default author.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the comment is a system comment or the
/// caller is not the author; propagates any [`AppError::NotFound`] or
/// [`AppError::Database`] from the store.
#[tauri::command]
pub fn delete_comment(active: tauri::State<'_, ActiveState>, id: String) -> Result<(), AppError> {
    let state = active.get()?;
    crate::services::comment::delete_comment(state.store.as_ref(), &id, DEFAULT_AUTHOR)
}

/// List a task's comments, newest first.
///
/// # Errors
///
/// Propagates any [`AppError::Database`] from the store.
#[tauri::command]
pub fn list_comments(
    active: tauri::State<'_, ActiveState>,
    task_id: String,
) -> Result<Vec<Comment>, AppError> {
    let state = active.get()?;
    crate::services::comment::list_comments(state.store.as_ref(), &task_id)
}
