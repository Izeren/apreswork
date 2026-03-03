// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Comment CRUD + system-comment generation (M12).
//!
//! Comments carry a plain-string `author`; [`SYSTEM_AUTHOR`] is reserved for
//! auto-generated progress comments (M12.2), which are immutable (M12.3).
//! Until authentication lands, every interactive surface (UI, REST, CLI)
//! acts as [`DEFAULT_AUTHOR`] (M12.10).

use chrono::{DateTime, Utc};

use crate::domain::inputs::{CreateCommentInput, UpdateCommentInput};
use crate::domain::models::Comment;
use crate::error::AppError;
use crate::traits::storage::Store;

/// Reserved author for auto-generated comments (M12.2).
pub const SYSTEM_AUTHOR: &str = "SYSTEM";

/// The single interactive author until authentication exists (M12.2/M12.10).
pub const DEFAULT_AUTHOR: &str = "User";

/// Create a user/agent comment on a task.
///
/// `input.author` defaults to [`DEFAULT_AUTHOR`] when `None`;
/// [`SYSTEM_AUTHOR`] is rejected — system comments are only generated
/// internally by progress transitions.
///
/// # Errors
///
/// - [`AppError::NotFound`] if the task does not exist.
/// - [`AppError::Validation`] for empty content, empty author, or the
///   reserved system author.
/// - [`AppError::Database`] on storage failure.
pub fn create_comment(
    store: &dyn Store,
    input: CreateCommentInput,
    now: DateTime<Utc>,
) -> Result<Comment, AppError> {
    if store.get_task(&input.task_id)?.is_none() {
        return Err(AppError::NotFound {
            entity: "Task".to_owned(),
            id: input.task_id,
        });
    }
    if input.content.trim().is_empty() {
        return Err(AppError::Validation(
            "Comment content must not be empty".to_owned(),
        ));
    }

    let author = input
        .author
        .as_deref()
        .unwrap_or(DEFAULT_AUTHOR)
        .trim()
        .to_owned();
    if author.is_empty() {
        return Err(AppError::Validation(
            "Comment author must not be empty".to_owned(),
        ));
    }
    if author == SYSTEM_AUTHOR {
        return Err(AppError::Validation(format!(
            "Author \"{SYSTEM_AUTHOR}\" is reserved for system comments"
        )));
    }

    let comment = Comment {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: input.task_id,
        author,
        content: input.content,
        created_at: now,
        updated_at: now,
    };
    store.create_comment(&comment)?;
    Ok(comment)
}

/// Update a comment's content (M12.7).
///
/// Only the comment's own author may edit it (M12.3); system comments are
/// immutable. A `None` content is a no-op patch — the comment is returned
/// unchanged.
///
/// # Errors
///
/// - [`AppError::NotFound`] if the comment does not exist.
/// - [`AppError::Validation`] for system comments, a foreign author, or
///   empty replacement content.
/// - [`AppError::Database`] on storage failure.
pub fn update_comment(
    store: &dyn Store,
    id: &str,
    input: &UpdateCommentInput,
    acting_author: &str,
    now: DateTime<Utc>,
) -> Result<Comment, AppError> {
    let mut comment = get_owned_comment(store, id, acting_author, "edited")?;

    let Some(content) = input.content.as_ref() else {
        return Ok(comment);
    };
    if content.trim().is_empty() {
        return Err(AppError::Validation(
            "Comment content must not be empty".to_owned(),
        ));
    }

    comment.content.clone_from(content);
    comment.updated_at = now;
    store.update_comment(&comment)?;
    Ok(comment)
}

/// Delete a comment. Same ownership rules as [`update_comment`].
///
/// # Errors
///
/// - [`AppError::NotFound`] if the comment does not exist.
/// - [`AppError::Validation`] for system comments or a foreign author.
/// - [`AppError::Database`] on storage failure.
pub fn delete_comment(store: &dyn Store, id: &str, acting_author: &str) -> Result<(), AppError> {
    let comment = get_owned_comment(store, id, acting_author, "deleted")?;
    store.delete_comment(&comment.id)
}

/// Return a task's comments, newest first (M12.4).
///
/// # Errors
///
/// - [`AppError::NotFound`] if the task does not exist.
/// - [`AppError::Database`] on storage failure.
pub fn list_comments(store: &dyn Store, task_id: &str) -> Result<Vec<Comment>, AppError> {
    if store.get_task(task_id)?.is_none() {
        return Err(AppError::NotFound {
            entity: "Task".to_owned(),
            id: task_id.to_owned(),
        });
    }
    store.list_comments_for_task(task_id)
}

/// Fetch a comment and enforce the mutation ownership rules (M12.3):
/// system comments are immutable, others belong to their author only.
/// `action` names the attempted operation in error messages ("edited"/"deleted").
fn get_owned_comment(
    store: &dyn Store,
    id: &str,
    acting_author: &str,
    action: &str,
) -> Result<Comment, AppError> {
    let comment = store.get_comment(id)?.ok_or_else(|| AppError::NotFound {
        entity: "Comment".to_owned(),
        id: id.to_owned(),
    })?;
    if comment.author == SYSTEM_AUTHOR {
        return Err(AppError::Validation(format!(
            "System comments cannot be {action}"
        )));
    }
    if comment.author != acting_author {
        return Err(AppError::Validation(format!(
            "Comments can only be {action} by their author"
        )));
    }
    Ok(comment)
}

/// Build a [`SYSTEM_AUTHOR`] comment. Callers persist it via the store —
/// typically inside the same transaction as the progress change it records.
#[must_use]
pub(crate) fn system_comment(task_id: &str, content: String, now: DateTime<Utc>) -> Comment {
    Comment {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task_id.to_owned(),
        author: SYSTEM_AUTHOR.to_owned(),
        content,
        created_at: now,
        updated_at: now,
    }
}

/// System-comment content for a completed chunk (M12.5), e.g.
/// `Chunk completed: +45m logged (1h 15m / 2h total)`.
#[must_use]
pub(crate) fn chunk_completed_content(
    logged_minutes: i64,
    total_logged_minutes: i64,
    duration_minutes: i64,
) -> String {
    format!(
        "Chunk completed: +{} logged ({} / {} total)",
        format_minutes(logged_minutes),
        format_minutes(total_logged_minutes),
        format_minutes(duration_minutes)
    )
}

/// System-comment content for a reopened chunk (M12.5), e.g.
/// `Chunk reopened: -45m logged (30m / 2h total)`.
#[must_use]
pub(crate) fn chunk_reopened_content(
    subtracted_minutes: i64,
    total_logged_minutes: i64,
    duration_minutes: i64,
) -> String {
    format!(
        "Chunk reopened: -{} logged ({} / {} total)",
        format_minutes(subtracted_minutes),
        format_minutes(total_logged_minutes),
        format_minutes(duration_minutes)
    )
}

/// Render minutes as a compact duration: `0m`, `45m`, `1h`, `1h 15m`, `2h`.
///
/// Callers guarantee non-negative input (deltas are clamped upstream);
/// negative values would render as e.g. `-1h -15m`.
#[must_use]
pub(crate) fn format_minutes(minutes: i64) -> String {
    debug_assert!(minutes >= 0, "format_minutes expects non-negative minutes");
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours == 0 {
        format!("{mins}m")
    } else if mins == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {mins}m")
    }
}

#[cfg(test)]
mod tests;
