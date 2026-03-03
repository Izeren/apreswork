// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared fixtures for the scheduling-service test suite.
//! Concern-specific tests live in the sibling modules.

mod diff;
mod incremental;
mod integration;
mod reschedule;
mod stale_locks;
mod warnings;

use chrono::{DateTime, Utc};

use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{ChunkStatus, TaskStatus};
use crate::domain::models::{Chunk, Task};
use crate::error::AppError;
use crate::services::scheduling::reschedule;
use crate::test_support::{test_now, utc};
use crate::traits::scheduling::{ScheduleInput, ScheduleResult, Scheduler};
use crate::traits::storage::{ChunkStore, ConfigStore, TaskStore};

fn stored_task(store: &SqliteStore, id: &str) -> Option<Task> {
    store.get_task(id).unwrap()
}

fn chunk_count(store: &SqliteStore) -> usize {
    store
        .get_chunks_in_range(utc(1970, 1, 1, 0, 0), utc(2999, 1, 1, 0, 0))
        .unwrap()
        .len()
}

fn make_task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: id.to_owned(),
        title: format!("Task {id}"),
        status,
        schedule_id: "sched-1".to_owned(),
        ..Task::test_default()
    }
}

fn make_chunk(id: &str, task_id: &str, is_fixed: bool) -> Chunk {
    let now = test_now();
    Chunk {
        id: id.to_owned(),
        task_id: task_id.to_owned(),
        start_time: now + chrono::Duration::hours(1),
        end_time: now + chrono::Duration::hours(2),
        status: ChunkStatus::Scheduled,
        is_fixed,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn make_auto_chunk(id: &str, task_id: &str) -> Chunk {
    make_chunk(id, task_id, false)
}

fn make_fixed_chunk(id: &str, task_id: &str) -> Chunk {
    make_chunk(id, task_id, true)
}

/// Build a fixed chunk with explicit start/end times.
pub(super) fn make_fixed_chunk_at(
    id: &str,
    task_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Chunk {
    Chunk {
        id: id.to_owned(),
        task_id: task_id.to_owned(),
        start_time: start,
        end_time: end,
        status: ChunkStatus::Scheduled,
        is_fixed: true,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: start,
        updated_at: start,
    }
}

/// Build a chunk with explicit start/end times and an optional `google_event_id`.
fn make_chunk_at(
    id: &str,
    task_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    google_event_id: Option<&str>,
) -> Chunk {
    Chunk {
        id: id.to_owned(),
        task_id: task_id.to_owned(),
        start_time: start,
        end_time: end,
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: google_event_id.map(str::to_owned),
        created_at: start,
        updated_at: start,
    }
}

pub(super) struct MockScheduler {
    result: ScheduleResult,
}

impl MockScheduler {
    pub(super) fn empty() -> Self {
        Self {
            result: ScheduleResult {
                placed_chunks: Vec::new(),
                warnings: Vec::new(),
            },
        }
    }

    pub(super) fn with_chunks(chunks: Vec<Chunk>) -> Self {
        Self {
            result: ScheduleResult {
                placed_chunks: chunks,
                warnings: Vec::new(),
            },
        }
    }
}

impl Scheduler for MockScheduler {
    fn schedule(&self, _input: ScheduleInput) -> Result<ScheduleResult, AppError> {
        Ok(self.result.clone())
    }
}

/// Run `reschedule`, assert it placed nothing and produced no warnings, then
/// assert both config timestamps were advanced to `now`. Shared by the mock-
/// and `DefaultScheduler`-backed "no work to do" tests.
fn assert_reschedule_empty_updates_config(
    store: &SqliteStore,
    scheduler: &dyn Scheduler,
    now: DateTime<Utc>,
) {
    let result = reschedule(store, scheduler, now).expect("reschedule should succeed");
    assert!(result.placed_chunks.is_empty());
    assert!(result.warnings.is_empty());

    let cfg = store.get_config().unwrap();
    assert_eq!(cfg.last_reschedule, Some(now));
    assert_eq!(cfg.last_mutation, Some(now));
}
