// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the stale fixed-chunk lock release pass.

use chrono::Duration;
use test_case::test_case;

use super::{make_fixed_chunk_at, make_task, MockScheduler};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{ChunkStatus, TaskStatus};
use crate::services::scheduling::{release_stale_fixed_locks, reschedule, reschedule_incremental};
use crate::test_support::{
    default_config, seed_chunk, seed_task, test_now, test_store_with_config,
};
use crate::traits::storage::{ChunkStore, TaskStore};

fn store_with_task_and_fixed_chunk_at(end_offset: Duration) -> (SqliteStore, String, String) {
    let store = test_store_with_config(default_config());
    let now = test_now();

    let task = make_task("task-1", TaskStatus::Scheduled);
    seed_task(&store, &task);

    let start = now + end_offset - Duration::hours(1);
    let end = now + end_offset;
    let chunk = make_fixed_chunk_at("chunk-1", "task-1", start, end);
    seed_chunk(&store, &chunk);

    (store, task.id, chunk.id)
}

// Stale (>4h) → unlocked; boundary (==4h) → stays locked (strict <); fresh → stays locked.
#[test_case(Duration::hours(-5), false ; "stale_5h_unlocked")]
#[test_case(Duration::hours(-4), true  ; "boundary_exactly_4h_stays_locked")]
#[test_case(Duration::hours(-3), true  ; "recent_3h_stays_locked")]
fn release_stale_fixed_locks_is_fixed_after_release(end_offset: Duration, expected_fixed: bool) {
    let (store, _, chunk_id) = store_with_task_and_fixed_chunk_at(end_offset);
    let now = test_now();

    release_stale_fixed_locks(&store, now).expect("release_stale_fixed_locks");

    let chunk = store.get_chunk(&chunk_id).unwrap().expect("chunk exists");
    assert_eq!(chunk.is_fixed, expected_fixed);
}

#[test]
fn release_stale_fixed_locks_updates_timestamp_when_unlocked() {
    let (store, _, chunk_id) = store_with_task_and_fixed_chunk_at(Duration::hours(-5));
    let now = test_now();

    release_stale_fixed_locks(&store, now).expect("release_stale_fixed_locks");

    let chunk = store.get_chunk(&chunk_id).unwrap().expect("chunk exists");
    assert_eq!(
        chunk.updated_at, now,
        "updated_at must be set to now on unlock"
    );
}

#[test]
fn release_stale_fixed_locks_does_not_touch_completed_chunks() {
    let store = test_store_with_config(default_config());
    let now = test_now();

    let task = make_task("task-1", TaskStatus::Scheduled);
    seed_task(&store, &task);

    // Completed chunk ending 5h ago — must never be unlocked.
    let start = now - Duration::hours(6);
    let end = now - Duration::hours(5);
    let mut chunk = make_fixed_chunk_at("chunk-1", "task-1", start, end);
    chunk.status = ChunkStatus::Completed;
    chunk.completed_at = Some(end);
    seed_chunk(&store, &chunk);

    release_stale_fixed_locks(&store, now).expect("release_stale_fixed_locks");

    let loaded = store.get_chunk(&chunk.id).unwrap().expect("chunk exists");
    assert!(loaded.is_fixed, "completed chunks must not be unlocked");
}

#[test]
fn reschedule_releases_stale_lock_and_reverts_task_to_pending() {
    let (store, task_id, _) = store_with_task_and_fixed_chunk_at(Duration::hours(-5));
    let now = test_now();

    reschedule(&store, &MockScheduler::empty(), now).expect("reschedule");

    let task = store.get_task(&task_id).unwrap().expect("task exists");
    assert_eq!(
        task.status,
        TaskStatus::Pending,
        "task with stale fixed chunk should revert to Pending after reschedule"
    );
}

#[test]
fn reschedule_keeps_fresh_fixed_chunk_and_task_scheduled() {
    let (store, task_id, chunk_id) = store_with_task_and_fixed_chunk_at(Duration::hours(-3));
    let now = test_now();

    reschedule(&store, &MockScheduler::empty(), now).expect("reschedule");

    let task = store.get_task(&task_id).unwrap().expect("task exists");
    assert_eq!(
        task.status,
        TaskStatus::Scheduled,
        "task with fresh fixed chunk should stay Scheduled after reschedule"
    );

    let chunk = store.get_chunk(&chunk_id).unwrap().expect("chunk exists");
    assert!(chunk.is_fixed, "fresh fixed chunk should retain its lock");
}

#[test]
fn release_stale_fixed_locks_syncs_is_pinned_to_false() {
    let store = test_store_with_config(default_config());
    let now = test_now();

    let mut task = make_task("task-1", TaskStatus::Scheduled);
    task.is_pinned = true;
    seed_task(&store, &task);

    let start = now - Duration::hours(6);
    let end = now - Duration::hours(5);
    seed_chunk(
        &store,
        &make_fixed_chunk_at("chunk-1", "task-1", start, end),
    );

    release_stale_fixed_locks(&store, now).expect("release_stale_fixed_locks");

    let updated = store.get_task("task-1").unwrap().expect("task exists");
    assert!(
        !updated.is_pinned,
        "is_pinned must be false after sole fixed chunk is unlocked"
    );
}

#[test]
fn reschedule_incremental_releases_stale_lock_and_does_not_inflate_duration() {
    let store = test_store_with_config(default_config());
    let now = test_now();

    // 90-min stale chunk on a 60-min task. If counted as fixed, the duration-fix
    // loop would inflate duration_minutes to 90 (total_committed=90 > 60). The
    // assertion below is discriminating: it fails when the release runs *after*
    // the duration-fix loop, or when the release is absent entirely.
    let mut task = make_task("task-1", TaskStatus::Scheduled);
    task.duration_minutes = 60;
    task.is_pinned = true;
    seed_task(&store, &task);

    let end = now - Duration::hours(5);
    let start = end - Duration::minutes(90);
    let chunk = make_fixed_chunk_at("chunk-1", "task-1", start, end);
    seed_chunk(&store, &chunk);

    reschedule_incremental(&store, &MockScheduler::empty(), &["task-1".to_owned()], now)
        .expect("reschedule_incremental");

    // The stale chunk is unlocked (is_fixed → false) then deleted by apply_diff_ops
    // because the scheduler placed nothing for the now-auto chunk. Its absence proves
    // the lock was released (a live fixed chunk would have been kept by the diff).
    assert!(
        store.get_chunk("chunk-1").unwrap().is_none(),
        "unlocked stale chunk must be removed by incremental reschedule when scheduler places nothing"
    );

    let task_after = store.get_task("task-1").unwrap().expect("task exists");
    assert_eq!(
        task_after.duration_minutes, 60,
        "duration_minutes must not be inflated by the released stale chunk"
    );
    // Scheduler placed nothing → task reverts to Pending, confirming the full flow ran.
    assert_eq!(
        task_after.status,
        TaskStatus::Pending,
        "task must revert to Pending when all chunks are gone"
    );
}
