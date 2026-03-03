// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`ScheduleStore`] implementation: mappers, query helpers, and thin trait
//! impls for both [`SqliteStore`] (mutex-guarded) and [`TxStore`]
//! (in-transaction).

use chrono::{DateTime, NaiveTime, Utc, Weekday};
use rusqlite::Connection;

use super::{SqliteStore, TxStore};
use crate::domain::models::{Schedule, ScheduleWindow};
use crate::error::AppError;
use crate::traits::storage::ScheduleStore;

/// Convert a [`Weekday`] variant to its integer representation (Mon=0..Sun=6).
pub(super) const fn weekday_to_i64(w: Weekday) -> i64 {
    match w {
        Weekday::Mon => 0,
        Weekday::Tue => 1,
        Weekday::Wed => 2,
        Weekday::Thu => 3,
        Weekday::Fri => 4,
        Weekday::Sat => 5,
        Weekday::Sun => 6,
    }
}

/// Convert an integer back to a [`Weekday`], returning an error for unknown values.
pub(super) fn weekday_from_i64(v: i64) -> Result<Weekday, AppError> {
    match v {
        0 => Ok(Weekday::Mon),
        1 => Ok(Weekday::Tue),
        2 => Ok(Weekday::Wed),
        3 => Ok(Weekday::Thu),
        4 => Ok(Weekday::Fri),
        5 => Ok(Weekday::Sat),
        6 => Ok(Weekday::Sun),
        other => Err(AppError::Database(format!(
            "unknown weekday value: {other}"
        ))),
    }
}

const SCHEDULE_SELECT_COLS: &str = "id, name, is_default, created_at, updated_at";

/// Build a [`Schedule`] from a row of the `schedules` table (5 columns in SELECT order).
///
/// Fields requiring conversion (datetimes) use placeholders; the caller
/// must apply [`finalize_schedule`] after exiting the rusqlite closure.
fn row_to_schedule(row: &rusqlite::Row<'_>) -> Result<Schedule, rusqlite::Error> {
    Ok(Schedule {
        id: row.get(0)?,
        name: row.get(1)?,
        is_default: row.get(2)?,
        windows: Vec::new(),    // populated separately
        created_at: Utc::now(), // placeholder — overwritten by finalize_schedule
        updated_at: Utc::now(), // placeholder
    })
}

/// Raw DB column values for schedule fields that need conversion (`created_at`, `updated_at`).
type RawScheduleFields = (String, String);

/// Read raw string columns that need conversion outside the rusqlite closure.
fn row_to_raw_schedule_fields(
    row: &rusqlite::Row<'_>,
) -> Result<RawScheduleFields, rusqlite::Error> {
    Ok((
        row.get(3)?, // created_at
        row.get(4)?, // updated_at
    ))
}

/// Finalize a [`Schedule`] by converting raw DB datetime strings.
fn finalize_schedule(
    mut schedule: Schedule,
    created_at_str: &str,
    updated_at_str: &str,
) -> Result<Schedule, AppError> {
    schedule.created_at = DateTime::parse_from_rfc3339(created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AppError::Database(format!("invalid created_at: {e}")))?;
    schedule.updated_at = DateTime::parse_from_rfc3339(updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AppError::Database(format!("invalid updated_at: {e}")))?;
    Ok(schedule)
}

fn fetch_windows_for_schedule(
    conn: &Connection,
    schedule_id: &str,
) -> Result<Vec<ScheduleWindow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, schedule_id, day_of_week, start_time, end_time \
         FROM schedule_windows WHERE schedule_id = ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![schedule_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut windows = Vec::new();
    for row in rows {
        let (id, sched_id, day_i64, start_str, end_str) = row?;
        let day_of_week = weekday_from_i64(day_i64)?;
        let start_time = NaiveTime::parse_from_str(&start_str, "%H:%M")
            .map_err(|e| AppError::Database(format!("invalid start_time: {e}")))?;
        let end_time = NaiveTime::parse_from_str(&end_str, "%H:%M")
            .map_err(|e| AppError::Database(format!("invalid end_time: {e}")))?;
        windows.push(ScheduleWindow {
            id,
            schedule_id: sched_id,
            day_of_week,
            start_time,
            end_time,
        });
    }
    Ok(windows)
}

/// Finalize timestamps and attach windows to a partially-built [`Schedule`].
fn finalize_with_windows(
    conn: &Connection,
    schedule: Schedule,
    (created, updated): (String, String),
) -> Result<Schedule, AppError> {
    let mut schedule = finalize_schedule(schedule, &created, &updated)?;
    schedule.windows = fetch_windows_for_schedule(conn, &schedule.id)?;
    Ok(schedule)
}

fn create_schedule(conn: &Connection, schedule: &Schedule) -> Result<(), AppError> {
    let mut stmt = conn.prepare(
        "INSERT INTO schedules (id, name, is_default, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    stmt.execute(rusqlite::params![
        schedule.id,
        schedule.name,
        schedule.is_default,
        schedule.created_at.to_rfc3339(),
        schedule.updated_at.to_rfc3339(),
    ])?;

    insert_windows(conn, schedule)?;
    Ok(())
}

/// Insert all windows of `schedule` into the `schedule_windows` table.
fn insert_windows(conn: &Connection, schedule: &Schedule) -> Result<(), AppError> {
    let mut window_stmt = conn.prepare(
        "INSERT INTO schedule_windows (id, schedule_id, day_of_week, start_time, end_time) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for window in &schedule.windows {
        window_stmt.execute(rusqlite::params![
            window.id,
            window.schedule_id,
            weekday_to_i64(window.day_of_week),
            window.start_time.format("%H:%M").to_string(),
            window.end_time.format("%H:%M").to_string(),
        ])?;
    }
    Ok(())
}

/// Read the partial [`Schedule`] plus its raw `(created, updated)` timestamp
/// strings from a `schedules` row (column order matches `SCHEDULE_SELECT_COLS`).
fn read_schedule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Schedule, (String, String))> {
    let schedule = row_to_schedule(row)?;
    let raw = row_to_raw_schedule_fields(row)?;
    Ok((schedule, raw))
}

/// Run a single-row `schedules` query, finalize timestamps, and attach windows.
/// Returns `Ok(None)` when the query matches no row.
fn query_one_schedule(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Option<Schedule>, AppError> {
    let mut stmt = conn.prepare(sql)?;
    match stmt.query_row(params, read_schedule_row) {
        Ok((schedule, raw)) => Ok(Some(finalize_with_windows(conn, schedule, raw)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}

fn get_schedule(conn: &Connection, id: &str) -> Result<Option<Schedule>, AppError> {
    let sql = format!("SELECT {SCHEDULE_SELECT_COLS} FROM schedules WHERE id = ?1");
    query_one_schedule(conn, &sql, rusqlite::params![id])
}

fn get_default_schedule(conn: &Connection) -> Result<Schedule, AppError> {
    let sql = format!("SELECT {SCHEDULE_SELECT_COLS} FROM schedules WHERE is_default = 1");
    query_one_schedule(conn, &sql, ())?.ok_or_else(|| AppError::NotFound {
        entity: "Schedule".into(),
        id: "default".into(),
    })
}

fn update_schedule(conn: &Connection, schedule: &Schedule) -> Result<(), AppError> {
    let mut stmt = conn.prepare(
        "UPDATE schedules SET \
            name = ?2, is_default = ?3, created_at = ?4, updated_at = ?5 \
        WHERE id = ?1",
    )?;
    stmt.execute(rusqlite::params![
        schedule.id,
        schedule.name,
        schedule.is_default,
        schedule.created_at.to_rfc3339(),
        schedule.updated_at.to_rfc3339(),
    ])?;

    conn.execute(
        "DELETE FROM schedule_windows WHERE schedule_id = ?1",
        rusqlite::params![schedule.id],
    )?;

    insert_windows(conn, schedule)?;
    Ok(())
}

fn delete_schedule(conn: &Connection, id: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM schedules WHERE id = ?1", rusqlite::params![id])?;
    Ok(())
}

fn list_schedules(conn: &Connection) -> Result<Vec<Schedule>, AppError> {
    let sql = format!("SELECT {SCHEDULE_SELECT_COLS} FROM schedules");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], read_schedule_row)?;

    let mut schedules = Vec::new();
    for row in rows {
        let (schedule, raw) = row?;
        schedules.push(finalize_with_windows(conn, schedule, raw)?);
    }

    Ok(schedules)
}

impl ScheduleStore for SqliteStore {
    fn create_schedule(&self, schedule: &Schedule) -> Result<(), AppError> {
        self.in_tx(|conn| create_schedule(conn, schedule))
    }

    fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, AppError> {
        get_schedule(&*self.lock()?, id)
    }

    fn get_default_schedule(&self) -> Result<Schedule, AppError> {
        get_default_schedule(&*self.lock()?)
    }

    fn update_schedule(&self, schedule: &Schedule) -> Result<(), AppError> {
        self.in_tx(|conn| update_schedule(conn, schedule))
    }

    fn delete_schedule(&self, id: &str) -> Result<(), AppError> {
        delete_schedule(&*self.lock()?, id)
    }

    fn list_schedules(&self) -> Result<Vec<Schedule>, AppError> {
        list_schedules(&*self.lock()?)
    }
}

impl ScheduleStore for TxStore<'_> {
    fn create_schedule(&self, schedule: &Schedule) -> Result<(), AppError> {
        create_schedule(self.conn, schedule)
    }

    fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, AppError> {
        get_schedule(self.conn, id)
    }

    fn get_default_schedule(&self) -> Result<Schedule, AppError> {
        get_default_schedule(self.conn)
    }

    fn update_schedule(&self, schedule: &Schedule) -> Result<(), AppError> {
        update_schedule(self.conn, schedule)
    }

    fn delete_schedule(&self, id: &str) -> Result<(), AppError> {
        delete_schedule(self.conn, id)
    }

    fn list_schedules(&self) -> Result<Vec<Schedule>, AppError> {
        list_schedules(self.conn)
    }
}
