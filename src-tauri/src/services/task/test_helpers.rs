// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared test builders for the task service submodules.

use chrono::Duration;

use crate::domain::enums::ChunkStatus;
use crate::domain::models::{Chunk, Task};
use crate::test_support::fixture_base;

pub(super) fn make_task(id: &str) -> Task {
    Task::test_default().with_id(id)
}

/// Builds a chunk with a known start/end for a task, either scheduled
/// (`completed = false`) or completed with `minutes` logged (`completed = true`).
fn make_chunk(id: &str, task_id: &str, minutes: i64, completed: bool) -> Chunk {
    let start = fixture_base();
    Chunk {
        id: id.to_owned(),
        task_id: task_id.to_owned(),
        start_time: start,
        end_time: start + Duration::minutes(minutes),
        status: if completed {
            ChunkStatus::Completed
        } else {
            ChunkStatus::Scheduled
        },
        is_fixed: true,
        logged_minutes: completed.then_some(minutes),
        completed_at: completed.then_some(start),
        google_event_id: None,
        created_at: start,
        updated_at: start,
    }
}

pub(super) fn make_scheduled_chunk(id: &str, task_id: &str, duration_min: i64) -> Chunk {
    make_chunk(id, task_id, duration_min, false)
}

pub(super) fn make_completed_chunk(id: &str, task_id: &str, logged: i64) -> Chunk {
    make_chunk(id, task_id, logged, true)
}
