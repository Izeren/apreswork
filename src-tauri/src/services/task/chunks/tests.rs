// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for chunk placement (child module of `services::task::chunks`).

use chrono::{DateTime, Duration, TimeZone, Utc};
use test_case::test_case;

use super::{
    create_fixed_chunk, delete_fixed_chunk, get_task, lock_chunk, move_chunk, resize_chunk,
    unlock_chunk,
};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{ChunkStatus, TaskStatus};
use crate::domain::models::Chunk;
use crate::error::AppError;
use crate::services::task::test_helpers::{make_completed_chunk, make_scheduled_chunk, make_task};
use crate::test_support::{
    assert_not_found, assert_validation_contains, seed_chunk, seed_task, test_now, test_store,
};
use crate::traits::storage::ChunkStore;

fn make_chunk(id: &str, task_id: &str, is_fixed: bool, duration_min: i64) -> Chunk {
    let start = test_now();
    Chunk::test_default()
        .with_id(id)
        .with_task(task_id)
        .with_fixed(is_fixed)
        .with_times(start, start + Duration::minutes(duration_min))
}

/// The default 2026-03-15 18:00 window: `start` fixed, `end` = `start +
/// duration_min`. Shared by the `create_fixed_chunk`/`move_chunk` tests that
/// only vary the duration.
fn window(duration_min: i64) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 18, 0, 0).unwrap();
    (start, start + Duration::minutes(duration_min))
}

/// Seeds an unpinned `task-1` plus one 30-min `chunk-1` with the given fixed
/// flag, returning the store and the seeded chunk snapshot (callers read back
/// its `start_time`/`end_time`/`updated_at`).
fn store_task_and_chunk(is_fixed: bool) -> (SqliteStore, Chunk) {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));
    let chunk = make_chunk("chunk-1", "task-1", is_fixed, 30);
    seed_chunk(&store, &chunk);
    (store, chunk)
}

/// Seeds a Scheduled `task-1` with `time_logged` minutes logged and one
/// completed 30-min `chunk-1`. Shared by the completed-chunk move/lock/unlock
/// paths that reject or accept based on task/chunk state.
fn store_with_completed_chunk(time_logged: i64) -> SqliteStore {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    task.time_logged_minutes = time_logged;
    seed_task(&store, &task);
    seed_chunk(&store, &make_completed_chunk("chunk-1", "task-1", 30));
    store
}

/// Seeds an unpinned `task-1` plus one auto `chunk-1`, asserting the task
/// starts unpinned. Shared by the "marks task pinned" tests.
fn store_unpinned_with_auto_chunk() -> SqliteStore {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));
    seed_chunk(&store, &make_chunk("chunk-1", "task-1", false, 30));
    assert!(
        !get_task(&store, "task-1").unwrap().is_pinned,
        "precondition: unpinned"
    );
    store
}

/// Seeds a pinned `task-1` with two fixed chunks (`chunk-1`, `chunk-2`).
/// Shared by the "keeps pin while another fixed chunk remains" tests.
fn store_pinned_two_fixed_chunks() -> SqliteStore {
    let store = test_store();
    seed_task(&store, &make_task("task-1").with_pinned(true));
    seed_chunk(&store, &make_chunk("chunk-1", "task-1", true, 30));
    seed_chunk(&store, &make_chunk("chunk-2", "task-1", true, 30));
    store
}

/// Seeds a pinned `task-1` with one 30-min fixed `chunk-1`.
/// Shared by tests that need an already-pinned task with a single fixed chunk.
fn store_pinned_with_fixed_chunk() -> (SqliteStore, Chunk) {
    let store = test_store();
    seed_task(&store, &make_task("task-1").with_pinned(true));
    let chunk = make_chunk("chunk-1", "task-1", true, 30);
    seed_chunk(&store, &chunk);
    (store, chunk)
}

/// Seeds `task-1` with the given status. Shared by the tests that check how
/// `create_fixed_chunk` treats each non-default task status.
fn store_with_task_status(status: TaskStatus) -> SqliteStore {
    let store = test_store();
    let task = make_task("task-1").with_status(status);
    seed_task(&store, &task);
    store
}

#[test]
fn create_fixed_chunk_pending_transitions_to_scheduled() {
    let store = test_store();
    let task = make_task("task-1");
    assert_eq!(task.status, TaskStatus::Pending);
    seed_task(&store, &task);

    let (start, end) = window(30);
    let (chunk, updated_task) =
        create_fixed_chunk(&store, "task-1", start, end, test_now()).expect("should succeed");
    assert_eq!(chunk.task_id, "task-1");
    assert_eq!(chunk.start_time, start);
    assert_eq!(chunk.end_time, end);
    assert_eq!(chunk.status, ChunkStatus::Scheduled);
    assert!(chunk.is_fixed);
    assert!(chunk.logged_minutes.is_none());
    assert!(chunk.completed_at.is_none());
    assert!(chunk.google_event_id.is_none());

    assert!(store.get_chunk(&chunk.id).unwrap().is_some());
    assert_eq!(updated_task.status, TaskStatus::Scheduled);
}

#[test_case(TaskStatus::Backlog ; "backlog stays backlog")]
#[test_case(TaskStatus::Scheduled ; "scheduled stays scheduled")]
fn create_fixed_chunk_non_pending_status_preserved(status: TaskStatus) {
    let store = store_with_task_status(status);
    let (start, end) = window(30);

    let (chunk, returned_task) =
        create_fixed_chunk(&store, "task-1", start, end, test_now()).expect("should succeed");
    assert!(chunk.is_fixed);

    assert_eq!(returned_task.status, status);
}

#[test]
fn create_fixed_chunk_over_allocation_rejected() {
    let store = test_store();
    let task = make_task("task-1"); // 60 min duration
    seed_task(&store, &task);

    // Pre-existing 30 min fixed chunk
    let existing = make_chunk("chunk-1", "task-1", true, 30);
    seed_chunk(&store, &existing);

    // Try to create 40 min chunk → 30 + 40 = 70 > 60
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 19, 0, 0).unwrap();
    let end = start + Duration::minutes(40);

    let result = create_fixed_chunk(&store, "task-1", start, end, test_now());
    assert_validation_contains(&result, "exceeds");
}

#[test]
fn create_fixed_chunk_exact_allocation_succeeds() {
    let store = test_store();
    let task = make_task("task-1"); // 60 min duration
    seed_task(&store, &task);

    // Create exactly 60 min chunk
    let (start, end) = window(60);
    let (chunk, _task) =
        create_fixed_chunk(&store, "task-1", start, end, test_now()).expect("should succeed");
    assert_eq!((chunk.end_time - chunk.start_time).num_minutes(), 60);
}

#[test]
fn create_fixed_chunk_partial_allocation_fills_remaining() {
    let store = test_store();
    let task = make_task("task-1"); // 60 min duration
    seed_task(&store, &task);

    // Pre-existing 20 min fixed chunk
    let existing = make_chunk("chunk-1", "task-1", true, 20);
    seed_chunk(&store, &existing);

    // Create 40 min chunk → 20 + 40 = 60 exactly
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 19, 0, 0).unwrap();
    let end = start + Duration::minutes(40);

    let (chunk, _task) =
        create_fixed_chunk(&store, "task-1", start, end, test_now()).expect("should succeed");
    assert_eq!((chunk.end_time - chunk.start_time).num_minutes(), 40);
}

#[test]
fn create_fixed_chunk_task_not_found() {
    let store = test_store();
    let (start, end) = window(30);

    let result = create_fixed_chunk(&store, "nonexistent", start, end, test_now());
    assert_not_found(&result, "Task", "nonexistent");
}

#[test_case(TaskStatus::Completed ; "completed task rejected")]
#[test_case(TaskStatus::Cancelled ; "cancelled task rejected")]
fn create_fixed_chunk_terminal_status_rejected(status: TaskStatus) {
    let store = store_with_task_status(status);
    let (start, end) = window(30);

    let result = create_fixed_chunk(&store, "task-1", start, end, test_now());
    assert_validation_contains(&result, "status");
}

#[test_case(0 ; "equals end")]
#[test_case(60 ; "after end")]
fn create_fixed_chunk_start_end_rejected(offset_min: i64) {
    let store = test_store();
    let task = make_task("task-1");
    seed_task(&store, &task);

    let end = Utc.with_ymd_and_hms(2026, 3, 15, 18, 0, 0).unwrap();
    let start = end + Duration::minutes(offset_min);

    let result = create_fixed_chunk(&store, "task-1", start, end, test_now());
    assert_validation_contains(&result, "before");
}

#[test]
fn create_fixed_chunk_auto_chunks_excluded_from_allocation() {
    let store = test_store();
    let task = make_task("task-1"); // 60 min duration
    seed_task(&store, &task);

    // Pre-existing 30 min AUTO chunk (should not count towards allocation)
    let auto_chunk = make_chunk("chunk-auto", "task-1", false, 30);
    seed_chunk(&store, &auto_chunk);

    // Create 60 min fixed chunk → only fixed allocation = 0, so 60 <= 60
    let (start, end) = window(60);
    let (chunk, _task) =
        create_fixed_chunk(&store, "task-1", start, end, test_now()).expect("should succeed");
    assert!(chunk.is_fixed);
    assert_eq!((chunk.end_time - chunk.start_time).num_minutes(), 60);
}

#[test]
fn create_fixed_chunk_time_logged_reduces_remaining() {
    let store = test_store();
    let mut task = make_task("task-1"); // 60 min duration
    task.time_logged_minutes = 20;
    seed_task(&store, &task);

    // remaining = 60 - 20 - 0 = 40 min
    // Try to create 50 min chunk → exceeds remaining
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 18, 0, 0).unwrap();
    let end = start + Duration::minutes(50);

    let result = create_fixed_chunk(&store, "task-1", start, end, test_now());
    assert_validation_contains(&result, "exceeds");
}

#[test]
fn create_fixed_chunk_timestamps_use_injected_now() {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));
    let (start, end) = window(30);
    let (chunk, _) =
        create_fixed_chunk(&store, "task-1", start, end, test_now()).expect("should succeed");
    assert_eq!(chunk.created_at, test_now());
    assert_eq!(chunk.updated_at, test_now());
}

fn move_and_assert(store: &SqliteStore, new_start: DateTime<Utc>, new_end: DateTime<Utc>) -> Chunk {
    let moved =
        move_chunk(store, "chunk-1", new_start, new_end, test_now()).expect("should succeed");
    assert_eq!(moved.start_time, new_start);
    assert_eq!(moved.end_time, new_end);
    assert!(moved.is_fixed);
    moved
}

#[test]
fn move_chunk_auto_chunk_becomes_fixed() {
    let (store, chunk) = store_task_and_chunk(false);
    assert!(!chunk.is_fixed);

    let new_start = Utc.with_ymd_and_hms(2026, 3, 16, 10, 0, 0).unwrap();
    let new_end = new_start + Duration::minutes(45);
    let moved = move_and_assert(&store, new_start, new_end);
    assert!(moved.updated_at > chunk.updated_at);

    let persisted = store.get_chunk("chunk-1").unwrap().unwrap();
    assert_eq!(persisted.start_time, new_start);
    assert!(persisted.is_fixed);
}

#[test]
fn move_chunk_fixed_stays_fixed() {
    let (store, chunk) = store_task_and_chunk(true);
    assert!(chunk.is_fixed);

    let new_start = Utc.with_ymd_and_hms(2026, 3, 16, 10, 0, 0).unwrap();
    let new_end = new_start + Duration::minutes(20);
    move_and_assert(&store, new_start, new_end);
}

#[test]
fn move_chunk_completed_chunk_works() {
    let store = store_with_completed_chunk(30);

    let new_start = Utc.with_ymd_and_hms(2026, 3, 17, 8, 0, 0).unwrap();
    let new_end = new_start + Duration::minutes(30);
    let moved = move_and_assert(&store, new_start, new_end);
    assert_eq!(moved.status, ChunkStatus::Completed);
}

#[test]
fn move_chunk_not_found() {
    let store = test_store();
    let (start, end) = window(30);

    let result = move_chunk(&store, "nonexistent", start, end, test_now());
    assert_not_found(&result, "Chunk", "nonexistent");
}

#[test_case(0 ; "equals end")]
#[test_case(60 ; "after end")]
fn move_chunk_start_end_rejected(offset_min: i64) {
    let (store, _chunk) = store_task_and_chunk(false);

    let end = Utc.with_ymd_and_hms(2026, 3, 16, 10, 0, 0).unwrap();
    let start = end + Duration::minutes(offset_min);

    let result = move_chunk(&store, "chunk-1", start, end, test_now());
    assert_validation_contains(&result, "before");
}

#[test]
fn resize_chunk_scheduled_happy_path() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    task.duration_minutes = 120;
    seed_task(&store, &task);

    let chunk = make_scheduled_chunk("chunk-1", "task-1", 30);
    seed_chunk(&store, &chunk);

    let new_end = chunk.start_time + Duration::minutes(45);

    let (resized, returned_task) =
        resize_chunk(&store, "chunk-1", new_end, test_now()).expect("should succeed");

    assert_eq!(resized.end_time, new_end);
    assert!(resized.is_fixed);
    // Task should be unchanged for scheduled chunks
    assert_eq!(returned_task.time_logged_minutes, 0);
    assert_eq!(returned_task.duration_minutes, 120);
}

// Completed-chunk resize adjusts the task budget by (new_duration - old_logged):
// (prior task-logged, new chunk duration, expected chunk logged, expected task logged).
#[test_case(30, 45, 45, 45 ; "grow 30→45 adds 15")]
#[test_case(30, 20, 20, 20 ; "shrink 30→20 removes 10")]
#[test_case(60, 50, 50, 80 ; "grow 30→50 adds 20 over prior 60")]
fn resize_chunk_completed_budget_adjustment(
    task_logged: i64,
    new_dur: i64,
    expected_chunk_logged: i64,
    expected_task_logged: i64,
) {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    task.duration_minutes = 120;
    task.time_logged_minutes = task_logged;
    seed_task(&store, &task);

    let chunk = make_completed_chunk("chunk-1", "task-1", 30);
    seed_chunk(&store, &chunk);

    let new_end = chunk.start_time + Duration::minutes(new_dur);
    let (resized, returned_task) =
        resize_chunk(&store, "chunk-1", new_end, test_now()).expect("should succeed");

    assert_eq!(resized.logged_minutes, Some(expected_chunk_logged));
    assert_eq!(returned_task.time_logged_minutes, expected_task_logged);
}

#[test]
fn resize_chunk_not_found() {
    let store = test_store();
    let new_end = Utc.with_ymd_and_hms(2026, 3, 15, 19, 0, 0).unwrap();

    let result = resize_chunk(&store, "nonexistent", new_end, test_now());
    assert_not_found(&result, "Chunk", "nonexistent");
}

#[test_case(0 ; "equals start")]
#[test_case(-10 ; "before start")]
fn resize_chunk_new_end_invalid(offset_min: i64) {
    let store = test_store();
    let task = make_task("task-1");
    seed_task(&store, &task);

    let chunk = make_scheduled_chunk("chunk-1", "task-1", 30);
    seed_chunk(&store, &chunk);

    let new_end = chunk.start_time + Duration::minutes(offset_min);

    let result = resize_chunk(&store, "chunk-1", new_end, test_now());
    assert_validation_contains(&result, "after");
}

#[test]
fn resize_chunk_completed_none_logged_minutes_treated_as_zero() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    task.duration_minutes = 120;
    task.time_logged_minutes = 50;
    seed_task(&store, &task);

    // Completed chunk with logged_minutes = None (edge case)
    let start = Utc.with_ymd_and_hms(2026, 3, 15, 18, 0, 0).unwrap();
    let mut chunk = make_completed_chunk("chunk-1", "task-1", 30);
    chunk.logged_minutes = None;
    chunk.start_time = start;
    chunk.end_time = start + Duration::minutes(30);
    seed_chunk(&store, &chunk);

    // Resize to 40 min → new_duration=40, old_logged=0, delta=40
    let new_end = start + Duration::minutes(40);
    let (resized, returned_task) =
        resize_chunk(&store, "chunk-1", new_end, test_now()).expect("should succeed");

    assert_eq!(resized.logged_minutes, Some(40));
    assert_eq!(returned_task.time_logged_minutes, 90); // 50 + 40
}

#[test]
fn move_chunk_marks_task_pinned() {
    let store = store_unpinned_with_auto_chunk();

    let new_start = Utc.with_ymd_and_hms(2026, 3, 16, 10, 0, 0).unwrap();
    move_chunk(
        &store,
        "chunk-1",
        new_start,
        new_start + Duration::minutes(30),
        test_now(),
    )
    .expect("should succeed");

    assert!(
        get_task(&store, "task-1").unwrap().is_pinned,
        "moving a chunk pins its task"
    );
}

#[test]
fn resize_chunk_marks_task_pinned() {
    let (store, chunk) = store_task_and_chunk(false);

    resize_chunk(
        &store,
        "chunk-1",
        chunk.start_time + Duration::minutes(45),
        test_now(),
    )
    .expect("should succeed");

    assert!(
        get_task(&store, "task-1").unwrap().is_pinned,
        "resizing a chunk pins its task"
    );
}

#[test]
fn resize_chunk_completed_pins_task() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    task.duration_minutes = 120;
    task.time_logged_minutes = 30;
    seed_task(&store, &task);

    let mut chunk = make_completed_chunk("chunk-1", "task-1", 30);
    chunk.is_fixed = false;
    seed_chunk(&store, &chunk);

    assert!(
        !get_task(&store, "task-1").unwrap().is_pinned,
        "precondition: unpinned"
    );

    let new_end = chunk.start_time + Duration::minutes(45);
    resize_chunk(&store, "chunk-1", new_end, test_now()).expect("should succeed");

    assert!(
        get_task(&store, "task-1").unwrap().is_pinned,
        "resizing a completed chunk pins its task"
    );
}

#[test]
fn resize_chunk_already_pinned_task_skips_redundant_write() {
    // Resizing a non-completed chunk on an already-pinned task leaves
    // is_pinned set and writes nothing extra to the task row.
    let (store, chunk) = store_pinned_with_fixed_chunk();
    let before = get_task(&store, "task-1").unwrap();

    resize_chunk(
        &store,
        "chunk-1",
        chunk.start_time + Duration::minutes(45),
        test_now(),
    )
    .expect("should succeed");

    let after = get_task(&store, "task-1").unwrap();
    assert!(after.is_pinned, "stays pinned");
    assert_eq!(
        after.updated_at, before.updated_at,
        "no redundant task write"
    );
}

#[test]
fn unlock_chunk_clears_pin_when_no_fixed_chunk_remains() {
    let store = test_store();
    seed_task(&store, &make_task("task-1").with_pinned(true));
    seed_chunk(&store, &make_chunk("chunk-1", "task-1", true, 30));

    unlock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(
        !get_task(&store, "task-1").unwrap().is_pinned,
        "unlocking the only fixed chunk unpins the task"
    );
}

#[test]
fn unlock_chunk_keeps_pin_when_another_fixed_chunk_remains() {
    let store = store_pinned_two_fixed_chunks();

    unlock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(
        get_task(&store, "task-1").unwrap().is_pinned,
        "task stays pinned while another fixed chunk remains"
    );
}

#[test]
fn lock_chunk_auto_scheduled_becomes_fixed() {
    let (store, chunk) = store_task_and_chunk(false);
    assert!(!chunk.is_fixed);

    let locked = lock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(locked.is_fixed);
    assert_eq!(locked.start_time, chunk.start_time, "times untouched");
    assert_eq!(locked.end_time, chunk.end_time, "times untouched");
    assert!(locked.updated_at > chunk.updated_at);

    let persisted = store.get_chunk("chunk-1").unwrap().unwrap();
    assert!(persisted.is_fixed);
}

#[test]
fn lock_chunk_already_fixed_stays_fixed() {
    let (store, _chunk) = store_pinned_with_fixed_chunk();

    let locked = lock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(locked.is_fixed);
    assert_eq!(locked.status, ChunkStatus::Scheduled);
}

fn lock_completed(store: &SqliteStore) -> Result<Chunk, AppError> {
    lock_chunk(store, "chunk-1", test_now())
}

fn unlock_completed(store: &SqliteStore) -> Result<Chunk, AppError> {
    unlock_chunk(store, "chunk-1", test_now())
}

#[test_case(lock_completed, "completed chunks cannot be locked" ; "lock rejected")]
#[test_case(unlock_completed, "completed chunks cannot be unlocked" ; "unlock rejected")]
fn completed_chunk_operation_rejected(
    op: fn(&SqliteStore) -> Result<Chunk, AppError>,
    expected_msg: &str,
) {
    let store = store_with_completed_chunk(30);
    let result = op(&store);
    assert_validation_contains(&result, expected_msg);
}

#[test]
fn lock_chunk_not_found() {
    let store = test_store();

    let result = lock_chunk(&store, "nonexistent", test_now());
    assert_not_found(&result, "Chunk", "nonexistent");
}

#[test]
fn lock_chunk_marks_task_pinned() {
    let store = store_unpinned_with_auto_chunk();

    lock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(
        get_task(&store, "task-1").unwrap().is_pinned,
        "locking a chunk pins its task"
    );
}

#[test]
fn unlock_chunk_fixed_scheduled_becomes_auto() {
    let (store, chunk) = store_task_and_chunk(true);
    assert!(chunk.is_fixed);

    let unlocked = unlock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(!unlocked.is_fixed);
    assert!(unlocked.updated_at > chunk.updated_at);

    let persisted = store.get_chunk("chunk-1").unwrap().unwrap();
    assert!(!persisted.is_fixed);
}

#[test]
fn unlock_chunk_already_auto_stays_auto() {
    let (store, chunk) = store_task_and_chunk(false);
    assert!(!chunk.is_fixed);

    let unlocked = unlock_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(!unlocked.is_fixed);
    assert_eq!(unlocked.status, ChunkStatus::Scheduled);
}

#[test]
fn unlock_chunk_not_found() {
    let store = test_store();

    let result = unlock_chunk(&store, "nonexistent", test_now());
    assert_not_found(&result, "Chunk", "nonexistent");
}

#[test]
fn delete_fixed_chunk_removes_chunk_and_unpins_task() {
    let store = test_store();
    seed_task(&store, &make_task("task-1").with_pinned(true));
    seed_chunk(&store, &make_chunk("chunk-1", "task-1", true, 30));

    let deleted = delete_fixed_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert_eq!(deleted.id, "chunk-1");
    assert_eq!(deleted.task_id, "task-1");
    assert!(
        store.get_chunk("chunk-1").unwrap().is_none(),
        "chunk removed from store"
    );
    assert!(
        !get_task(&store, "task-1").unwrap().is_pinned,
        "deleting the only fixed chunk unpins the task"
    );
}

#[test]
fn delete_fixed_chunk_keeps_pin_when_another_fixed_chunk_remains() {
    let store = store_pinned_two_fixed_chunks();

    delete_fixed_chunk(&store, "chunk-1", test_now()).expect("should succeed");

    assert!(
        store.get_chunk("chunk-1").unwrap().is_none(),
        "chunk removed from store"
    );
    assert!(
        get_task(&store, "task-1").unwrap().is_pinned,
        "task stays pinned while another fixed chunk remains"
    );
}

#[test]
fn delete_fixed_chunk_auto_chunk_rejected() {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));
    seed_chunk(&store, &make_chunk("chunk-1", "task-1", false, 30));

    let result = delete_fixed_chunk(&store, "chunk-1", test_now());
    assert_validation_contains(&result, "only fixed chunks can be deleted");
    assert!(
        store.get_chunk("chunk-1").unwrap().is_some(),
        "auto chunk untouched"
    );
}

#[test]
fn delete_fixed_chunk_completed_rejected() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    task.time_logged_minutes = 30;
    seed_task(&store, &task);

    let mut chunk = make_completed_chunk("chunk-1", "task-1", 30);
    chunk.is_fixed = true;
    seed_chunk(&store, &chunk);

    let result = delete_fixed_chunk(&store, "chunk-1", test_now());
    assert_validation_contains(&result, "completed chunks cannot be deleted");
    assert!(
        store.get_chunk("chunk-1").unwrap().is_some(),
        "completed chunk untouched"
    );
}

#[test]
fn delete_fixed_chunk_not_found() {
    let store = test_store();

    let result = delete_fixed_chunk(&store, "nonexistent", test_now());
    assert_not_found(&result, "Chunk", "nonexistent");
}
