// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Integration tests (`DefaultScheduler`).
//!
//! These tests use the real `DefaultScheduler` instead of `MockScheduler`.
//! They require a schedule with windows so `expand_schedule_windows` produces
//! actual slots that the engine can place chunks into.

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc, Weekday};
use test_case::test_case;

use super::{assert_reschedule_empty_updates_config, chunk_count, stored_task};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{ChunkStatus, Priority, TaskStatus};
use crate::domain::models::{Chunk, ExternalEventRecord, Schedule, ScheduleWindow, Task};
use crate::scheduler::engine::DefaultScheduler;
use crate::services::scheduling::{reschedule, reschedule_incremental};
use crate::services::task::complete_task;
use crate::test_support::{
    default_config, default_schedule_id, seed_chunk, seed_task, test_store_with_config,
};
use crate::traits::scheduling::{ScheduleResult, ScheduleWarning, WarningKind};
use crate::traits::storage::{ChunkStore, ExternalEventStore, ScheduleStore};

mod support;
use support::{
    assert_chunks_valid_and_non_overlapping, assert_minute_aligned, assert_no_overlap_with_busy,
    assert_total_chunk_duration, make_schedulable_task, monday_now, seed_all_day_schedule,
    seed_busy_event,
};

fn assert_status_after_reschedule(
    store: &SqliteStore,
    now: DateTime<Utc>,
    task_id: &str,
    expected: TaskStatus,
) {
    reschedule(store, &DefaultScheduler, now).expect("reschedule should succeed");
    let t = stored_task(store, task_id).unwrap();
    assert_eq!(t.status, expected, "unexpected status after reschedule");
}

fn scheduled_60min_task_store() -> (SqliteStore, DateTime<Utc>, Task) {
    let (store, now) = test_fixture();
    let mut task = make_schedulable_task("task-1", Priority::Medium, 60);
    task.status = TaskStatus::Scheduled;
    (store, now, task)
}

fn chunks_for_task<'a>(result: &'a ScheduleResult, task_id: &str) -> Vec<&'a Chunk> {
    result
        .placed_chunks
        .iter()
        .filter(|c| c.task_id == task_id)
        .collect()
}

fn warnings_for_task<'a>(result: &'a ScheduleResult, task_id: &str) -> Vec<&'a ScheduleWarning> {
    result
        .warnings
        .iter()
        .filter(|w| w.task_id == task_id)
        .collect()
}

fn test_fixture() -> (SqliteStore, DateTime<Utc>) {
    let store = test_store_with_config(default_config());
    seed_all_day_schedule(&store);
    (store, monday_now())
}

/// Build a store pre-seeded with a `no_split` 600-minute task.
/// Seeded directly at store level, bypassing service validation, because validation rejects
/// `duration > largest_window`.
fn huge_no_split_task_store(deadline: Option<DateTime<Utc>>) -> (SqliteStore, DateTime<Utc>) {
    let (store, now) = test_fixture();
    let mut task = make_schedulable_task("task-huge", Priority::High, 600);
    task.no_split = true;
    task.deadline = deadline;
    seed_task(&store, &task);
    (store, now)
}

#[test]
fn integration_full_reschedule_places_chunks_in_windows() {
    let (store, now) = test_fixture();

    let mut high = make_schedulable_task("task-high", Priority::High, 30);
    high.deadline = Some(now + Duration::days(14));
    seed_task(&store, &high);

    let mut med = make_schedulable_task("task-med", Priority::Medium, 60);
    med.deadline = Some(now + Duration::days(14));
    seed_task(&store, &med);

    let mut low = make_schedulable_task("task-low", Priority::Low, 90);
    low.deadline = Some(now + Duration::days(14));
    seed_task(&store, &low);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert!(
        !result.placed_chunks.is_empty(),
        "expected chunks to be placed"
    );
    let placed_task_ids: std::collections::HashSet<&str> = result
        .placed_chunks
        .iter()
        .map(|c| c.task_id.as_str())
        .collect();
    assert!(
        placed_task_ids.contains("task-high"),
        "task-high should be placed"
    );
    assert!(
        placed_task_ids.contains("task-med"),
        "task-med should be placed"
    );
    assert!(
        placed_task_ids.contains("task-low"),
        "task-low should be placed"
    );

    assert_chunks_valid_and_non_overlapping(&result.placed_chunks);

    assert_total_chunk_duration(&result.placed_chunks, "task-high", 30);
    assert_total_chunk_duration(&result.placed_chunks, "task-med", 60);
    assert_total_chunk_duration(&result.placed_chunks, "task-low", 90);

    for id in &["task-high", "task-med", "task-low"] {
        let t = stored_task(&store, id).unwrap();
        assert_eq!(
            t.status,
            TaskStatus::Scheduled,
            "task {id} should be Scheduled"
        );
    }
}

#[test]
fn integration_higher_priority_gets_earlier_slot() {
    let (store, now) = test_fixture();

    // Both tasks have same duration (60 min), differing priority only.
    let high = make_schedulable_task("task-high", Priority::High, 60);
    let low = make_schedulable_task("task-low", Priority::Low, 60);
    seed_task(&store, &high);
    seed_task(&store, &low);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    let earliest_start = |task_id: &str| -> DateTime<Utc> {
        result
            .placed_chunks
            .iter()
            .filter(|c| c.task_id == task_id)
            .map(|c| c.start_time)
            .min()
            .expect("task should have at least one chunk")
    };

    let high_start = earliest_start("task-high");
    let low_start = earliest_start("task-low");

    assert!(
        high_start <= low_start,
        "high-priority task should start no later than low-priority task; \
         high_start={high_start}, low_start={low_start}"
    );
}

#[test]
fn integration_reschedule_is_idempotent() {
    let (store, now) = test_fixture();
    let task = make_schedulable_task("task-1", Priority::Medium, 60);
    seed_task(&store, &task);

    let result1 =
        reschedule(&store, &DefaultScheduler, now).expect("first reschedule should succeed");
    let chunk_count_after_first = chunk_count(&store);

    let result2 =
        reschedule(&store, &DefaultScheduler, now).expect("second reschedule should succeed");
    let chunk_count_after_second = chunk_count(&store);

    assert_eq!(
        chunk_count_after_first, chunk_count_after_second,
        "chunk count should be stable across two reschedule runs"
    );

    assert_eq!(
        result1.placed_chunks.len(),
        result2.placed_chunks.len(),
        "placed chunk count should be stable"
    );

    let t = stored_task(&store, "task-1").unwrap();
    assert_eq!(t.status, TaskStatus::Scheduled);
}

#[test]
fn integration_incremental_replaces_target_preserves_others() {
    let (store, now) = test_fixture();

    let task_a = make_schedulable_task("task-a", Priority::High, 30);
    let task_b = make_schedulable_task("task-b", Priority::Medium, 30);
    seed_task(&store, &task_a);
    seed_task(&store, &task_b);

    reschedule(&store, &DefaultScheduler, now).expect("full reschedule should succeed");

    let chunks_before_b: Vec<Chunk> = store.get_chunks_for_task("task-b").unwrap();

    let result_inc = reschedule_incremental(&store, &DefaultScheduler, &["task-a".to_owned()], now)
        .expect("incremental reschedule should succeed");

    let placed_ids: Vec<&str> = result_inc
        .placed_chunks
        .iter()
        .map(|c| c.task_id.as_str())
        .collect();
    assert!(
        placed_ids.contains(&"task-a"),
        "task-a should appear in incremental placed chunks"
    );

    assert!(
        !chunks_before_b.is_empty(),
        "task-b should have chunks from the full reschedule"
    );
    for c in &chunks_before_b {
        assert!(
            store.get_chunk(&c.id).unwrap().is_some(),
            "task-b chunk {} should still exist in the store",
            c.id
        );
    }
}

#[test_case(30_i64, TaskStatus::Pending, 0_i64; "pending_task_becomes_scheduled")]
#[test_case(60_i64, TaskStatus::Scheduled, 60_i64; "fully_logged_scheduled_task_stays_scheduled")]
fn status_transitions_after_reschedule(
    duration: i64,
    initial_status: TaskStatus,
    time_logged: i64,
) {
    let (store, now) = test_fixture();
    let mut task = make_schedulable_task("task-1", Priority::Medium, duration);
    task.status = initial_status;
    task.time_logged_minutes = time_logged;
    seed_task(&store, &task);
    assert_status_after_reschedule(&store, now, "task-1", TaskStatus::Scheduled);
}

#[test]
fn integration_fixed_chunks_keep_task_scheduled() {
    // Task is Scheduled with a fixed chunk covering the full duration.
    let (store, now, task) = scheduled_60min_task_store();
    seed_task(&store, &task);

    // The fixed chunk covers the full 60 min — scheduler sees 0 remaining.
    let fixed = Chunk {
        id: "fixed-chunk".to_owned(),
        task_id: "task-1".to_owned(),
        start_time: now + Duration::hours(1),
        end_time: now + Duration::hours(2),
        status: ChunkStatus::Scheduled,
        is_fixed: true,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    };
    seed_chunk(&store, &fixed);

    assert_status_after_reschedule(&store, now, "task-1", TaskStatus::Scheduled);
}

#[test]
fn integration_no_windows_tasks_remain_pending() {
    let store = test_store_with_config(default_config());
    // Explicitly pre-seed a window-less "sched-default" so that
    // seed_task's ensure_schedule skips auto-creation (which now adds a
    // window). The scheduler then receives no slots for this schedule.
    store
        .create_schedule(&Schedule::test_default().with_id("sched-default"))
        .expect("seed window-less sched-default");

    let now = monday_now();
    let task = make_schedulable_task("task-1", Priority::Medium, 60);
    seed_task(&store, &task);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert!(
        result.placed_chunks.is_empty(),
        "no chunks should be placed without schedule windows"
    );

    let t = stored_task(&store, "task-1").unwrap();
    assert_eq!(
        t.status,
        TaskStatus::Pending,
        "task should remain Pending when no windows exist"
    );
}

#[test]
fn integration_no_tasks_empty_result_config_updated() {
    let (store, now) = test_fixture();

    assert_reschedule_empty_updates_config(&store, &DefaultScheduler, now);
}

#[test]
fn integration_partial_logged_schedules_remaining_only() {
    let (store, now) = test_fixture();

    let mut task = make_schedulable_task("task-1", Priority::Medium, 60);
    task.time_logged_minutes = 30;
    seed_task(&store, &task);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert_total_chunk_duration(&result.placed_chunks, "task-1", 30);
}

#[test_case(Priority::Medium, 60_i64, 1_usize, 60_i64, 0_usize; "no_split_60min_placed_as_single_chunk")]
#[test_case(Priority::High, 600_i64, 0_usize, 0_i64, 1_usize; "no_split_600min_emits_warning")]
fn no_split_placement_behavior(
    priority: Priority,
    duration: i64,
    expected_chunks: usize,
    expected_total_minutes: i64,
    expected_unschedulable: usize,
) {
    let (store, now) = test_fixture();
    let mut task = make_schedulable_task("task-no-split", priority, duration);
    task.no_split = true;
    seed_task(&store, &task);
    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");
    let task_chunks = chunks_for_task(&result, "task-no-split");
    assert_eq!(task_chunks.len(), expected_chunks, "unexpected chunk count");
    assert_total_chunk_duration(
        &result.placed_chunks,
        "task-no-split",
        expected_total_minutes,
    );
    let unschedulable: usize = warnings_for_task(&result, "task-no-split")
        .into_iter()
        .filter(|w| matches!(w.kind, WarningKind::Unschedulable { .. }))
        .count();
    assert_eq!(
        unschedulable, expected_unschedulable,
        "unexpected unschedulable warning count"
    );
}

#[test]
/// Verify that an incremental reschedule for a high-priority task cascades into
/// re-scheduling a low-priority task whose existing chunk it displaces.
///
/// Setup:
/// - task-B (low priority) is already Scheduled with an auto-chunk at 10:00–10:30.
/// - task-A (high priority) is Pending; an incremental reschedule is triggered for task-A only.
/// - `DefaultScheduler` places task-A at 10:00–10:30 — the earliest free slot — which overlaps
///   task-B's existing chunk.
/// - task-B is added to the cascade set and re-placed at the next available slot.
fn integration_incremental_cascading_displacement() {
    // now = 2026-03-23 10:00 UTC (Monday). Window 09:00–17:00 is clipped to
    // [10:00, 17:00) for today.
    let (store, now) = test_fixture();
    let slot_start = now; // 10:00 UTC
    let slot_end = now + Duration::minutes(30); // 10:30 UTC

    let task_a = make_schedulable_task("task-a", Priority::High, 30);
    seed_task(&store, &task_a);

    let mut task_b = make_schedulable_task("task-b", Priority::Low, 30);
    task_b.status = TaskStatus::Scheduled;
    seed_task(&store, &task_b);

    let b_existing_chunk = Chunk {
        id: "b-chunk-existing".to_owned(),
        task_id: "task-b".to_owned(),
        start_time: slot_start,
        end_time: slot_end,
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    };
    seed_chunk(&store, &b_existing_chunk);

    // Incremental reschedule for task-A only.
    // DefaultScheduler places task-A at [10:00, 10:30) — the earliest free
    // slot. This overlaps task-B's existing chunk at the same times, so
    // task-B is added to the cascade set and re-placed.
    let result_inc = reschedule_incremental(&store, &DefaultScheduler, &["task-a".to_owned()], now)
        .expect("incremental reschedule should succeed");

    let placed_ids: std::collections::HashSet<&str> = result_inc
        .placed_chunks
        .iter()
        .map(|c| c.task_id.as_str())
        .collect();
    assert!(
        placed_ids.contains("task-a"),
        "task-A should be placed; placed_ids={placed_ids:?}"
    );

    assert!(
        placed_ids.contains("task-b"),
        "task-B should be cascaded and re-placed; placed_ids={placed_ids:?}"
    );
}

#[test]
fn integration_busy_time_excluded_from_placement() {
    let (store, now) = test_fixture();

    let busy_start = Utc.with_ymd_and_hms(2026, 3, 23, 9, 0, 0).unwrap();
    let busy_end = Utc.with_ymd_and_hms(2026, 3, 23, 11, 0, 0).unwrap();
    seed_busy_event(&store, now, busy_start, busy_end);

    let task = make_schedulable_task("task-1", Priority::High, 60);
    seed_task(&store, &task);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert_no_overlap_with_busy(&result.placed_chunks, busy_start, busy_end);

    // Task should still be scheduled (window is 09:00–17:00, 8h available,
    // busy block removes 2h, leaving 6h — plenty for 60 min task).
    let t = stored_task(&store, "task-1").unwrap();
    assert_eq!(t.status, TaskStatus::Scheduled);
}

#[test]
fn integration_sub_minute_now_yields_minute_aligned_chunks() {
    let (store, now) = test_fixture();
    // 10:00:47.606881160 — the live-trace shape that caused ragged anchors.
    let now = now + Duration::nanoseconds(47_606_881_160);

    seed_task(&store, &make_schedulable_task("task-a", Priority::High, 60));

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert_minute_aligned(&result.placed_chunks);
    let first = result
        .placed_chunks
        .iter()
        .map(|c| c.start_time)
        .min()
        .unwrap();
    assert_eq!(
        first,
        monday_now() + Duration::minutes(1),
        "placement must start at the whole minute after now"
    );
}

#[test]
fn integration_ragged_busy_edge_yields_minute_aligned_chunks() {
    let (store, now) = test_fixture();

    // Busy 10:00:00 – 11:30:30 UTC: the ragged end cuts the free slot mid-minute.
    let busy_start = Utc.with_ymd_and_hms(2026, 3, 23, 10, 0, 0).unwrap();
    let busy_end = Utc.with_ymd_and_hms(2026, 3, 23, 11, 30, 30).unwrap();
    seed_busy_event(&store, now, busy_start, busy_end);

    // 6 h of work forces placement past the busy block into the cut slot.
    seed_task(
        &store,
        &make_schedulable_task("task-a", Priority::High, 360),
    );

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert_minute_aligned(&result.placed_chunks);
    assert_no_overlap_with_busy(&result.placed_chunks, busy_start, busy_end);
}

#[test_case(false, false, true ; "transparent_event_ignored")]
#[test_case(false, true, true ; "declined_event_ignored")]
#[test_case(true, false, false ; "busy_event_blocks")]
fn integration_non_busy_event_does_not_block(busy: bool, declined: bool, expect_placed: bool) {
    let (store, now) = test_fixture();

    // Seed an event that covers the entire 09:00–17:00 window on 2026-03-23.
    // For the busy_event_blocks case this leaves zero available time;
    // for the non-busy cases the full 8 h window remains available.
    let day_start = Utc.with_ymd_and_hms(2026, 3, 23, 9, 0, 0).unwrap();
    let day_end = Utc.with_ymd_and_hms(2026, 3, 23, 17, 0, 0).unwrap();
    let event = ExternalEventRecord {
        id: "ev-param".to_owned(),
        calendar_id: "cal-1".to_owned(),
        event_id: "gcal-param".to_owned(),
        title: "All-day cover".to_owned(),
        description: None,
        start_time: day_start,
        end_time: day_end,
        busy,
        declined,
        all_day: false,
        updated_at: now,
    };
    store
        .replace_external_events_in_window(
            "cal-1",
            day_start - Duration::hours(1),
            day_end + Duration::hours(1),
            &[event],
        )
        .expect("seed event");

    // 60-minute task; if the first day is fully blocked, all subsequent days
    // are open and will absorb the task.
    let task = make_schedulable_task("task-param", Priority::High, 60);
    seed_task(&store, &task);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    let task_chunks = chunks_for_task(&result, "task-param");

    if expect_placed {
        assert!(
            !task_chunks.is_empty(),
            "non-busy event (busy={busy}, declined={declined}) must not block placement"
        );
    } else {
        // busy=true covers the whole first-day window (09–17); a 30-day
        // horizon has many more open days so the task will still get placed
        // on other days. Verify instead that chunks do NOT fall in the
        // blocked first-day window.
        let blocked_start = day_start;
        let blocked_end = day_end;
        for chunk in &result.placed_chunks {
            let overlaps = chunk.start_time < blocked_end && chunk.end_time > blocked_start;
            assert!(
                !overlaps,
                "busy event must block the window: chunk [{}, {}) overlaps [{blocked_start}, {blocked_end})",
                chunk.start_time, chunk.end_time
            );
        }
    }
}

#[test_case(None; "no_deadline_no_warning")]
#[test_case(
    Some(monday_now() + Duration::days(60));
    "deadline_beyond_horizon_no_warning"
)]
fn no_split_oversized_beyond_horizon_no_warning(deadline: Option<DateTime<Utc>>) {
    let (store, now) = huge_no_split_task_store(deadline);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    let task_chunks = chunks_for_task(&result, "task-huge");
    assert!(task_chunks.is_empty(), "oversized task must not be placed");

    // No warnings for this task: deadline is beyond/absent horizon, so the
    // filter suppresses the engine's Unschedulable output.
    let task_warnings = warnings_for_task(&result, "task-huge");
    assert!(
        task_warnings.is_empty(),
        "no warning expected for out-of-horizon backlog; got {task_warnings:?}"
    );
}

#[test]
fn splittable_in_horizon_deadline_violation_kept() {
    let (store, now) = test_fixture();
    let deadline = now + Duration::hours(2); // 12:00 on the same day — very tight

    let mut task = make_schedulable_task("task-tight", Priority::High, 600);
    task.no_split = false;
    task.deadline = Some(deadline);
    seed_task(&store, &task);

    let result = reschedule(&store, &DefaultScheduler, now).expect("reschedule should succeed");

    assert!(
        result
            .placed_chunks
            .iter()
            .any(|c| c.task_id == "task-tight"),
        "splittable task should have at least one chunk placed"
    );

    let dv_warnings: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| {
            w.task_id == "task-tight" && matches!(w.kind, WarningKind::DeadlineViolation { .. })
        })
        .collect();
    assert_eq!(
        dv_warnings.len(),
        1,
        "exactly one DeadlineViolation expected for in-horizon tight deadline; \
         got {dv_warnings:?}"
    );

    if let WarningKind::DeadlineViolation {
        deadline: payload_dl,
        ..
    } = dv_warnings[0].kind
    {
        assert_eq!(
            payload_dl, deadline,
            "warning payload deadline must match the task deadline"
        );
    }
}

#[test]
fn incremental_no_split_oversized_no_deadline_no_warning() {
    let (store, now) = huge_no_split_task_store(None);

    let result = reschedule_incremental(&store, &DefaultScheduler, &["task-huge".to_owned()], now)
        .expect("incremental reschedule should succeed");

    let task_warnings = warnings_for_task(&result, "task-huge");
    assert!(
        task_warnings.is_empty(),
        "no warning expected for out-of-horizon backlog on incremental path; \
         got {task_warnings:?}"
    );
}

#[test]
/// Completing a task triggers `policy_for(TaskCompleted)` → `Full` reschedule.
/// An incremental reschedule for the completed task's id would be a no-op (the task
/// is no longer schedulable), leaving task-b pending until the next background run —
/// which is why `TaskCompleted` maps to `Full`. This test pins that the freed slots
/// are immediately reclaimed by waiting tasks on the next full reschedule.
fn reschedule_after_task_completed_reclaims_freed_slot() {
    // Monday-only schedule, 10:00–11:30 UTC (90 min/week).
    // 30-day horizon → 5 Mondays → 450 min total capacity.
    let store = test_store_with_config(default_config());
    store
        .delete_schedule(&default_schedule_id(&store))
        .expect("drop migration-seeded default schedule");
    let narrow_window = ScheduleWindow {
        id: "win-mon-narrow".to_owned(),
        schedule_id: "sched-default".to_owned(),
        day_of_week: Weekday::Mon,
        start_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(11, 30, 0).unwrap(),
    };
    let narrow_schedule = Schedule::test_default()
        .with_id("sched-default")
        .with_default(true)
        .with_windows(vec![narrow_window]);
    store
        .create_schedule(&narrow_schedule)
        .expect("create narrow monday-only schedule");

    let now = monday_now();

    let mut task_a = make_schedulable_task("task-a", Priority::High, 450);
    task_a.deadline = None;
    seed_task(&store, &task_a);

    let mut task_b = make_schedulable_task("task-b", Priority::Medium, 90);
    task_b.deadline = None;
    seed_task(&store, &task_b);

    reschedule(&store, &DefaultScheduler, now).expect("initial reschedule should succeed");
    let t_b_before = stored_task(&store, "task-b").unwrap();
    assert_eq!(
        t_b_before.status,
        TaskStatus::Pending,
        "task-b must be Pending when all Monday slots are occupied by task-a"
    );

    complete_task(&store, "task-a", now).expect("complete task-a should succeed");

    reschedule(&store, &DefaultScheduler, now).expect("post-completion reschedule should succeed");
    let t_b_after = stored_task(&store, "task-b").unwrap();
    assert_eq!(
        t_b_after.status,
        TaskStatus::Scheduled,
        "task-b must be Scheduled after full reschedule reclaims task-a's freed slots"
    );
}
