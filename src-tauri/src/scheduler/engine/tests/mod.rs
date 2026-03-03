// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared fixtures for the scheduler-engine test suite.
//! Concern-specific tests live in the sibling modules.

mod breaks;
mod placement;

use chrono::{DateTime, Utc};

use crate::domain::enums::ChunkStatus;
use crate::domain::models::{Chunk, Task};
use crate::test_support::{test_now, utc};
use crate::traits::scheduling::{
    AvailableSlot, ScheduleInput, ScheduleResult, Scheduler, WarningKind,
};

use super::DefaultScheduler;

fn make_slot(schedule_id: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> AvailableSlot {
    AvailableSlot {
        start,
        end,
        schedule_id: schedule_id.to_owned(),
    }
}

/// A single slot in schedule `s1` running 2026-03-16 18:00 → `end`.
fn slot_18_to(end: DateTime<Utc>) -> AvailableSlot {
    make_slot("s1", utc(2026, 3, 16, 18, 0), end)
}

/// The canonical single test slot: `s1`, 2026-03-16 18:00–19:00 (one hour).
fn default_slot() -> AvailableSlot {
    slot_18_to(utc(2026, 3, 16, 19, 0))
}

/// Build a minimal valid Task with sensible defaults.
fn make_task(id: &str, title: &str, schedule_id: &str, duration_minutes: i64) -> Task {
    Task {
        id: id.to_owned(),
        title: title.to_owned(),
        duration_minutes,
        deadline: None,
        schedule_id: schedule_id.to_owned(),
        ..Task::test_default()
    }
}

/// Build a fixed chunk between two times for a task.
fn make_fixed_chunk(task_id: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> Chunk {
    let now = test_now();
    Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task_id.to_owned(),
        start_time: start,
        end_time: end,
        status: ChunkStatus::Scheduled,
        is_fixed: true,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

/// Build a [`ScheduleInput`] with no fixed chunks and an explicit break policy.
fn make_input_with_policy(
    tasks: Vec<Task>,
    slots: Vec<AvailableSlot>,
    max_continuous_minutes: i64,
    min_break_minutes: i64,
) -> ScheduleInput {
    ScheduleInput {
        tasks,
        existing_fixed_chunks: Vec::new(),
        available_slots: slots,
        horizon_end: utc(2026, 12, 31, 23, 59),
        now: test_now(),
        max_continuous_minutes,
        min_break_minutes,
    }
}

/// Build a minimal [`ScheduleInput`] with no fixed chunks and the default break policy.
fn make_input(tasks: Vec<Task>, slots: Vec<AvailableSlot>) -> ScheduleInput {
    make_input_with_policy(tasks, slots, 120, 5)
}

fn run(tasks: Vec<Task>, slots: Vec<AvailableSlot>) -> ScheduleResult {
    DefaultScheduler
        .schedule(make_input(tasks, slots))
        .expect("schedule must not fail")
}

/// Schedule `tasks` into `slots` under an explicit break policy and unwrap.
fn run_with_policy(
    tasks: Vec<Task>,
    slots: Vec<AvailableSlot>,
    max_continuous_minutes: i64,
    min_break_minutes: i64,
) -> ScheduleResult {
    DefaultScheduler
        .schedule(make_input_with_policy(
            tasks,
            slots,
            max_continuous_minutes,
            min_break_minutes,
        ))
        .expect("schedule must not fail")
}

/// Schedule a single `task` against `slots` with one pre-existing `fixed` chunk,
/// under the default horizon/break policy, and unwrap the result.
fn run_with_fixed(task: Task, fixed: Chunk, slots: Vec<AvailableSlot>) -> ScheduleResult {
    let mut input = make_input(vec![task], slots);
    input.existing_fixed_chunks = vec![fixed];
    DefaultScheduler.schedule(input).expect("ok")
}

/// Schedule two tasks under the standard 60/10 break policy, assert exactly two
/// placed chunks and no warnings, and return the result.
fn run_two_tasks_expect_two_chunks(
    task_a: Task,
    task_b: Task,
    slots: Vec<AvailableSlot>,
) -> ScheduleResult {
    let result = run_with_policy(vec![task_a, task_b], slots, 60, 10);
    assert_eq!(result.placed_chunks.len(), 2, "expected 2 chunks");
    assert_no_warnings(&result);
    result
}

/// Assert the scheduler produced no warnings.
fn assert_no_warnings(result: &ScheduleResult) {
    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );
}

/// Assert the result carries exactly one `Unschedulable` warning.
fn assert_single_unschedulable(result: &ScheduleResult) {
    assert_eq!(result.warnings.len(), 1);
    assert!(matches!(
        result.warnings[0].kind,
        WarningKind::Unschedulable { .. }
    ));
}

/// Assert the scheduler placed nothing and warned about nothing.
fn assert_no_placement_no_warnings(result: &ScheduleResult) {
    assert!(result.placed_chunks.is_empty(), "expected no placed chunks");
    assert_no_warnings(result);
}

/// Find the placed chunk for `task_id`, panicking with a clear message if absent.
fn find_chunk<'a>(result: &'a ScheduleResult, task_id: &str) -> &'a Chunk {
    result
        .placed_chunks
        .iter()
        .find(|c| c.task_id == task_id)
        .unwrap_or_else(|| panic!("no placed chunk for {task_id}"))
}

/// Total placed minutes across all chunks.
fn total_placed_minutes(result: &ScheduleResult) -> i64 {
    result
        .placed_chunks
        .iter()
        .map(|c| (c.end_time - c.start_time).num_minutes())
        .sum()
}
