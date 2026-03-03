// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`CommentStore`] implementation (M12).
//!
//! Query helpers take a plain `&Connection` and are shared by the
//! mutex-guarded [`SqliteStore`] impls and the transaction-scoped
//! [`TxStore`] impls.

use rusqlite::{Connection, OptionalExtension};

use super::{parse_datetime, SqliteStore, TxStore};
use crate::domain::models::{Comment, EntityId};
use crate::error::AppError;
use crate::traits::storage::CommentStore;

type CommentRow = (EntityId, EntityId, String, String, String, String);

fn comment_row_from_sql(row: &rusqlite::Row<'_>) -> rusqlite::Result<CommentRow> {
    Ok((
        row.get::<_, EntityId>(0)?,
        row.get::<_, EntityId>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, String>(4)?,
        row.get::<_, String>(5)?,
    ))
}

fn comment_from_row(row: CommentRow) -> Result<Comment, AppError> {
    let (id, task_id, author, content, created_str, updated_str) = row;
    Ok(Comment {
        id,
        task_id,
        author,
        content,
        created_at: parse_datetime(&created_str, "created_at")?,
        updated_at: parse_datetime(&updated_str, "updated_at")?,
    })
}

fn create_comment(conn: &Connection, comment: &Comment) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO comments (id, task_id, author, content, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            comment.id,
            comment.task_id,
            comment.author,
            comment.content,
            comment.created_at.to_rfc3339(),
            comment.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn get_comment(conn: &Connection, id: &str) -> Result<Option<Comment>, AppError> {
    let row = conn
        .query_row(
            "SELECT id, task_id, author, content, created_at, updated_at \
             FROM comments WHERE id = ?1",
            rusqlite::params![id],
            comment_row_from_sql,
        )
        .optional()?;
    row.map(comment_from_row).transpose()
}

fn update_comment(conn: &Connection, comment: &Comment) -> Result<(), AppError> {
    // task_id, author, and created_at are immutable after creation — only the
    // editable fields are written back (M12.3).
    let rows = conn.execute(
        "UPDATE comments SET content = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![comment.id, comment.content, comment.updated_at.to_rfc3339(),],
    )?;
    if rows == 0 {
        return Err(AppError::NotFound {
            entity: "Comment".to_owned(),
            id: comment.id.clone(),
        });
    }
    Ok(())
}

fn delete_comment(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM comments WHERE id = ?1", rusqlite::params![id])?;
    Ok(())
}

fn list_comments_for_task(conn: &Connection, task_id: &str) -> Result<Vec<Comment>, AppError> {
    // Newest first (M12.4); UUID v7 ids are time-ordered, so `id` breaks
    // same-timestamp ties deterministically.
    let mut stmt = conn.prepare(
        "SELECT id, task_id, author, content, created_at, updated_at \
         FROM comments WHERE task_id = ?1 \
         ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![task_id], comment_row_from_sql)?;

    let mut result = Vec::new();
    for row in rows {
        result.push(comment_from_row(row?)?);
    }
    Ok(result)
}

impl CommentStore for SqliteStore {
    fn create_comment(&self, comment: &Comment) -> Result<(), AppError> {
        create_comment(&*self.lock()?, comment)
    }

    fn get_comment(&self, id: &str) -> Result<Option<Comment>, AppError> {
        get_comment(&*self.lock()?, id)
    }

    fn update_comment(&self, comment: &Comment) -> Result<(), AppError> {
        update_comment(&*self.lock()?, comment)
    }

    fn delete_comment(&self, id: &str) -> Result<(), AppError> {
        delete_comment(&*self.lock()?, id)
    }

    fn list_comments_for_task(&self, task_id: &str) -> Result<Vec<Comment>, AppError> {
        list_comments_for_task(&*self.lock()?, task_id)
    }
}

impl CommentStore for TxStore<'_> {
    fn create_comment(&self, comment: &Comment) -> Result<(), AppError> {
        create_comment(self.conn, comment)
    }

    fn get_comment(&self, id: &str) -> Result<Option<Comment>, AppError> {
        get_comment(self.conn, id)
    }

    fn update_comment(&self, comment: &Comment) -> Result<(), AppError> {
        update_comment(self.conn, comment)
    }

    fn delete_comment(&self, id: &str) -> Result<(), AppError> {
        delete_comment(self.conn, id)
    }

    fn list_comments_for_task(&self, task_id: &str) -> Result<Vec<Comment>, AppError> {
        list_comments_for_task(self.conn, task_id)
    }
}
