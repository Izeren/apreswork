// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`ExternalEventStore`], [`GoogleAuthStore`], and [`ChunkSyncStateStore`] implementations.
//!
//! Query helpers take a plain `&Connection` and are shared by the
//! mutex-guarded [`SqliteStore`] impls and the transaction-scoped
//! [`TxStore`] impls.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use super::{parse_datetime, SqliteStore, TxStore};
use crate::domain::models::{ChunkSyncState, EntityId, ExternalEventRecord, GoogleAuthState};
use crate::error::AppError;
use crate::traits::storage::{ChunkSyncStateStore, ExternalEventStore, GoogleAuthStore};

/// Upsert SQL shared by the bulk window sync and the single-event upsert.
/// Preserves the existing row id on conflict (`calendar_id` is part of the
/// unique key, so it is excluded from the `SET` list).
const UPSERT_EXTERNAL_EVENT_SQL: &str = "INSERT INTO external_events \
     (id, calendar_id, event_id, title, description, start_utc, end_utc, busy, declined, updated_at, all_day) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
     ON CONFLICT(calendar_id, event_id) DO UPDATE SET \
       title=excluded.title, \
       description=excluded.description, \
       start_utc=excluded.start_utc, \
       end_utc=excluded.end_utc, \
       busy=excluded.busy, \
       declined=excluded.declined, \
       all_day=excluded.all_day, \
       updated_at=excluded.updated_at";

/// Bind one `external_events` row (11 columns) onto a statement prepared from
/// [`UPSERT_EXTERNAL_EVENT_SQL`] and execute it.
fn bind_external_event_upsert(
    stmt: &mut rusqlite::Statement<'_>,
    event: &ExternalEventRecord,
) -> Result<(), AppError> {
    stmt.execute(rusqlite::params![
        event.id,
        event.calendar_id,
        event.event_id,
        event.title,
        event.description,
        event.start_time.to_rfc3339(),
        event.end_time.to_rfc3339(),
        i64::from(event.busy),
        i64::from(event.declined),
        event.updated_at.to_rfc3339(),
        i64::from(event.all_day),
    ])?;
    Ok(())
}

fn replace_external_events_in_window(
    conn: &Connection,
    calendar_id: &str,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    events: &[ExternalEventRecord],
) -> Result<(), AppError> {
    let window_end_str = window_end.to_rfc3339();
    let window_start_str = window_start.to_rfc3339();

    // Step a: delete rows for this calendar that overlap the window but whose
    // event_id is NOT in the incoming batch (they were removed on the provider).
    if events.is_empty() {
        // Empty batch: delete ALL overlapping rows for this calendar.
        conn.execute(
            "DELETE FROM external_events \
             WHERE start_utc < ?1 AND end_utc > ?2 AND calendar_id = ?3",
            rusqlite::params![window_end_str, window_start_str, calendar_id],
        )?;
    } else {
        // Non-empty batch: build NOT IN clause with one `?` placeholder per event.
        // The placeholder string contains only `?` characters and commas — no
        // user data is interpolated (values are bound separately).
        let placeholders: String = std::iter::repeat_n("?", events.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "DELETE FROM external_events \
             WHERE start_utc < ? AND end_utc > ? AND calendar_id = ? \
             AND event_id NOT IN ({placeholders})"
        );
        let mut params: Vec<String> =
            vec![window_end_str, window_start_str, calendar_id.to_owned()];
        for event in events {
            params.push(event.event_id.clone());
        }
        conn.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
    }

    // Step b: upsert each record, preserving the original row id on conflict.
    // calendar_id is part of the unique key and cannot change — excluded from SET.
    let mut stmt = conn.prepare(UPSERT_EXTERNAL_EVENT_SQL)?;
    for event in events {
        bind_external_event_upsert(&mut stmt, event)?;
    }

    Ok(())
}

/// Raw column tuple for one `external_events` row (pre-datetime-parse).
type ExternalEventRow = (
    EntityId,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    i64,
    i64,
    String,
    i64,
);

/// Comma-separated select list shared by the range and single-row reads.
const EXTERNAL_EVENT_COLUMNS: &str = "id, calendar_id, event_id, title, description, \
     start_utc, end_utc, busy, declined, updated_at, all_day";

/// Read one `external_events` row into the raw column tuple (order matches
/// [`EXTERNAL_EVENT_COLUMNS`]).
fn external_event_row(row: &rusqlite::Row) -> rusqlite::Result<ExternalEventRow> {
    Ok((
        row.get::<_, EntityId>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, Option<String>>(4)?,
        row.get::<_, String>(5)?,
        row.get::<_, String>(6)?,
        row.get::<_, i64>(7)?,
        row.get::<_, i64>(8)?,
        row.get::<_, String>(9)?,
        row.get::<_, i64>(10)?,
    ))
}

/// Parse a raw column tuple into an [`ExternalEventRecord`] (datetime + bool
/// conversions). Shared by the range and single-row reads.
fn parse_external_event_row(raw: ExternalEventRow) -> Result<ExternalEventRecord, AppError> {
    let (
        id,
        calendar_id,
        event_id,
        title,
        description,
        start_str,
        end_str,
        busy,
        declined,
        updated_str,
        all_day,
    ) = raw;

    let start_time = parse_datetime(&start_str, "start_utc")?;
    let end_time = parse_datetime(&end_str, "end_utc")?;
    let updated_at = parse_datetime(&updated_str, "updated_at")?;

    Ok(ExternalEventRecord {
        id,
        calendar_id,
        event_id,
        title,
        description,
        start_time,
        end_time,
        busy: busy != 0,
        declined: declined != 0,
        all_day: all_day != 0,
        updated_at,
    })
}

fn get_external_events_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<ExternalEventRecord>, AppError> {
    let sql = format!(
        "SELECT {EXTERNAL_EVENT_COLUMNS} FROM external_events \
         WHERE start_utc < ?1 AND end_utc > ?2"
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(
        rusqlite::params![end.to_rfc3339(), start.to_rfc3339()],
        external_event_row,
    )?;

    let mut result = Vec::new();
    for row in rows {
        result.push(parse_external_event_row(row?)?);
    }

    Ok(result)
}

fn get_external_event(
    conn: &Connection,
    calendar_id: &str,
    event_id: &str,
) -> Result<Option<ExternalEventRecord>, AppError> {
    let sql = format!(
        "SELECT {EXTERNAL_EVENT_COLUMNS} FROM external_events \
         WHERE calendar_id = ?1 AND event_id = ?2"
    );
    let raw = conn
        .query_row(
            &sql,
            rusqlite::params![calendar_id, event_id],
            external_event_row,
        )
        .optional()?;
    raw.map(parse_external_event_row).transpose()
}

fn upsert_external_event(conn: &Connection, event: &ExternalEventRecord) -> Result<(), AppError> {
    // Same ON CONFLICT shape as the bulk window upsert (shared SQL + binder):
    // the existing row id is preserved on update.
    let mut stmt = conn.prepare(UPSERT_EXTERNAL_EVENT_SQL)?;
    bind_external_event_upsert(&mut stmt, event)
}

fn delete_external_event(
    conn: &Connection,
    calendar_id: &str,
    event_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM external_events WHERE calendar_id = ?1 AND event_id = ?2",
        rusqlite::params![calendar_id, event_id],
    )?;
    Ok(())
}

fn clear_all_external_events(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM external_events", [])?;
    Ok(())
}

fn delete_external_events_for_calendar(
    conn: &Connection,
    calendar_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM external_events WHERE calendar_id = ?1",
        rusqlite::params![calendar_id],
    )?;
    Ok(())
}

fn get_mirrored_calendar_ids(conn: &Connection) -> Result<Vec<String>, AppError> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT calendar_id FROM external_events ORDER BY calendar_id")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn get_google_auth(conn: &Connection) -> Result<Option<GoogleAuthState>, AppError> {
    let row = conn
        .query_row(
            "SELECT calendar_id, connected_at FROM google_auth WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;

    match row {
        None => Ok(None),
        Some((calendar_id, connected_at_opt)) => {
            let connected_at = connected_at_opt
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| parse_datetime(s, "connected_at"))
                .transpose()?;
            Ok(Some(GoogleAuthState {
                calendar_id,
                connected_at,
            }))
        }
    }
}

fn set_google_auth(conn: &Connection, auth: &GoogleAuthState) -> Result<(), AppError> {
    let connected_at_str = auth.connected_at.as_ref().map(DateTime::to_rfc3339);
    conn.execute(
        "INSERT INTO google_auth (id, calendar_id, connected_at) VALUES (1, ?1, ?2) \
         ON CONFLICT(id) DO UPDATE SET \
           calendar_id=excluded.calendar_id, \
           connected_at=excluded.connected_at",
        rusqlite::params![auth.calendar_id, connected_at_str],
    )?;
    Ok(())
}

fn clear_google_auth(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM google_auth", [])?;
    Ok(())
}

fn get_chunk_sync_states_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<ChunkSyncState>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT chunk_id, event_id, etag, synced_start, synced_end, \
                synced_title, synced_description, updated_at \
         FROM chunk_sync_state \
         WHERE synced_start < ?1 AND synced_end > ?2",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![end.to_rfc3339(), start.to_rfc3339()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;

    let mut result = Vec::new();
    for row in rows {
        let (
            chunk_id,
            event_id,
            etag,
            synced_start_str,
            synced_end_str,
            synced_title,
            synced_description,
            updated_at_str,
        ) = row?;

        let synced_start = parse_datetime(&synced_start_str, "synced_start")?;
        let synced_end = parse_datetime(&synced_end_str, "synced_end")?;
        let updated_at = parse_datetime(&updated_at_str, "updated_at")?;

        result.push(ChunkSyncState {
            chunk_id,
            event_id,
            etag,
            synced_start,
            synced_end,
            synced_title,
            synced_description,
            updated_at,
        });
    }

    Ok(result)
}

fn upsert_chunk_sync_state(conn: &Connection, state: &ChunkSyncState) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO chunk_sync_state \
         (chunk_id, event_id, etag, synced_start, synced_end, \
          synced_title, synced_description, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(chunk_id) DO UPDATE SET \
           event_id=excluded.event_id, \
           etag=excluded.etag, \
           synced_start=excluded.synced_start, \
           synced_end=excluded.synced_end, \
           synced_title=excluded.synced_title, \
           synced_description=excluded.synced_description, \
           updated_at=excluded.updated_at",
        rusqlite::params![
            state.chunk_id,
            state.event_id,
            state.etag,
            state.synced_start.to_rfc3339(),
            state.synced_end.to_rfc3339(),
            state.synced_title,
            state.synced_description,
            state.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn delete_chunk_sync_state(conn: &Connection, chunk_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM chunk_sync_state WHERE chunk_id = ?1",
        rusqlite::params![chunk_id],
    )?;
    Ok(())
}

fn clear_all_chunk_sync_state(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM chunk_sync_state", [])?;
    Ok(())
}

impl ExternalEventStore for SqliteStore {
    fn replace_external_events_in_window(
        &self,
        calendar_id: &str,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        events: &[ExternalEventRecord],
    ) -> Result<(), AppError> {
        self.in_tx(|conn| {
            replace_external_events_in_window(conn, calendar_id, window_start, window_end, events)
        })
    }

    fn get_external_events_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEventRecord>, AppError> {
        get_external_events_in_range(&*self.lock()?, start, end)
    }

    fn clear_all_external_events(&self) -> Result<(), AppError> {
        clear_all_external_events(&*self.lock()?)
    }

    fn delete_external_events_for_calendar(&self, calendar_id: &str) -> Result<(), AppError> {
        delete_external_events_for_calendar(&*self.lock()?, calendar_id)
    }

    fn get_mirrored_calendar_ids(&self) -> Result<Vec<String>, AppError> {
        get_mirrored_calendar_ids(&*self.lock()?)
    }

    fn upsert_external_event(&self, event: &ExternalEventRecord) -> Result<(), AppError> {
        upsert_external_event(&*self.lock()?, event)
    }

    fn get_external_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<Option<ExternalEventRecord>, AppError> {
        get_external_event(&*self.lock()?, calendar_id, event_id)
    }

    fn delete_external_event(&self, calendar_id: &str, event_id: &str) -> Result<(), AppError> {
        delete_external_event(&*self.lock()?, calendar_id, event_id)
    }
}

impl GoogleAuthStore for SqliteStore {
    fn get_google_auth(&self) -> Result<Option<GoogleAuthState>, AppError> {
        get_google_auth(&*self.lock()?)
    }

    fn set_google_auth(&self, auth: &GoogleAuthState) -> Result<(), AppError> {
        set_google_auth(&*self.lock()?, auth)
    }

    fn clear_google_auth(&self) -> Result<(), AppError> {
        clear_google_auth(&*self.lock()?)
    }
}

impl ChunkSyncStateStore for SqliteStore {
    fn get_chunk_sync_states_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ChunkSyncState>, AppError> {
        get_chunk_sync_states_in_range(&*self.lock()?, start, end)
    }

    fn upsert_chunk_sync_state(&self, state: &ChunkSyncState) -> Result<(), AppError> {
        upsert_chunk_sync_state(&*self.lock()?, state)
    }

    fn delete_chunk_sync_state(&self, chunk_id: &str) -> Result<(), AppError> {
        delete_chunk_sync_state(&*self.lock()?, chunk_id)
    }

    fn clear_all_chunk_sync_state(&self) -> Result<(), AppError> {
        clear_all_chunk_sync_state(&*self.lock()?)
    }
}

impl ExternalEventStore for TxStore<'_> {
    fn replace_external_events_in_window(
        &self,
        calendar_id: &str,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        events: &[ExternalEventRecord],
    ) -> Result<(), AppError> {
        replace_external_events_in_window(self.conn, calendar_id, window_start, window_end, events)
    }

    fn get_external_events_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEventRecord>, AppError> {
        get_external_events_in_range(self.conn, start, end)
    }

    fn clear_all_external_events(&self) -> Result<(), AppError> {
        clear_all_external_events(self.conn)
    }

    fn delete_external_events_for_calendar(&self, calendar_id: &str) -> Result<(), AppError> {
        delete_external_events_for_calendar(self.conn, calendar_id)
    }

    fn get_mirrored_calendar_ids(&self) -> Result<Vec<String>, AppError> {
        get_mirrored_calendar_ids(self.conn)
    }

    fn upsert_external_event(&self, event: &ExternalEventRecord) -> Result<(), AppError> {
        upsert_external_event(self.conn, event)
    }

    fn get_external_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<Option<ExternalEventRecord>, AppError> {
        get_external_event(self.conn, calendar_id, event_id)
    }

    fn delete_external_event(&self, calendar_id: &str, event_id: &str) -> Result<(), AppError> {
        delete_external_event(self.conn, calendar_id, event_id)
    }
}

impl GoogleAuthStore for TxStore<'_> {
    fn get_google_auth(&self) -> Result<Option<GoogleAuthState>, AppError> {
        get_google_auth(self.conn)
    }

    fn set_google_auth(&self, auth: &GoogleAuthState) -> Result<(), AppError> {
        set_google_auth(self.conn, auth)
    }

    fn clear_google_auth(&self) -> Result<(), AppError> {
        clear_google_auth(self.conn)
    }
}

impl ChunkSyncStateStore for TxStore<'_> {
    fn get_chunk_sync_states_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ChunkSyncState>, AppError> {
        get_chunk_sync_states_in_range(self.conn, start, end)
    }

    fn upsert_chunk_sync_state(&self, state: &ChunkSyncState) -> Result<(), AppError> {
        upsert_chunk_sync_state(self.conn, state)
    }

    fn delete_chunk_sync_state(&self, chunk_id: &str) -> Result<(), AppError> {
        delete_chunk_sync_state(self.conn, chunk_id)
    }

    fn clear_all_chunk_sync_state(&self) -> Result<(), AppError> {
        clear_all_chunk_sync_state(self.conn)
    }
}
