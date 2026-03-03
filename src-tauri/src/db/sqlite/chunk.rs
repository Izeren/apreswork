// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`ChunkStore`] implementation: mappers, query helpers, and thin trait impls
//! for both [`SqliteStore`] (mutex-guarded) and [`TxStore`] (in-transaction).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use super::task::fetch_labels_for_tasks;
use super::{
    format_optional_datetime, parse_datetime, parse_optional_datetime, priority_from_i64,
    SqliteStore, TxStore,
};
use crate::domain::enums::ChunkStatus;
use crate::domain::inputs::AgendaItem;
use crate::domain::models::Chunk;
use crate::error::AppError;
use crate::traits::storage::ChunkStore;

pub(super) const fn chunk_status_to_str(s: ChunkStatus) -> &'static str {
    match s {
        ChunkStatus::Scheduled => "scheduled",
        ChunkStatus::Completed => "completed",
    }
}

pub(super) fn chunk_status_from_str(s: &str) -> Result<ChunkStatus, AppError> {
    match s {
        "scheduled" => Ok(ChunkStatus::Scheduled),
        "completed" => Ok(ChunkStatus::Completed),
        other => Err(AppError::Database(format!("unknown chunk status: {other}"))),
    }
}

const CHUNK_SELECT_COLS: &str = "id, task_id, start_time, end_time, status, is_fixed, \
     logged_minutes, completed_at, google_event_id, created_at, updated_at";

/// Build a [`Chunk`] from a row of the `chunks` table (11 columns in SELECT order).
///
/// Fields requiring conversion (status, datetimes) use placeholders; the caller
/// must apply [`finalize_chunk`] after exiting the rusqlite closure.
fn row_to_chunk(row: &rusqlite::Row<'_>) -> Result<Chunk, rusqlite::Error> {
    Ok(Chunk {
        id: row.get(0)?,
        task_id: row.get(1)?,
        start_time: DateTime::<Utc>::UNIX_EPOCH, // placeholder — overwritten by finalize_chunk
        end_time: DateTime::<Utc>::UNIX_EPOCH,
        status: ChunkStatus::Scheduled,
        is_fixed: row.get(5)?,
        logged_minutes: row.get(6)?,
        completed_at: None,
        google_event_id: row.get(8)?,
        created_at: DateTime::<Utc>::UNIX_EPOCH,
        updated_at: DateTime::<Utc>::UNIX_EPOCH,
    })
}

/// Raw DB column values for chunk fields that need conversion outside the rusqlite closure.
///
/// Fields: (`start_time`, `end_time`, status, `completed_at`, `created_at`, `updated_at`)
type RawChunkFields = (String, String, String, Option<String>, String, String);

/// Read raw string columns that need conversion outside the rusqlite closure.
fn row_to_raw_chunk_fields(row: &rusqlite::Row<'_>) -> Result<RawChunkFields, rusqlite::Error> {
    Ok((
        row.get(2)?,  // start_time
        row.get(3)?,  // end_time
        row.get(4)?,  // status
        row.get(7)?,  // completed_at
        row.get(9)?,  // created_at
        row.get(10)?, // updated_at
    ))
}

fn parse_opt_dt(s: Option<&str>) -> Result<Option<DateTime<Utc>>, AppError> {
    Ok(s.map(parse_optional_datetime).transpose()?.flatten())
}

fn finalize_chunk(
    mut chunk: Chunk,
    start_time_str: &str,
    end_time_str: &str,
    status_str: &str,
    completed_at_str: Option<&str>,
    created_at_str: &str,
    updated_at_str: &str,
) -> Result<Chunk, AppError> {
    chunk.start_time = parse_datetime(start_time_str, "start_time")?;
    chunk.end_time = parse_datetime(end_time_str, "end_time")?;
    chunk.status = chunk_status_from_str(status_str)?;
    chunk.completed_at = parse_opt_dt(completed_at_str)?;
    chunk.created_at = parse_datetime(created_at_str, "created_at")?;
    chunk.updated_at = parse_datetime(updated_at_str, "updated_at")?;
    Ok(chunk)
}

fn collect_chunks(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> Result<(Chunk, RawChunkFields), rusqlite::Error>,
    >,
) -> Result<Vec<Chunk>, AppError> {
    let mut result = Vec::new();
    for row in rows {
        let (chunk, (start, end, status, completed, created, updated)) = row?;
        let chunk = finalize_chunk(
            chunk,
            &start,
            &end,
            &status,
            completed.as_deref(),
            &created,
            &updated,
        )?;
        result.push(chunk);
    }
    Ok(result)
}

/// Bind a chunk's 11 columns (SELECT order: `?1`=id … `?11`=`updated_at`) and run
/// a prepared write. INSERT and UPDATE share the identical positional binding.
fn execute_chunk_write(stmt: &mut rusqlite::Statement<'_>, chunk: &Chunk) -> Result<(), AppError> {
    stmt.execute(rusqlite::params![
        chunk.id,
        chunk.task_id,
        chunk.start_time.to_rfc3339(),
        chunk.end_time.to_rfc3339(),
        chunk_status_to_str(chunk.status),
        chunk.is_fixed,
        chunk.logged_minutes,
        format_optional_datetime(chunk.completed_at.as_ref()),
        chunk.google_event_id,
        chunk.created_at.to_rfc3339(),
        chunk.updated_at.to_rfc3339(),
    ])?;
    Ok(())
}

/// Run a `SELECT {CHUNK_SELECT_COLS} FROM chunks {tail}` query and finalize the
/// rows. `tail` is a static `WHERE`/order clause; `params` supplies its binds. The
/// row → chunk assembly is shared with every reader via [`collect_chunks`].
fn query_chunks(
    conn: &Connection,
    tail: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Chunk>, AppError> {
    let sql = format!("SELECT {CHUNK_SELECT_COLS} FROM chunks {tail}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, |row| {
        let chunk = row_to_chunk(row)?;
        let raw = row_to_raw_chunk_fields(row)?;
        Ok((chunk, raw))
    })?;
    collect_chunks(rows)
}

fn create_chunk(conn: &Connection, chunk: &Chunk) -> Result<(), AppError> {
    let mut stmt = conn.prepare(
        "INSERT INTO chunks (\
            id, task_id, start_time, end_time, status, is_fixed, \
            logged_minutes, completed_at, google_event_id, created_at, updated_at\
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    execute_chunk_write(&mut stmt, chunk)
}

fn get_chunk(conn: &Connection, id: &str) -> Result<Option<Chunk>, AppError> {
    Ok(query_chunks(conn, "WHERE id = ?1", rusqlite::params![id])?
        .into_iter()
        .next())
}

fn update_chunk(conn: &Connection, chunk: &Chunk) -> Result<(), AppError> {
    let mut stmt = conn.prepare(
        "UPDATE chunks SET \
            task_id = ?2, start_time = ?3, end_time = ?4, status = ?5, \
            is_fixed = ?6, logged_minutes = ?7, completed_at = ?8, \
            google_event_id = ?9, created_at = ?10, updated_at = ?11 \
        WHERE id = ?1",
    )?;
    execute_chunk_write(&mut stmt, chunk)
}

fn delete_chunk(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM chunks WHERE id = ?1", rusqlite::params![id])?;
    Ok(())
}

fn get_chunks_for_task(conn: &Connection, task_id: &str) -> Result<Vec<Chunk>, AppError> {
    query_chunks(conn, "WHERE task_id = ?1", rusqlite::params![task_id])
}

fn get_chunks_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Chunk>, AppError> {
    query_chunks(
        conn,
        "WHERE start_time < ?1 AND end_time > ?2",
        rusqlite::params![end.to_rfc3339(), start.to_rfc3339()],
    )
}

type RawAgendaRow = (
    Chunk,
    RawChunkFields,
    String,
    i64,
    Option<String>,
    Option<String>,
);

fn assemble_agenda_item(
    raw: RawAgendaRow,
    labels_map: &HashMap<String, Vec<String>>,
) -> Result<AgendaItem, AppError> {
    let (
        chunk,
        (start_str, end_str, status, completed, created, updated),
        title,
        priority,
        template_id,
        deadline_str,
    ) = raw;
    let chunk = finalize_chunk(
        chunk,
        &start_str,
        &end_str,
        &status,
        completed.as_deref(),
        &created,
        &updated,
    )?;
    let task_priority = priority_from_i64(priority)?;
    let task_labels = labels_map.get(&chunk.task_id).cloned().unwrap_or_default();
    let task_deadline = parse_opt_dt(deadline_str.as_deref())?;
    Ok(AgendaItem {
        chunk,
        task_title: title,
        task_priority,
        task_labels,
        task_recurring_template_id: template_id,
        task_deadline,
    })
}

fn get_agenda_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<AgendaItem>, AppError> {
    // Chunk columns first, in `CHUNK_SELECT_COLS` order (indices 0..=10, read
    // by the shared chunk mappers), then the task fields the agenda needs
    // (11..=14). The inner join drops any chunk whose task is absent — a state
    // the FK forbids — so there is no orphan branch to handle here.
    let sql = "SELECT c.id, c.task_id, c.start_time, c.end_time, c.status, c.is_fixed, \
         c.logged_minutes, c.completed_at, c.google_event_id, c.created_at, c.updated_at, \
         t.title, t.priority, t.recurring_template_id, t.deadline \
         FROM chunks c INNER JOIN tasks t ON c.task_id = t.id \
         WHERE c.start_time < ?1 AND c.end_time > ?2";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![end.to_rfc3339(), start.to_rfc3339()],
        |row| {
            let chunk = row_to_chunk(row)?;
            let raw = row_to_raw_chunk_fields(row)?;
            let title: String = row.get(11)?;
            let priority_i64: i64 = row.get(12)?;
            let template_id: Option<String> = row.get(13)?;
            let deadline: Option<String> = row.get(14)?;
            Ok((chunk, raw, title, priority_i64, template_id, deadline))
        },
    )?;

    // Collect all rows before the label fetch to avoid N+1 per-task queries.
    let mut raw_rows: Vec<RawAgendaRow> = Vec::new();
    for row in rows {
        raw_rows.push(row?);
    }

    // One batch query for all distinct task_ids seen in this range.
    let mut unique_task_ids: Vec<&str> =
        raw_rows.iter().map(|(c, ..)| c.task_id.as_str()).collect();
    unique_task_ids.sort_unstable();
    unique_task_ids.dedup();
    let labels_map = fetch_labels_for_tasks(conn, &unique_task_ids)?;

    let mut items = Vec::new();
    for raw in raw_rows {
        items.push(assemble_agenda_item(raw, &labels_map)?);
    }
    Ok(items)
}

fn get_auto_chunks(conn: &Connection) -> Result<Vec<Chunk>, AppError> {
    query_chunks(
        conn,
        "WHERE is_fixed = 0 AND status != ?1",
        rusqlite::params![chunk_status_to_str(ChunkStatus::Completed)],
    )
}

fn get_all_fixed_and_completed(conn: &Connection) -> Result<Vec<Chunk>, AppError> {
    query_chunks(
        conn,
        "WHERE is_fixed = 1 OR status = ?1",
        rusqlite::params![chunk_status_to_str(ChunkStatus::Completed)],
    )
}

fn get_fixed_scheduled_chunks(conn: &Connection) -> Result<Vec<Chunk>, AppError> {
    query_chunks(
        conn,
        "WHERE is_fixed = 1 AND status = ?1",
        rusqlite::params![chunk_status_to_str(ChunkStatus::Scheduled)],
    )
}

fn get_past_due_scheduled_chunks(
    conn: &Connection,
    cutoff: DateTime<Utc>,
) -> Result<Vec<Chunk>, AppError> {
    query_chunks(
        conn,
        "WHERE status = ?1 AND end_time < ?2",
        rusqlite::params![
            chunk_status_to_str(ChunkStatus::Scheduled),
            cutoff.to_rfc3339()
        ],
    )
}

/// Private sealed trait: provides a `&Connection` for one call.
/// Implemented by `SqliteStore` (acquires via mutex) and `TxStore` (borrows field).
/// The blanket `impl ChunkStore for C: WithConn` below removes all method duplication.
trait WithConn {
    fn with_conn<F, R>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&Connection) -> Result<R, AppError>;
}

impl WithConn for SqliteStore {
    fn with_conn<F, R>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&Connection) -> Result<R, AppError>,
    {
        f(&*self.lock()?)
    }
}

impl WithConn for TxStore<'_> {
    fn with_conn<F, R>(&self, f: F) -> Result<R, AppError>
    where
        F: FnOnce(&Connection) -> Result<R, AppError>,
    {
        f(self.conn)
    }
}

impl<C: WithConn> ChunkStore for C {
    fn create_chunk(&self, chunk: &Chunk) -> Result<(), AppError> {
        self.with_conn(|conn| create_chunk(conn, chunk))
    }

    fn get_chunk(&self, id: &str) -> Result<Option<Chunk>, AppError> {
        self.with_conn(|conn| get_chunk(conn, id))
    }

    fn update_chunk(&self, chunk: &Chunk) -> Result<(), AppError> {
        self.with_conn(|conn| update_chunk(conn, chunk))
    }

    fn delete_chunk(&self, id: &str) -> Result<(), AppError> {
        self.with_conn(|conn| delete_chunk(conn, id))
    }

    fn get_chunks_for_task(&self, task_id: &str) -> Result<Vec<Chunk>, AppError> {
        self.with_conn(|conn| get_chunks_for_task(conn, task_id))
    }

    fn get_chunks_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Chunk>, AppError> {
        self.with_conn(|conn| get_chunks_in_range(conn, start, end))
    }

    fn get_agenda_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<AgendaItem>, AppError> {
        self.with_conn(|conn| get_agenda_in_range(conn, start, end))
    }

    fn get_auto_chunks(&self) -> Result<Vec<Chunk>, AppError> {
        self.with_conn(get_auto_chunks)
    }

    fn get_all_fixed_and_completed(&self) -> Result<Vec<Chunk>, AppError> {
        self.with_conn(get_all_fixed_and_completed)
    }

    fn get_fixed_scheduled_chunks(&self) -> Result<Vec<Chunk>, AppError> {
        self.with_conn(get_fixed_scheduled_chunks)
    }

    fn get_past_due_scheduled_chunks(&self, cutoff: DateTime<Utc>) -> Result<Vec<Chunk>, AppError> {
        self.with_conn(|conn| get_past_due_scheduled_chunks(conn, cutoff))
    }
}
