// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Task lifecycle — completing, reopening, and cancelling work.

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::domain::enums::{ChunkStatus, TaskStatus};
use crate::domain::models::{Chunk, Task};
use crate::error::AppError;
use crate::services::comment::{chunk_completed_content, chunk_reopened_content, system_comment};
use crate::traits::storage::Store;

use super::{get_task, require_chunk};

/// Mark a scheduled chunk as completed and log its time against the task.
///
/// A non-fixed chunk's window is re-anchored to `[now - logged, now]` so a
/// completed chunk records when the work was actually done, not wherever the
/// last reschedule parked it — and because completed chunks are immovable,
/// that anchor survives later reschedules. A fixed (pinned) chunk completes
/// in place: the user chose its window, so only `completed_at` records the
/// completion moment. Only the chunk's `start_time`/`end_time` ever move; the
/// task's `start_date`/`deadline` are never touched. `logged` is
/// `duration_override` when supplied, otherwise the chunk's planned length.
/// Reaching `duration_minutes` auto-completes the task. A `SYSTEM` progress
/// comment is recorded in the same transaction (M12.5).
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the chunk or its task does not exist.
/// Returns [`AppError::Validation`] if the chunk is not in `Scheduled` status or
/// the resolved logged duration is negative.
/// Returns [`AppError::Database`] on storage failure.
pub fn complete_chunk(
    store: &dyn Store,
    chunk_id: &str,
    duration_override: Option<i64>,
    now: DateTime<Utc>,
) -> Result<(Chunk, Task), AppError> {
    let mut chunk = require_chunk(store, chunk_id)?;

    if chunk.status != ChunkStatus::Scheduled {
        return Err(AppError::Validation("chunk is already completed".into()));
    }

    let mut task = get_task(store, &chunk.task_id)?;

    let logged =
        duration_override.unwrap_or_else(|| (chunk.end_time - chunk.start_time).num_minutes());

    if logged < 0 {
        return Err(AppError::Validation(
            "duration_override cannot be negative".into(),
        ));
    }

    chunk.status = ChunkStatus::Completed;
    chunk.completed_at = Some(now);
    chunk.logged_minutes = Some(logged);
    // Anchor the window to the completion moment (length = logged minutes) —
    // unless the chunk is pinned: a fixed chunk completes in place.
    if !chunk.is_fixed {
        chunk.end_time = now;
        chunk.start_time = now - Duration::minutes(logged);
    }
    chunk.updated_at = now;

    task.time_logged_minutes += logged;
    if task.time_logged_minutes >= task.duration_minutes {
        task.status = TaskStatus::Completed;
    }
    task.updated_at = now;

    store.with_tx(&mut |tx| {
        tx.update_chunk(&chunk)?;
        tx.update_task(&task)?;
        tx.create_comment(&system_comment(
            &task.id,
            chunk_completed_content(logged, task.time_logged_minutes, task.duration_minutes),
            now,
        ))
    })?;

    // TODO(1.8.1): set config.last_mutation

    Ok((chunk, task))
}

/// Complete a task by collapsing its remaining scheduled chunks into a single
/// completed chunk that logs the task's outstanding time.
///
/// The per-chunk schedule split is a *plan*, not history: completing the whole
/// task at once means it was not worked in those separate slices, so the
/// remaining scheduled chunks are merged into one — the earliest is kept and,
/// when non-fixed, anchored to `[now - remaining, now]` (a fixed survivor
/// completes in place, see [`complete_chunk`]); the rest are deleted.
/// Already-completed chunks are left untouched as real history.
///
/// A task with no scheduled chunks at all (e.g. a backlog item finished
/// outside the planner) completes the same way: a chunk covering the
/// outstanding time is synthesized at `[now - remaining, now]` and completed
/// through [`complete_chunk`], so logged time, the SYSTEM comment, and
/// reopenability all follow the one completion policy. The whole collapse —
/// deletions or synthesis plus the completion — runs in one transaction.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the task does not exist.
/// Returns [`AppError::Validation`] if the task is already completed/cancelled.
/// Returns any error produced while completing the surviving chunk (see
/// [`complete_chunk`]).
pub fn complete_task(
    store: &dyn Store,
    task_id: &str,
    now: DateTime<Utc>,
) -> Result<Task, AppError> {
    let task = get_task(store, task_id)?;

    if matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled) {
        return Err(AppError::Validation(
            "task is already completed or cancelled".into(),
        ));
    }

    let mut scheduled_chunks: Vec<Chunk> = store
        .get_chunks_for_task(task_id)?
        .into_iter()
        .filter(|chunk| chunk.status == ChunkStatus::Scheduled)
        .collect();

    let remaining = (task.duration_minutes - task.time_logged_minutes).max(0);

    store.with_tx(&mut |tx| {
        if scheduled_chunks.is_empty() {
            // Nothing planned to collapse: synthesize the chunk that records
            // the outstanding time, then complete it through the normal chunk
            // path.
            let chunk = Chunk {
                id: Uuid::now_v7().to_string(),
                task_id: task_id.to_owned(),
                start_time: now - Duration::minutes(remaining),
                end_time: now,
                status: ChunkStatus::Scheduled,
                is_fixed: false,
                logged_minutes: None,
                completed_at: None,
                google_event_id: None,
                created_at: now,
                updated_at: now,
            };
            tx.create_chunk(&chunk)?;
            complete_chunk(tx, &chunk.id, Some(remaining), now)?;
        } else {
            scheduled_chunks.sort_by_key(|chunk| chunk.start_time);

            // Keep the earliest scheduled chunk as the survivor and delete the
            // rest, so the task collapses to one completed chunk holding all
            // outstanding time.
            for chunk in &scheduled_chunks[1..] {
                tx.delete_chunk(&chunk.id)?;
            }
            complete_chunk(tx, &scheduled_chunks[0].id, Some(remaining), now)?;
        }
        Ok(())
    })?;

    // The transaction closure cannot return a value; re-read committed state.
    get_task(store, task_id)
}

/// Reopen a completed chunk, reverting it to `Scheduled` and subtracting its
/// logged time from the task.
///
/// If the task was `Completed`, it is reverted to `Scheduled`. A `SYSTEM`
/// progress comment is recorded in the same transaction (M12.5).
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the chunk does not exist.
/// Returns [`AppError::Validation`] if the chunk is not in `Completed` status.
/// Returns [`AppError::NotFound`] if the associated task does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn reopen_chunk(
    store: &dyn Store,
    chunk_id: &str,
    now: DateTime<Utc>,
) -> Result<(Chunk, Task), AppError> {
    let mut chunk = require_chunk(store, chunk_id)?;

    if chunk.status != ChunkStatus::Completed {
        return Err(AppError::Validation("chunk is not completed".into()));
    }

    let mut task = get_task(store, &chunk.task_id)?;

    let logged = chunk.logged_minutes.unwrap_or(0);
    let prior_logged = task.time_logged_minutes;
    task.time_logged_minutes = (task.time_logged_minutes - logged).max(0);
    // What actually left the running total (differs from `logged` only when
    // the subtraction clamped at zero) — this is what the comment reports.
    let subtracted = prior_logged - task.time_logged_minutes;

    chunk.status = ChunkStatus::Scheduled;
    chunk.completed_at = None;
    chunk.logged_minutes = None;

    if task.status == TaskStatus::Completed {
        task.status = TaskStatus::Scheduled;
    }

    chunk.updated_at = now;
    task.updated_at = now;

    store.with_tx(&mut |tx| {
        tx.update_chunk(&chunk)?;
        tx.update_task(&task)?;
        tx.create_comment(&system_comment(
            &task.id,
            chunk_reopened_content(subtracted, task.time_logged_minutes, task.duration_minutes),
            now,
        ))
    })?;

    // TODO(1.8.1): set config.last_mutation

    Ok((chunk, task))
}

/// Cancel a task and delete its scheduled (non-completed) chunks.
///
/// Completed chunks are kept as history. The task is set to `Cancelled`.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the task does not exist.
/// Returns [`AppError::Validation`] if the task is already `Completed` or
/// `Cancelled`.
/// Returns [`AppError::Database`] on storage failure.
pub fn cancel_task(store: &dyn Store, task_id: &str, now: DateTime<Utc>) -> Result<Task, AppError> {
    let mut task = get_task(store, task_id)?;

    if matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled) {
        return Err(AppError::Validation(format!(
            "cannot cancel task with status {:?}",
            task.status
        )));
    }

    let chunks_to_delete: Vec<String> = store
        .get_chunks_for_task(task_id)?
        .into_iter()
        .filter(|chunk| chunk.status == ChunkStatus::Scheduled)
        .map(|chunk| chunk.id)
        .collect();

    task.status = TaskStatus::Cancelled;
    super::stamp_and_persist(store, &mut task, &chunks_to_delete, now)?;

    // TODO(1.8.1): set config.last_mutation

    Ok(task)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use test_case::test_case;

    use super::{cancel_task, complete_chunk, complete_task, reopen_chunk};
    use crate::db::sqlite::SqliteStore;
    use crate::domain::enums::{ChunkStatus, TaskStatus};
    use crate::domain::models::{Chunk, Comment, Task};
    use crate::error::AppError;
    use crate::services::comment::SYSTEM_AUTHOR;
    use crate::services::task::test_helpers::{
        make_completed_chunk, make_scheduled_chunk, make_task,
    };
    use crate::test_support::{seed_chunk, seed_task, test_now, test_store, utc};
    use crate::traits::storage::{ChunkStore, CommentStore};

    /// A fresh store seeded with task "task-1" in `status`, `duration` minutes
    /// long with `logged` minutes already logged — the setup every lifecycle
    /// test starts from. Chunks are seeded per-test on top.
    fn store_with_task(status: TaskStatus, duration: i64, logged: i64) -> SqliteStore {
        let store = test_store();
        let mut task = make_task("task-1");
        task.status = status;
        task.duration_minutes = duration;
        task.time_logged_minutes = logged;
        seed_task(&store, &task);
        store
    }

    /// A "task-1" (`Scheduled`, 90 min, 10 already logged) with two scheduled
    /// chunks seeded back-to-back with a 15-minute gap: chunk-1 (30m) then
    /// chunk-2 (45m). Shared by the `complete_task` tests that exercise
    /// chunk collapse.
    fn two_chunk_task_90_10() -> SqliteStore {
        let store = store_with_task(TaskStatus::Scheduled, 90, 10);
        let chunk1 = make_scheduled_chunk("chunk-1", "task-1", 30);
        let mut chunk2 = make_scheduled_chunk("chunk-2", "task-1", 45);
        chunk2.start_time = chunk1.end_time + Duration::minutes(15);
        chunk2.end_time = chunk2.start_time + Duration::minutes(45);
        seed_chunk(&store, &chunk1);
        seed_chunk(&store, &chunk2);
        store
    }

    /// Seed `chunk` and complete it with the given `now`. Shared by the
    /// `complete_chunk` tests that assert on completion timing.
    fn seed_and_complete(
        store: &SqliteStore,
        chunk: &Chunk,
        override_min: Option<i64>,
        now: DateTime<Utc>,
    ) -> (Chunk, Task) {
        seed_chunk(store, chunk);
        complete_chunk(store, &chunk.id, override_min, now).expect("should succeed")
    }

    /// Assert `completed` kept its 30-minute pinned window starting at
    /// `pinned_start` and logged exactly `expected_logged` minutes. Shared by
    /// the two tests asserting a fixed chunk survives completion unmoved.
    fn assert_kept_pinned_window(
        completed: &Chunk,
        pinned_start: DateTime<Utc>,
        expected_logged: i64,
    ) {
        assert_eq!(completed.status, ChunkStatus::Completed);
        assert_eq!(completed.start_time, pinned_start);
        assert_eq!(completed.end_time, pinned_start + Duration::minutes(30));
        assert_eq!(completed.logged_minutes, Some(expected_logged));
    }

    #[test_case(None, 30 ; "logged from planned window")]
    #[test_case(Some(45), 45 ; "logged from duration override")]
    fn complete_chunk_happy_path(override_min: Option<i64>, expected_logged: i64) {
        let store = store_with_task(TaskStatus::Scheduled, 120, 0);
        seed_chunk(&store, &make_scheduled_chunk("chunk-1", "task-1", 30));

        let (completed_chunk, updated_task) =
            complete_chunk(&store, "chunk-1", override_min, test_now()).expect("should succeed");

        assert_eq!(completed_chunk.status, ChunkStatus::Completed);
        assert!(completed_chunk.completed_at.is_some());
        assert_eq!(completed_chunk.logged_minutes, Some(expected_logged));
        assert_eq!(updated_task.time_logged_minutes, expected_logged);
        assert_eq!(updated_task.status, TaskStatus::Scheduled);
    }

    #[test]
    fn complete_chunk_negative_duration_override_rejected() {
        let store = store_with_task(TaskStatus::Pending, 60, 0);
        seed_chunk(&store, &make_scheduled_chunk("chunk-1", "task-1", 30));

        let result = complete_chunk(&store, "chunk-1", Some(-10), test_now());
        assert!(
            matches!(result, Err(AppError::Validation(ref msg)) if msg.contains("negative")),
            "expected Validation error, got: {result:?}"
        );
    }

    #[test]
    fn complete_chunk_already_completed() {
        let store = store_with_task(TaskStatus::Pending, 60, 0);
        let mut chunk = make_scheduled_chunk("chunk-1", "task-1", 30);
        chunk.status = ChunkStatus::Completed;
        seed_chunk(&store, &chunk);

        let result = complete_chunk(&store, "chunk-1", None, test_now());
        assert!(
            matches!(result, Err(AppError::Validation(ref msg)) if msg.contains("already completed")),
            "expected Validation error, got: {result:?}"
        );
    }

    #[test]
    fn complete_chunk_not_found() {
        let store = test_store();
        let result = complete_chunk(&store, "nonexistent", None, test_now());
        assert!(
            matches!(
                result,
                Err(AppError::NotFound { ref entity, ref id })
                if entity == "Chunk" && id == "nonexistent"
            ),
            "expected NotFound, got: {result:?}"
        );
    }

    /// Completing a chunk auto-completes the task exactly when the total logged
    /// time reaches `duration_minutes`.
    #[test_case(60, 30, 30, TaskStatus::Completed, 60 ; "reaching duration auto-completes")]
    #[test_case(120, 0, 30, TaskStatus::Scheduled, 30 ; "under duration stays scheduled")]
    #[test_case(60, 0, 60, TaskStatus::Completed, 60 ; "exact single-chunk boundary completes")]
    fn complete_chunk_task_status_tracks_logged_time(
        duration: i64,
        pre_logged: i64,
        chunk_min: i64,
        expected_status: TaskStatus,
        expected_logged: i64,
    ) {
        let store = store_with_task(TaskStatus::Scheduled, duration, pre_logged);
        seed_chunk(
            &store,
            &make_scheduled_chunk("chunk-1", "task-1", chunk_min),
        );

        let (_chunk, updated_task) =
            complete_chunk(&store, "chunk-1", None, test_now()).expect("should succeed");

        assert_eq!(updated_task.status, expected_status);
        assert_eq!(updated_task.time_logged_minutes, expected_logged);
    }

    /// Divergence pin (bugfix batch B2): completing the task's *only* scheduled
    /// chunk logs the chunk's planned length, not the remaining budget. On an
    /// under-placed task (45-min chunk, 120-min duration) the task must stay
    /// `Scheduled` with the 75-min remainder left for the next reschedule —
    /// unlike `complete_task`, which logs all remaining time at once.
    #[test]
    fn complete_chunk_under_placed_last_chunk_leaves_remainder() {
        let store = store_with_task(TaskStatus::Scheduled, 120, 0);
        seed_chunk(&store, &make_scheduled_chunk("chunk-1", "task-1", 45));

        let (chunk, task) =
            complete_chunk(&store, "chunk-1", None, test_now()).expect("should succeed");

        assert_eq!(chunk.logged_minutes, Some(45));
        assert_eq!(task.time_logged_minutes, 45);
        assert_eq!(task.status, TaskStatus::Scheduled);
        assert_eq!(task.duration_minutes - task.time_logged_minutes, 75);
    }

    #[test]
    fn complete_task_collapses_scheduled_chunks_and_completes_task() {
        let store = two_chunk_task_90_10();

        let updated_task = complete_task(&store, "task-1", test_now()).expect("should succeed");

        assert_eq!(updated_task.status, TaskStatus::Completed);
        assert_eq!(updated_task.time_logged_minutes, 90);

        // The earliest scheduled chunk survives and logs all outstanding time
        // (duration 90 − already-logged 10); later scheduled chunks are deleted.
        let survivor = store
            .get_chunk("chunk-1")
            .expect("load chunk-1")
            .expect("chunk-1 exists");
        assert_eq!(survivor.status, ChunkStatus::Completed);
        assert_eq!(survivor.logged_minutes, Some(80));
        assert!(
            store.get_chunk("chunk-2").expect("load chunk-2").is_none(),
            "later scheduled chunks should be deleted on collapse"
        );
    }

    /// Completing a task with no scheduled chunks (e.g. a backlog item done
    /// outside the planner) synthesizes a chunk covering the outstanding time
    /// and completes it through the normal chunk path, so logged time, the
    /// SYSTEM comment, and reopenability follow the one completion policy.
    #[test_case(TaskStatus::Backlog, 60, 0, 60 ; "backlog logs full duration")]
    #[test_case(TaskStatus::Pending, 90, 30, 60 ; "pending logs the remainder")]
    #[test_case(TaskStatus::Pending, 60, 60, 0 ; "fully logged gets zero-length chunk")]
    fn complete_task_without_scheduled_chunks_synthesizes_completed_chunk(
        status: TaskStatus,
        duration: i64,
        logged: i64,
        expected_chunk_logged: i64,
    ) {
        let store = store_with_task(status, duration, logged);

        let now_ts = test_now();
        let updated_task = complete_task(&store, "task-1", now_ts).expect("should succeed");

        assert_eq!(updated_task.status, TaskStatus::Completed);
        assert_eq!(updated_task.time_logged_minutes, duration);

        let chunks = store.get_chunks_for_task("task-1").expect("load chunks");
        assert_eq!(chunks.len(), 1, "expected exactly one synthesized chunk");
        let chunk = &chunks[0];
        assert_eq!(chunk.status, ChunkStatus::Completed);
        assert_eq!(chunk.logged_minutes, Some(expected_chunk_logged));
        assert_eq!(chunk.end_time, now_ts);
        assert_eq!(
            (chunk.end_time - chunk.start_time).num_minutes(),
            expected_chunk_logged
        );
        assert_eq!(chunk.completed_at, Some(chunk.end_time));
        assert!(!chunk.is_fixed);

        only_system_comment(&store);
    }

    #[test_case(None, 30 ; "length derived from planned window")]
    #[test_case(Some(45), 45 ; "length from duration override")]
    fn complete_chunk_anchors_completed_window_to_now(
        override_min: Option<i64>,
        expected_len: i64,
    ) {
        let store = store_with_task(TaskStatus::Scheduled, 120, 0);

        // A prior reschedule parked this scheduler-placed (non-fixed) chunk a
        // day after test_now; only non-fixed chunks re-anchor on completion.
        let planned_start = test_now() + Duration::days(1);
        let mut chunk = make_scheduled_chunk("chunk-1", "task-1", 30);
        chunk.is_fixed = false;
        chunk.start_time = planned_start;
        chunk.end_time = planned_start + Duration::minutes(30);
        let now_ts = test_now();
        let (completed, _task) = seed_and_complete(&store, &chunk, override_min, now_ts);

        // Window now ends at the completion moment, not the future planned slot.
        assert_eq!(completed.end_time, now_ts);
        assert!(completed.end_time < planned_start);
        assert_eq!(completed.completed_at, Some(completed.end_time));
        assert_eq!(completed.logged_minutes, Some(expected_len));
        assert_eq!(
            (completed.end_time - completed.start_time).num_minutes(),
            expected_len
        );
    }

    /// A fixed (pinned) chunk completes in place: the user pinned the window,
    /// so completion must not re-anchor it to "now".
    #[test_case(None, 30 ; "logged from planned window")]
    #[test_case(Some(45), 45 ; "logged from duration override")]
    fn complete_chunk_fixed_keeps_pinned_window(override_min: Option<i64>, expected_logged: i64) {
        let store = store_with_task(TaskStatus::Scheduled, 120, 0);

        // Pinned a day in the future; the window must survive completion as-is.
        let pinned_start = test_now() + Duration::days(1);
        let mut chunk = make_scheduled_chunk("chunk-1", "task-1", 30);
        chunk.is_fixed = true;
        chunk.start_time = pinned_start;
        chunk.end_time = pinned_start + Duration::minutes(30);
        let now_ts = test_now();
        let (completed, _task) = seed_and_complete(&store, &chunk, override_min, now_ts);

        assert_kept_pinned_window(&completed, pinned_start, expected_logged);
        // completed_at still records the completion moment.
        assert_eq!(completed.completed_at, Some(now_ts));
    }

    /// `complete_task` inherits the pin rule via `complete_chunk`: a fixed
    /// surviving chunk stays where it was pinned instead of ending "now".
    #[test]
    fn complete_task_fixed_survivor_stays_pinned() {
        let store = store_with_task(TaskStatus::Scheduled, 75, 0);

        let pinned_start = test_now() + Duration::days(2);
        let mut survivor = make_scheduled_chunk("chunk-1", "task-1", 30);
        survivor.is_fixed = true;
        survivor.start_time = pinned_start;
        survivor.end_time = pinned_start + Duration::minutes(30);
        let mut later = make_scheduled_chunk("chunk-2", "task-1", 45);
        later.is_fixed = false;
        later.start_time = pinned_start + Duration::hours(3);
        later.end_time = later.start_time + Duration::minutes(45);
        seed_chunk(&store, &survivor);
        seed_chunk(&store, &later);

        complete_task(&store, "task-1", test_now()).expect("should succeed");

        let completed = store.get_chunk("chunk-1").unwrap().unwrap();
        assert_kept_pinned_window(&completed, pinned_start, 75);
        assert!(store.get_chunk("chunk-2").unwrap().is_none());
    }

    #[test]
    fn complete_task_collapses_to_one_chunk_ending_now() {
        let store = store_with_task(TaskStatus::Scheduled, 75, 0);

        // Two future non-fixed chunks a reschedule had spread out with a gap
        // between them (a fixed survivor would stay pinned instead).
        let base = test_now() + Duration::days(2);
        let mut chunk1 = make_scheduled_chunk("chunk-1", "task-1", 30);
        chunk1.is_fixed = false;
        chunk1.start_time = base;
        chunk1.end_time = base + Duration::minutes(30);
        let mut chunk2 = make_scheduled_chunk("chunk-2", "task-1", 45);
        chunk2.is_fixed = false;
        chunk2.start_time = base + Duration::hours(3);
        chunk2.end_time = chunk2.start_time + Duration::minutes(45);
        seed_chunk(&store, &chunk1);
        seed_chunk(&store, &chunk2);

        let now_ts = test_now();
        complete_task(&store, "task-1", now_ts).expect("should succeed");

        // The task collapses into the surviving (earliest) chunk: it logs all 75
        // outstanding minutes and ends "now"; the later chunk is deleted.
        let survivor = store.get_chunk("chunk-1").unwrap().unwrap();
        assert_eq!(survivor.status, ChunkStatus::Completed);
        assert_eq!(survivor.logged_minutes, Some(75));
        assert_eq!(survivor.end_time, now_ts);
        assert_eq!((survivor.end_time - survivor.start_time).num_minutes(), 75);
        assert!(
            store.get_chunk("chunk-2").unwrap().is_none(),
            "the later scheduled chunk should be deleted on collapse"
        );
    }

    #[test]
    fn reopen_chunk_happy_path() {
        let store = store_with_task(TaskStatus::Scheduled, 120, 30);
        seed_chunk(&store, &make_completed_chunk("chunk-1", "task-1", 30));

        let (reopened_chunk, updated_task) =
            reopen_chunk(&store, "chunk-1", test_now()).expect("should succeed");

        assert_eq!(reopened_chunk.status, ChunkStatus::Scheduled);
        assert!(reopened_chunk.completed_at.is_none());
        assert!(reopened_chunk.logged_minutes.is_none());
        assert_eq!(updated_task.time_logged_minutes, 0);
        assert_eq!(updated_task.status, TaskStatus::Scheduled);
    }

    #[test]
    fn reopen_chunk_not_completed() {
        let store = store_with_task(TaskStatus::Pending, 60, 0);
        seed_chunk(&store, &make_scheduled_chunk("chunk-1", "task-1", 30));

        let result = reopen_chunk(&store, "chunk-1", test_now());
        assert!(
            matches!(result, Err(AppError::Validation(ref msg)) if msg.contains("not completed")),
            "expected Validation error, got: {result:?}"
        );
    }

    #[test]
    fn reopen_chunk_not_found() {
        let store = test_store();
        let result = reopen_chunk(&store, "nonexistent", test_now());
        assert!(
            matches!(
                result,
                Err(AppError::NotFound { ref entity, ref id })
                if entity == "Chunk" && id == "nonexistent"
            ),
            "expected NotFound, got: {result:?}"
        );
    }

    /// Reopening subtracts the chunk's logged time; a `Completed` task reverts
    /// to `Scheduled`, while an already-`Scheduled` task just stays there.
    #[test_case(TaskStatus::Completed, 60 ; "completed task reverts to scheduled")]
    #[test_case(TaskStatus::Scheduled, 120 ; "scheduled task stays scheduled")]
    fn reopen_chunk_task_status_after_reopen(status: TaskStatus, duration: i64) {
        let store = store_with_task(status, duration, 60);
        seed_chunk(&store, &make_completed_chunk("chunk-1", "task-1", 30));

        let (_chunk, updated_task) =
            reopen_chunk(&store, "chunk-1", test_now()).expect("should succeed");

        assert_eq!(updated_task.status, TaskStatus::Scheduled);
        assert_eq!(updated_task.time_logged_minutes, 30);
    }

    #[test]
    fn reopen_chunk_logged_minutes_none_subtracts_zero() {
        let store = store_with_task(TaskStatus::Completed, 60, 60);

        // Completed chunk with logged_minutes = None (defensive edge case)
        let start = Utc.with_ymd_and_hms(2026, 3, 15, 18, 0, 0).unwrap();
        let mut chunk = make_completed_chunk("chunk-1", "task-1", 30);
        chunk.logged_minutes = None;
        chunk.completed_at = Some(start);
        seed_chunk(&store, &chunk);

        let (_chunk, updated_task) =
            reopen_chunk(&store, "chunk-1", test_now()).expect("should succeed");

        // logged_minutes was None, so 0 is subtracted
        assert_eq!(updated_task.time_logged_minutes, 60);
        assert_eq!(updated_task.status, TaskStatus::Scheduled);
        // The system comment reports the actual (zero) delta (M12.5).
        let comment = only_system_comment(&store);
        assert_eq!(
            comment.content,
            "Chunk reopened: -0m logged (1h / 1h total)"
        );
    }

    /// The single SYSTEM comment on "task-1", asserting there is exactly one.
    fn only_system_comment(store: &SqliteStore) -> Comment {
        let comments = store.list_comments_for_task("task-1").expect("list");
        assert_eq!(comments.len(), 1, "expected exactly one comment");
        let comment = comments.into_iter().next().expect("one comment");
        assert_eq!(comment.author, SYSTEM_AUTHOR);
        comment
    }

    #[test]
    fn complete_chunk_records_system_comment() {
        let store = store_with_task(TaskStatus::Scheduled, 120, 30);
        seed_chunk(&store, &make_scheduled_chunk("chunk-1", "task-1", 45));

        complete_chunk(&store, "chunk-1", None, test_now()).expect("should succeed");

        let comment = only_system_comment(&store);
        assert_eq!(
            comment.content,
            "Chunk completed: +45m logged (1h 15m / 2h total)"
        );
    }

    #[test_case(75, "Chunk reopened: -45m logged (30m / 2h total)"; "normal subtraction")]
    #[test_case(30, "Chunk reopened: -30m logged (0m / 2h total)"; "clamped at zero when logged exceeds total")]
    fn reopen_chunk_system_comment(logged_min: i64, expected: &str) {
        let store = store_with_task(TaskStatus::Scheduled, 120, logged_min);
        seed_chunk(&store, &make_completed_chunk("chunk-1", "task-1", 45));

        reopen_chunk(&store, "chunk-1", test_now()).expect("should succeed");

        let comment = only_system_comment(&store);
        assert_eq!(comment.content, expected);
    }

    /// `complete_task` collapses to one surviving chunk, so exactly one
    /// system comment is recorded — logging the outstanding remainder.
    #[test]
    fn complete_task_records_exactly_one_system_comment() {
        let store = two_chunk_task_90_10();

        complete_task(&store, "task-1", test_now()).expect("should succeed");

        let comment = only_system_comment(&store);
        assert_eq!(
            comment.content,
            "Chunk completed: +1h 20m logged (1h 30m / 1h 30m total)"
        );
    }

    #[test_case(TaskStatus::Pending ; "pending")]
    #[test_case(TaskStatus::Backlog ; "backlog")]
    fn cancel_task_without_chunks(status: TaskStatus) {
        let store = store_with_task(status, 60, 0);

        let cancelled = cancel_task(&store, "task-1", test_now()).expect("should succeed");
        assert_eq!(cancelled.status, TaskStatus::Cancelled);
    }

    #[test]
    fn cancel_task_scheduled_with_mixed_chunks() {
        let store = store_with_task(TaskStatus::Scheduled, 120, 30);

        seed_chunk(
            &store,
            &make_completed_chunk("chunk-completed", "task-1", 30),
        );
        seed_chunk(
            &store,
            &make_scheduled_chunk("chunk-scheduled", "task-1", 30),
        );

        let cancelled = cancel_task(&store, "task-1", test_now()).expect("should succeed");
        assert_eq!(cancelled.status, TaskStatus::Cancelled);

        assert!(store.get_chunk("chunk-scheduled").unwrap().is_none());
        assert!(store.get_chunk("chunk-completed").unwrap().is_some());
    }

    #[test_case(TaskStatus::Completed ; "already completed")]
    #[test_case(TaskStatus::Cancelled ; "already cancelled")]
    fn cancel_task_terminal_status_rejected(status: TaskStatus) {
        let store = store_with_task(status, 60, 0);

        let result = cancel_task(&store, "task-1", test_now());
        assert!(
            matches!(result, Err(AppError::Validation(ref msg)) if msg.contains("cannot cancel")),
            "expected Validation error for {status:?}, got: {result:?}"
        );
    }

    #[test]
    fn cancel_task_not_found() {
        let store = test_store();
        let result = cancel_task(&store, "nonexistent", test_now());
        assert!(
            matches!(result, Err(AppError::NotFound { .. })),
            "expected NotFound, got: {result:?}"
        );
    }

    #[test]
    fn cancel_task_stamps_injected_now() {
        let store = store_with_task(TaskStatus::Pending, 60, 0);
        let now = utc(2026, 5, 15, 10, 0);
        let cancelled = cancel_task(&store, "task-1", now).expect("should succeed");
        assert_eq!(cancelled.updated_at, now);
    }
}
