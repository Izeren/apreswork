// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared fixtures and construction tests for the `SqliteStore` test suite.
//! Entity-specific tests live in the sibling modules.

mod chunk;
mod comment;
mod config_busy;
mod external_event_single;
mod schedule;
mod sync_state;
mod task;
mod template;
mod tx;

use chrono::{Duration, NaiveTime, TimeZone, Utc, Weekday};

use crate::test_support::{default_schedule_id, fixture_base};

use crate::db::sqlite::SqliteStore;
use crate::domain::cadence::Cadence;
use crate::domain::enums::ChunkStatus;
use crate::domain::models::{
    Chunk, ExternalEventRecord, RecurringTemplate, Schedule, ScheduleWindow, Task,
};
use crate::error::AppError;
use crate::traits::storage::{Store, TaskStore};

/// Create a [`Task`] with sensible defaults, using the default schedule from seed data.
fn make_test_task(store: &SqliteStore) -> Task {
    let schedule_id = default_schedule_id(store);

    Task {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Test task".to_owned(),
        description: Some("A test task description".to_owned()),
        deadline: Some(Utc.with_ymd_and_hms(2026, 4, 1, 23, 59, 59).unwrap()),
        expire_at: Some(Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap()),
        schedule_id,
        ..Task::test_default()
    }
}

/// Create a task with the given labels and persist it. Returns the stored task.
fn create_labeled_task(store: &SqliteStore, labels: &[&str]) -> Task {
    let mut task = make_test_task(store);
    task.labels = labels.iter().map(|s| (*s).to_owned()).collect();
    store.create_task(&task).expect("create labeled task");
    task
}

/// Create a [`Chunk`] with sensible defaults, creating a parent task first.
fn make_test_chunk(store: &SqliteStore) -> Chunk {
    let task = make_test_task(store);
    store.create_task(&task).expect("create parent task");

    let now = fixture_base();
    Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task.id,
        start_time: Utc.with_ymd_and_hms(2026, 3, 13, 18, 0, 0).unwrap(),
        end_time: Utc.with_ymd_and_hms(2026, 3, 13, 19, 0, 0).unwrap(),
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

/// Create a [`Schedule`] with sensible defaults for testing.
fn make_test_schedule() -> Schedule {
    let schedule_id = uuid::Uuid::now_v7().to_string();
    let now = fixture_base();
    Schedule {
        id: schedule_id.clone(),
        name: "Test Schedule".to_owned(),
        is_default: false,
        windows: vec![
            ScheduleWindow {
                id: uuid::Uuid::now_v7().to_string(),
                schedule_id: schedule_id.clone(),
                day_of_week: Weekday::Mon,
                start_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                end_time: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            },
            ScheduleWindow {
                id: uuid::Uuid::now_v7().to_string(),
                schedule_id,
                day_of_week: Weekday::Wed,
                start_time: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
                end_time: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
            },
        ],
        created_at: now,
        updated_at: now,
    }
}

/// Create a [`RecurringTemplate`] with sensible defaults, using the
/// default schedule from seed data.
fn make_test_template(store: &SqliteStore) -> RecurringTemplate {
    let schedule_id = default_schedule_id(store);
    let created_at = fixture_base();
    let updated_at = created_at + Duration::hours(1);

    RecurringTemplate {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Test Template".to_owned(),
        description: Some("A test template".to_owned()),
        duration_minutes: 30,
        cadence: Cadence::weekly(vec![Weekday::Mon, Weekday::Wed]),
        schedule_id,
        created_at,
        updated_at,
        ..RecurringTemplate::test_default()
    }
}

/// Create an [`ExternalEventRecord`] on `2026-03-13` spanning
/// `start_hour`–`end_hour`, busy and not declined.
fn make_external_event(
    event_id: &str,
    calendar_id: &str,
    start_hour: u32,
    end_hour: u32,
) -> ExternalEventRecord {
    ExternalEventRecord {
        id: uuid::Uuid::now_v7().to_string(),
        calendar_id: calendar_id.to_owned(),
        event_id: event_id.to_owned(),
        title: format!("Event {event_id}"),
        description: None,
        start_time: Utc.with_ymd_and_hms(2026, 3, 13, start_hour, 0, 0).unwrap(),
        end_time: Utc.with_ymd_and_hms(2026, 3, 13, end_hour, 0, 0).unwrap(),
        busy: true,
        declined: false,
        all_day: false,
        updated_at: Utc.with_ymd_and_hms(2026, 3, 13, 0, 0, 0).unwrap(),
    }
}

#[test]
fn new_in_memory_creates_store() {
    let _store = SqliteStore::new_in_memory();
}

#[test]
fn wal_mode_enabled() {
    // WAL is only meaningful for file-backed databases; in-memory uses
    // "memory" journal mode. Test with a temp file instead.
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("test.db");
    let store = SqliteStore::new(path.to_str().expect("path to str")).expect("open store");

    let conn = store.conn.lock().expect("lock");
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .expect("query journal_mode");
    assert_eq!(mode, "wal");
}

#[test]
fn foreign_keys_enabled() {
    let store = SqliteStore::new_in_memory();
    let conn = store.conn.lock().expect("lock");
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("query foreign_keys");
    assert_eq!(fk, 1);
}

#[test]
fn vacuum_into_is_refused_inside_a_transaction() {
    // `VACUUM INTO` cannot run inside an open transaction, so the trait's
    // default implementation (which `TxStore` inherits) must refuse.
    let store = SqliteStore::new_in_memory();
    let dir = tempfile::tempdir().expect("create temp dir");
    let dest = dir.path().join("snapshot.db");
    store
        .with_tx(&mut |tx| {
            let err = tx.vacuum_into(&dest).expect_err("must refuse in a tx");
            assert!(matches!(err, AppError::Database(_)), "got: {err}");
            Ok(())
        })
        .expect("tx itself succeeds");
}

#[cfg(unix)]
#[test]
fn vacuum_into_rejects_a_non_utf8_destination() {
    use std::os::unix::ffi::OsStrExt as _;
    let store = SqliteStore::new_in_memory();
    let dest = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"\xff\xfe-snapshot"));
    let err = store.vacuum_into(&dest).expect_err("must reject");
    assert!(matches!(err, AppError::Database(_)), "got: {err}");
}
