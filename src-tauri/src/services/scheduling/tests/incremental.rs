// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `reschedule_incremental` flow (cascading displacement,
//! duration invariant, status transitions).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use super::{make_auto_chunk, make_chunk_at, make_fixed_chunk, make_task, stored_task};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{Priority, TaskStatus};
use crate::domain::models::{AppConfig, Chunk};
use crate::error::AppError;
use crate::services::scheduling::reschedule_incremental;
use crate::test_support::{
    default_config, seed_chunk, seed_task, test_now, test_store_with_config,
};
use crate::traits::scheduling::{ScheduleInput, ScheduleResult, Scheduler};
use crate::traits::storage::{ChunkStore, ConfigStore};

struct IncrementalMockScheduler {
    chunks_by_task: HashMap<String, Vec<Chunk>>,
}

impl IncrementalMockScheduler {
    fn new(chunks_by_task: HashMap<String, Vec<Chunk>>) -> Self {
        Self { chunks_by_task }
    }
}

impl Scheduler for IncrementalMockScheduler {
    fn schedule(&self, input: ScheduleInput) -> Result<ScheduleResult, AppError> {
        let mut placed = Vec::new();
        for task in &input.tasks {
            if let Some(chunks) = self.chunks_by_task.get(&task.id) {
                placed.extend(chunks.iter().cloned());
            }
        }
        Ok(ScheduleResult {
            placed_chunks: placed,
            warnings: Vec::new(),
        })
    }
}

fn run_incremental_single(
    store: &SqliteStore,
    now: DateTime<Utc>,
    task_id: &str,
    chunks: Vec<Chunk>,
) -> ScheduleResult {
    let scheduler = IncrementalMockScheduler::new(HashMap::from([(task_id.to_owned(), chunks)]));
    reschedule_incremental(store, &scheduler, &[task_id.to_owned()], now).unwrap()
}

fn run_empty_scheduler(initial_ids: &[&str]) -> (SqliteStore, ScheduleResult) {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let scheduler = IncrementalMockScheduler::new(HashMap::new());
    let ids: Vec<String> = initial_ids.iter().map(|&s| s.to_owned()).collect();
    let result = reschedule_incremental(&store, &scheduler, &ids, now).unwrap();
    (store, result)
}

#[test]
fn incremental_basic_single_task_placed() {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let task = make_task("task-1", TaskStatus::Pending);
    seed_task(&store, &task);

    let result = run_incremental_single(
        &store,
        now,
        "task-1",
        vec![make_auto_chunk("chunk-new-1", "task-1")],
    );

    assert_eq!(result.placed_chunks.len(), 1);
    assert!(store.get_chunk("chunk-new-1").unwrap().is_some());

    let updated = stored_task(&store, "task-1").unwrap();
    assert_eq!(updated.status, TaskStatus::Scheduled);
}

#[test_case(60, 30, 45, 75 ; "bumped_when_over_committed")]
#[test_case(120, 30, 30, 120 ; "not_bumped_when_under")]
fn incremental_duration_invariant(
    duration_minutes: i64,
    time_logged_minutes: i64,
    fixed_minutes: i64,
    expected_duration: i64,
) {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let mut task = make_task("task-1", TaskStatus::Pending);
    task.duration_minutes = duration_minutes;
    task.time_logged_minutes = time_logged_minutes;
    seed_task(&store, &task);

    let mut fixed = make_fixed_chunk("fixed-1", "task-1");
    fixed.start_time = now + Duration::hours(1);
    fixed.end_time = now + Duration::hours(1) + Duration::minutes(fixed_minutes);
    seed_chunk(&store, &fixed);

    let scheduler = IncrementalMockScheduler::new(HashMap::new());
    reschedule_incremental(&store, &scheduler, &["task-1".to_owned()], now).unwrap();

    let updated = stored_task(&store, "task-1").unwrap();
    assert_eq!(
        updated.duration_minutes, expected_duration,
        "duration invariant: expected {expected_duration} min"
    );
}

/// Verify that cascaded displacement produces an UPDATE, not a DELETE+CREATE.
/// The diff algorithm pairs each old chunk with the closest new chunk 1:1; with one
/// old and one new chunk for task-b the pairing is unambiguous → UPDATE of `b-chunk-old`
/// to the new times rather than deleting and inserting a fresh row.
#[test]
fn incremental_cascading_displacement_detected() {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let t = now + Duration::hours(1);

    let mut task_a = make_task("task-a", TaskStatus::Pending);
    task_a.priority = Priority::High;
    seed_task(&store, &task_a);

    let task_b = make_task("task-b", TaskStatus::Scheduled);
    seed_task(&store, &task_b);
    let b_old_chunk = make_chunk_at("b-chunk-old", "task-b", t, t + Duration::hours(1), None);
    seed_chunk(&store, &b_old_chunk);

    let a_new_chunk = make_chunk_at("a-chunk-new", "task-a", t, t + Duration::minutes(30), None);

    let b_new_start = t + Duration::hours(2);
    let b_new_end = t + Duration::hours(3);
    let b_new_chunk = make_chunk_at("b-chunk-new", "task-b", b_new_start, b_new_end, None);

    let scheduler = IncrementalMockScheduler::new(HashMap::from([
        ("task-a".to_owned(), vec![a_new_chunk]),
        ("task-b".to_owned(), vec![b_new_chunk]),
    ]));

    let result = reschedule_incremental(&store, &scheduler, &["task-a".to_owned()], now).unwrap();

    let placed_task_ids: Vec<&str> = result
        .placed_chunks
        .iter()
        .map(|c| c.task_id.as_str())
        .collect();
    assert!(
        placed_task_ids.contains(&"task-a"),
        "task-a chunk should be placed"
    );
    assert!(
        placed_task_ids.contains(&"task-b"),
        "task-b should have been cascaded and re-placed"
    );

    // The diff pairs b-chunk-old (at t) with b-new (at t+2h) → UPDATE.
    // b-chunk-old should exist but with the new start/end times.
    let stored_b = store.get_chunk("b-chunk-old").unwrap();
    assert!(
        stored_b.is_some(),
        "b-chunk-old should still exist (UPDATE)"
    );
    let stored_b = stored_b.unwrap();
    assert_eq!(
        stored_b.start_time, b_new_start,
        "b-chunk-old should have updated start time"
    );
    assert_eq!(
        stored_b.end_time, b_new_end,
        "b-chunk-old should have updated end time"
    );
}

#[test]
fn incremental_no_cascade_when_no_overlap() {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let t = now + Duration::hours(5);

    let task_a = make_task("task-a", TaskStatus::Pending);
    seed_task(&store, &task_a);

    // task-b has auto-chunk at a completely different time from task-a.
    let task_b = make_task("task-b", TaskStatus::Scheduled);
    seed_task(&store, &task_b);
    let b_chunk = make_chunk_at(
        "b-chunk",
        "task-b",
        t + Duration::hours(10),
        t + Duration::hours(11),
        None,
    );
    seed_chunk(&store, &b_chunk);

    let a_new_chunk = make_chunk_at("a-chunk-new", "task-a", t, t + Duration::hours(1), None);
    let result = run_incremental_single(&store, now, "task-a", vec![a_new_chunk]);

    let placed_task_ids: Vec<&str> = result
        .placed_chunks
        .iter()
        .map(|c| c.task_id.as_str())
        .collect();
    assert!(
        placed_task_ids.contains(&"task-a"),
        "task-a should be placed"
    );
    assert!(
        !placed_task_ids.contains(&"task-b"),
        "task-b should NOT be cascaded"
    );

    assert!(
        store.get_chunk("b-chunk").unwrap().is_some(),
        "task-b chunk should be untouched"
    );
}

#[test]
fn incremental_unaffected_task_without_existing_chunks_is_skipped_silently() {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let t = now + Duration::hours(1);

    let task_b = make_task("task-b", TaskStatus::Pending);
    seed_task(&store, &task_b);

    let task_a = make_task("task-a", TaskStatus::Pending);
    seed_task(&store, &task_a);
    let a_chunk = make_chunk_at("a-chunk", "task-a", t, t + Duration::hours(1), None);
    let result = run_incremental_single(&store, now, "task-a", vec![a_chunk]);

    assert_eq!(result.placed_chunks.len(), 1);
    assert_eq!(result.placed_chunks[0].task_id, "task-a");
    assert!(store.get_chunk("a-chunk").unwrap().is_some());
    let task_b_chunks = store.get_chunks_for_task("task-b").expect("no db err");
    assert!(task_b_chunks.is_empty(), "task-b must have no chunks");
    assert_eq!(
        stored_task(&store, "task-b").unwrap().status,
        TaskStatus::Pending,
        "task-b status must remain unchanged"
    );
}

/// Note: this test verifies slot consumption only *indirectly*. `IncrementalMockScheduler`
/// ignores `input.available_slots` entirely and returns pre-configured chunks — the two
/// assertions only confirm that task-a's mock chunk lands and task-b's chunk survives
/// untouched. Coverage of `subtract_intervals` itself lives in `scheduler::slot_finder::tests`.
#[test]
fn incremental_unaffected_tasks_consume_slots() {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let t = now + Duration::hours(1);

    let task_b = make_task("task-b", TaskStatus::Scheduled);
    seed_task(&store, &task_b);
    let b_chunk = make_chunk_at("b-chunk", "task-b", t, t + Duration::hours(1), None);
    seed_chunk(&store, &b_chunk);

    let task_a = make_task("task-a", TaskStatus::Pending);
    seed_task(&store, &task_a);

    let a_new_chunk = make_chunk_at(
        "a-chunk-new",
        "task-a",
        t + Duration::hours(2),
        t + Duration::hours(3),
        None,
    );
    let result = run_incremental_single(&store, now, "task-a", vec![a_new_chunk]);

    assert_eq!(result.placed_chunks.len(), 1);
    assert_eq!(result.placed_chunks[0].task_id, "task-a");

    assert!(store.get_chunk("b-chunk").unwrap().is_some());
}

#[test_case(TaskStatus::Pending, true, TaskStatus::Scheduled ; "pending_gets_chunks_becomes_scheduled")]
#[test_case(TaskStatus::Scheduled, false, TaskStatus::Pending ; "scheduled_loses_chunks_becomes_pending")]
fn incremental_status_transitions(
    initial_status: TaskStatus,
    place_chunk: bool,
    expected_status: TaskStatus,
) {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let mut task = make_task("task-1", initial_status);
    task.priority = Priority::Medium;
    seed_task(&store, &task);

    let chunks_for_task = if place_chunk {
        vec![make_auto_chunk("chunk-1", "task-1")]
    } else {
        Vec::new()
    };

    run_incremental_single(&store, now, "task-1", chunks_for_task);

    let updated = stored_task(&store, "task-1").unwrap();
    assert_eq!(
        updated.status, expected_status,
        "expected status {expected_status:?}, got {:?}",
        updated.status
    );
}

#[test]
fn incremental_config_sets_last_mutation_not_last_reschedule() {
    let store = test_store_with_config(default_config());
    let now = test_now();
    let task = make_task("task-1", TaskStatus::Pending);
    seed_task(&store, &task);

    let scheduler = IncrementalMockScheduler::new(HashMap::new());
    reschedule_incremental(&store, &scheduler, &["task-1".to_owned()], now).unwrap();

    let cfg = store.get_config().unwrap();
    assert_eq!(cfg.last_mutation, Some(now), "last_mutation should be set");
    assert!(
        cfg.last_reschedule.is_none(),
        "last_reschedule must NOT be set by incremental reschedule"
    );
}

#[test]
fn incremental_empty_initial_ids_returns_empty() {
    let (store, result) = run_empty_scheduler(&[]);

    assert!(result.placed_chunks.is_empty());
    assert!(result.warnings.is_empty());

    // Config should NOT be updated when there's nothing to do.
    let cfg = store.get_config().unwrap();
    assert!(cfg.last_mutation.is_none());
}

#[test_case("Not/ATimezone" ; "garbage_tz")]
#[test_case("Europe/Narnia" ; "fictional_tz")]
#[test_case("" ; "empty_tz")]
fn incremental_invalid_timezone_returns_error(tz: &str) {
    let cfg = AppConfig {
        timezone: tz.to_owned(),
        ..default_config()
    };
    let store = test_store_with_config(cfg);
    let scheduler = IncrementalMockScheduler::new(HashMap::new());
    let now = test_now();

    let err = reschedule_incremental(&store, &scheduler, &["task-1".to_owned()], now)
        .expect_err("should fail with invalid timezone");

    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation error, got: {err:?}"
    );
}

#[test]
fn incremental_deleted_task_skipped_gracefully() {
    // "ghost-task" never added to the store.
    let (_store, result) = run_empty_scheduler(&["ghost-task"]);

    // No error — just empty result.
    assert!(result.placed_chunks.is_empty());
}
