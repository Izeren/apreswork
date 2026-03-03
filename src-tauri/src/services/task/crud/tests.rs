// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for task CRUD (child module of `services::task::crud`).

use chrono::{DateTime, Utc};
use test_case::test_case;

use super::{create_task, delete_task, get_task, update_task};
use crate::domain::enums::{Priority, TaskStatus};
use crate::domain::inputs::{CreateTaskInput, UpdateTaskInput};
use crate::domain::models::{Schedule, Task};
use crate::error::AppError;
use crate::services::task::test_helpers::{make_completed_chunk, make_scheduled_chunk, make_task};
use crate::test_support::{
    default_schedule_id, schedule_with_window, seed_chunk, seed_task, test_now, test_store, utc,
};
use crate::traits::storage::{ChunkStore, ScheduleStore, Store, TaskStore};

fn valid_create_input() -> CreateTaskInput {
    CreateTaskInput {
        title: "Read a book".to_owned(),
        description: Some("Chapter 1".to_owned()),
        ..CreateTaskInput::test_default()
    }
}

fn empty_update_input() -> UpdateTaskInput {
    UpdateTaskInput::default()
}

/// Set `input.status = Some(target)`, run `update_task` for `"task-1"`,
/// assert it succeeds, and assert the resulting task's status is `target`.
/// Shared by tests that only check the transition succeeded, not side effects.
fn assert_status_transition_succeeds(
    store: &dyn Store,
    mut input: UpdateTaskInput,
    target: TaskStatus,
) -> Task {
    input.status = Some(target);
    let updated =
        update_task(store, "task-1", input, test_now()).expect("transition should succeed");
    assert_eq!(updated.status, target);
    updated
}

#[test]
fn create_task_happy_path() {
    let store = test_store();
    let input = valid_create_input();
    let task = create_task(&store, input, test_now()).expect("should succeed");

    assert_eq!(task.title, "Read a book");
    assert_eq!(task.description, Some("Chapter 1".to_owned()));
    assert_eq!(task.duration_minutes, 60);
    assert_eq!(task.time_logged_minutes, 0);
    assert_eq!(task.priority, Priority::Medium);
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.min_chunk_minutes, 30);
    assert!(!task.no_split);
    assert!(task.labels.is_empty());
    assert!(task.recurring_template_id.is_none());
    assert!(task.deadline.is_some());

    assert!(store.get_task(&task.id).unwrap().is_some());
}

#[test_case(20, Some(30), true  ; "duration below min_chunk")]
#[test_case(30, Some(30), true  ; "duration equals min_chunk")]
#[test_case(60, Some(30), false ; "duration above min_chunk")]
#[test_case(25, None,     true  ; "duration below default min_chunk")]
#[test_case(30, None,     true  ; "duration equals default min_chunk")]
#[test_case(60, None,     false ; "duration above default min_chunk")]
fn create_task_auto_no_split(duration: i64, min_chunk: Option<i64>, expected_no_split: bool) {
    let store = test_store();
    let mut input = valid_create_input();
    input.duration_minutes = duration;
    input.min_chunk_minutes = min_chunk;
    input.no_split = Some(false); // explicitly false, should be overridden when applicable
    let task = create_task(&store, input, test_now()).expect("should succeed");
    assert_eq!(task.no_split, expected_no_split);
}

#[test]
fn create_task_no_auto_no_split_respects_true() {
    let store = test_store();
    let mut input = valid_create_input();
    input.duration_minutes = 120;
    input.min_chunk_minutes = Some(30);
    input.no_split = Some(true);
    let task = create_task(&store, input, test_now()).expect("should succeed");
    assert!(task.no_split);
}

#[test]
fn create_task_validation_error() {
    let store = test_store();
    let mut input = valid_create_input();
    input.duration_minutes = 0;
    let result = create_task(&store, input, test_now());
    assert!(matches!(result, Err(AppError::Validation(_))));
}

#[test]
fn create_task_uses_default_schedule() {
    let store = test_store();
    let input = valid_create_input();
    // schedule_id is None in the input
    let task = create_task(&store, input, test_now()).expect("should succeed");
    assert_eq!(task.schedule_id, default_schedule_id(&store));
}

#[test]
fn create_task_with_explicit_schedule() {
    let store = test_store();
    // Needs a window ≥ min_chunk (30 min default) for the splittable task.
    store
        .create_schedule(&schedule_with_window("my-custom-schedule", 60))
        .expect("seed schedule");
    let mut input = valid_create_input();
    input.schedule_id = Some("my-custom-schedule".to_owned());
    let task = create_task(&store, input, test_now()).expect("should succeed");
    assert_eq!(task.schedule_id, "my-custom-schedule");
}

#[test]
fn create_task_with_labels() {
    let store = test_store();
    let mut input = valid_create_input();
    input.labels = Some(vec!["urgent".to_owned(), "reading".to_owned()]);
    let task = create_task(&store, input, test_now()).expect("should succeed");
    assert_eq!(task.labels, vec!["urgent", "reading"]);
}

#[test]
fn create_task_with_status_backlog() {
    let store = test_store();
    let mut input = valid_create_input();
    input.status = Some(TaskStatus::Backlog);
    let task = create_task(&store, input, test_now()).expect("should succeed");
    assert_eq!(task.status, TaskStatus::Backlog);
}

#[test]
fn create_task_rejects_splittable_min_chunk_exceeds_largest_window() {
    let store = test_store();
    // Schedule: 25-min window; task min_chunk=30 (splittable, 30 > 25 → fail)
    store
        .create_schedule(&schedule_with_window("tiny-sched", 25))
        .expect("seed schedule");
    let mut input = valid_create_input();
    input.schedule_id = Some("tiny-sched".to_owned());
    input.min_chunk_minutes = Some(30);
    input.duration_minutes = 120;
    let result = create_task(&store, input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn create_task_rejects_no_split_duration_exceeds_largest_window() {
    let store = test_store();
    // Schedule: 50-min window; task duration=60, no_split=true → 60 > 50 → fail
    store
        .create_schedule(&schedule_with_window("small-sched", 50))
        .expect("seed schedule");
    let mut input = valid_create_input();
    input.schedule_id = Some("small-sched".to_owned());
    input.duration_minutes = 60;
    input.no_split = Some(true);
    let result = create_task(&store, input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn create_task_rejects_auto_no_split_duration_exceeds_largest_window() {
    let store = test_store();
    // Schedule: 19-min window; duration=20, min_chunk=30 → auto-no-split (20≤30),
    // then check duration(20) > largest(19) → fail
    store
        .create_schedule(&schedule_with_window("micro-sched", 19))
        .expect("seed schedule");
    let mut input = valid_create_input();
    input.schedule_id = Some("micro-sched".to_owned());
    input.duration_minutes = 20;
    input.min_chunk_minutes = Some(30);
    input.no_split = Some(false);
    let result = create_task(&store, input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn create_task_bogus_explicit_schedule_id_returns_not_found() {
    let store = test_store();
    let mut input = valid_create_input();
    input.schedule_id = Some("no-such-schedule".to_owned());
    let result = create_task(&store, input, test_now());
    assert!(
        matches!(result, Err(AppError::NotFound { .. })),
        "expected NotFound for bogus schedule_id, got: {result:?}"
    );
}

#[test]
fn get_task_found() {
    let store = test_store();
    let original = make_task("task-1");
    seed_task(&store, &original);

    let task = get_task(&store, "task-1").expect("should succeed");
    assert_eq!(task.id, "task-1");
    assert_eq!(task.title, "Test task");
}

#[test]
fn get_task_not_found() {
    let store = test_store();
    let result = get_task(&store, "nonexistent");
    assert!(
        matches!(
            result,
            Err(AppError::NotFound { ref entity, ref id })
            if entity == "Task" && id == "nonexistent"
        ),
        "expected NotFound, got: {result:?}"
    );
}

#[test]
fn update_task_happy_path() {
    let store = test_store();
    let original = make_task("task-1");
    seed_task(&store, &original);

    let mut input = empty_update_input();
    input.title = Some("Updated title".to_owned());
    input.priority = Some(Priority::High);

    let updated = update_task(&store, "task-1", input, test_now()).expect("should succeed");
    assert_eq!(updated.title, "Updated title");
    assert_eq!(updated.priority, Priority::High);
    assert!(updated.updated_at >= original.updated_at);
}

#[test]
fn update_task_duration_floor() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.time_logged_minutes = 30;
    seed_task(&store, &task);

    let mut input = empty_update_input();
    input.duration_minutes = Some(29); // less than time_logged_minutes

    let result = update_task(&store, "task-1", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(ref msg)) if msg.contains("time_logged_minutes")),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn update_task_duration_floor_exact() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.time_logged_minutes = 30;
    seed_task(&store, &task);

    let mut input = empty_update_input();
    input.duration_minutes = Some(30); // exactly equal — should be OK

    let updated = update_task(&store, "task-1", input, test_now()).expect("should succeed");
    assert_eq!(updated.duration_minutes, 30);
}

#[test]
fn update_task_partial_patch() {
    let store = test_store();
    let mut original = make_task("task-1");
    original.title = "Original title".to_owned();
    original.description = Some("Original desc".to_owned());
    original.priority = Priority::Low;
    original.labels = vec!["label1".to_owned()];
    seed_task(&store, &original);

    let mut input = empty_update_input();
    input.title = Some("New title".to_owned());

    let updated = update_task(&store, "task-1", input, test_now()).expect("should succeed");
    assert_eq!(updated.title, "New title");
    // Unchanged fields
    assert_eq!(updated.description, Some("Original desc".to_owned()));
    assert_eq!(updated.priority, Priority::Low);
    assert_eq!(updated.labels, vec!["label1"]);
    assert_eq!(updated.duration_minutes, original.duration_minutes);
}

#[test]
fn update_task_clear_description() {
    let store = test_store();
    let mut original = make_task("task-1");
    original.description = Some("Has a description".to_owned());
    seed_task(&store, &original);

    let mut input = empty_update_input();
    input.description = Some(None); // clear the description

    let updated = update_task(&store, "task-1", input, test_now()).expect("should succeed");
    assert_eq!(updated.description, None);
}

#[test]
fn update_task_not_found() {
    let store = test_store();
    let input = empty_update_input();
    let result = update_task(&store, "nonexistent", input, test_now());
    assert!(
        matches!(result, Err(AppError::NotFound { .. })),
        "expected NotFound, got: {result:?}"
    );
}

#[test]
fn update_task_validation_error() {
    let store = test_store();
    let original = make_task("task-1");
    seed_task(&store, &original);

    let mut input = empty_update_input();
    input.duration_minutes = Some(0); // invalid

    let result = update_task(&store, "task-1", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn update_task_rejects_raising_min_chunk_beyond_largest_window() {
    let store = test_store();
    // Schedule: 50-min window; task min_chunk=30 (splittable 30≤50 → fits)
    store
        .create_schedule(&schedule_with_window("sched-50", 50))
        .expect("seed schedule");
    let task = Task {
        id: "task-cap1".to_owned(),
        duration_minutes: 120,
        min_chunk_minutes: 30,
        no_split: false,
        schedule_id: "sched-50".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    // Raise min_chunk to 60 → 60 > 50 → fail
    let mut input = empty_update_input();
    input.min_chunk_minutes = Some(60);
    let result = update_task(&store, "task-cap1", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn update_task_rejects_schedule_reassignment_that_makes_task_unfit() {
    let store = test_store();
    // Big schedule (task currently fits)
    store
        .create_schedule(&schedule_with_window("big-sched", 120))
        .expect("seed big-sched");
    // Small schedule (task won't fit after reassignment)
    store
        .create_schedule(&schedule_with_window("small-sched", 25))
        .expect("seed small-sched");

    let task = Task {
        id: "task-cap2".to_owned(),
        duration_minutes: 120,
        min_chunk_minutes: 30,
        no_split: false,
        schedule_id: "big-sched".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    // Reassign to small schedule: min_chunk=30 > 25 → fail
    let mut input = empty_update_input();
    input.schedule_id = Some("small-sched".to_owned());
    let result = update_task(&store, "task-cap2", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn update_task_rejects_no_split_toggle_when_duration_does_not_fit() {
    let store = test_store();
    // Schedule: 50-min window; task duration=60, min_chunk=30, no_split=false
    // Splittable check passes (min_chunk=30 ≤ 50); no_split=false is stored.
    store
        .create_schedule(&schedule_with_window("sched-50b", 50))
        .expect("seed schedule");
    let task = Task {
        id: "task-cap3".to_owned(),
        duration_minutes: 60,
        min_chunk_minutes: 30,
        no_split: false,
        schedule_id: "sched-50b".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    // Set no_split=true: now check duration(60) > largest(50) → fail
    let mut input = empty_update_input();
    input.no_split = Some(true);
    let result = update_task(&store, "task-cap3", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test_case(TaskStatus::Completed ; "completed")]
#[test_case(TaskStatus::Cancelled ; "cancelled")]
fn update_task_terminal_task_cosmetic_edit_skips_capacity_validation(status: TaskStatus) {
    let store = test_store();
    // Window-less schedule → largest_window = 0; any task would fail if validated.
    store
        .create_schedule(&Schedule::test_default().with_id("zero-win-sched"))
        .expect("seed schedule");
    let task = Task {
        id: "task-done".to_owned(),
        status,
        duration_minutes: 60,
        min_chunk_minutes: 30,
        no_split: false,
        schedule_id: "zero-win-sched".to_owned(),
        ..Task::test_default()
    };
    // Bypass service: insert directly so capacity check is not run on create.
    store.create_task(&task).expect("seed terminal task");

    // Title-only edit on a terminal task — capacity validation must be skipped.
    let mut input = empty_update_input();
    input.title = Some("Renamed".to_owned());
    let result = update_task(&store, "task-done", input, test_now());
    assert!(
        result.is_ok(),
        "cosmetic edit on terminal task must bypass capacity check: {result:?}"
    );
    assert_eq!(result.unwrap().title, "Renamed");
}

#[test]
fn update_task_terminal_task_bogus_schedule_reassignment_returns_not_found() {
    let store = test_store();
    let task = Task {
        id: "task-done-bogus".to_owned(),
        status: TaskStatus::Completed,
        ..Task::test_default()
    };
    seed_task(&store, &task);

    // Even though capacity validation is skipped for terminal tasks, the
    // schedule itself must still resolve: bogus reassignment → NotFound,
    // never a DB FK error.
    let mut input = empty_update_input();
    input.schedule_id = Some("no-such-schedule".to_owned());
    let result = update_task(&store, "task-done-bogus", input, test_now());
    assert!(
        matches!(result, Err(AppError::NotFound { .. })),
        "expected NotFound for bogus reassignment on terminal task, got: {result:?}"
    );
}

#[test]
fn update_task_bogus_schedule_reassignment_returns_not_found() {
    let store = test_store();
    seed_task(&store, &make_task("task-bogus-sched"));

    let mut input = empty_update_input();
    input.schedule_id = Some("nonexistent-schedule".to_owned());
    let result = update_task(&store, "task-bogus-sched", input, test_now());
    assert!(
        matches!(result, Err(AppError::NotFound { .. })),
        "expected NotFound for bogus schedule reassignment, got: {result:?}"
    );
}

/// A recurring instance with window [03-02 00:00, expire 03-09 23:59].
fn recurring_instance(id: &str) -> Task {
    let base = make_task(id)
        .with_template("tmpl-x")
        .with_deadline(utc(2026, 3, 2, 23, 59))
        .with_expire_at(utc(2026, 3, 9, 23, 59));
    Task {
        start_date: Some(utc(2026, 3, 2, 0, 0)),
        ..base
    }
}

#[test]
fn update_task_rejects_start_date_change_on_recurring_instance() {
    let store = test_store();
    seed_task(&store, &recurring_instance("inst-1"));

    let mut input = empty_update_input();
    input.start_date = Some(Some(utc(2026, 3, 3, 0, 0))); // different from anchor

    let result = update_task(&store, "inst-1", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(ref m)) if m.contains("start_date cannot be changed")),
        "expected start_date Validation error, got: {result:?}"
    );
}

#[test]
fn update_task_allows_unchanged_start_date_on_recurring_instance() {
    // The edit form always re-sends start_date; the same value must be a no-op,
    // not a rejection.
    let store = test_store();
    seed_task(&store, &recurring_instance("inst-1"));

    let mut input = empty_update_input();
    input.start_date = Some(Some(utc(2026, 3, 2, 0, 0))); // identical to anchor
    input.title = Some("Renamed".to_owned());

    let updated = update_task(&store, "inst-1", input, test_now()).expect("no-op start allowed");
    assert_eq!(updated.title, "Renamed");
}

#[test_case(utc(2026, 3, 1, 0, 0),   false ; "before start rejected")]
#[test_case(utc(2026, 3, 2, 0, 0),   true  ; "exactly start allowed")]
#[test_case(utc(2026, 3, 5, 12, 0),  true  ; "within window allowed")]
#[test_case(utc(2026, 3, 9, 23, 59), true  ; "exactly expiry allowed")]
#[test_case(utc(2026, 3, 10, 0, 0),  false ; "after expiry rejected")]
fn update_task_recurring_instance_deadline_bounds(deadline: DateTime<Utc>, ok: bool) {
    let store = test_store();
    seed_task(&store, &recurring_instance("inst-1"));

    let mut input = empty_update_input();
    input.deadline = Some(deadline);

    let result = update_task(&store, "inst-1", input, test_now());
    assert_eq!(result.is_ok(), ok, "deadline {deadline}: {result:?}");
    if ok {
        assert_eq!(result.expect("ok").deadline, Some(deadline));
    }
}

#[test]
fn update_task_allows_timing_changes_on_non_recurring_task() {
    // A normal (non-recurring) task has no cadence anchor — start and deadline
    // are freely editable (no window constraint), as long as start ≤ deadline.
    let store = test_store();
    seed_task(&store, &make_task("task-1"));

    let mut input = empty_update_input();
    input.start_date = Some(Some(utc(2026, 1, 1, 0, 0)));
    input.deadline = Some(utc(2026, 9, 1, 0, 0));

    let updated = update_task(&store, "task-1", input, test_now()).expect("free timing edit");
    assert_eq!(updated.start_date, Some(utc(2026, 1, 1, 0, 0)));
    assert_eq!(updated.deadline, Some(utc(2026, 9, 1, 0, 0)));
}

#[test_case(TaskStatus::Backlog, TaskStatus::Pending, true  ; "backlog to pending allowed")]
#[test_case(TaskStatus::Pending, TaskStatus::Backlog, true  ; "pending to backlog allowed")]
#[test_case(TaskStatus::Scheduled, TaskStatus::Backlog, true ; "scheduled to backlog allowed")]
#[test_case(TaskStatus::Pending, TaskStatus::Scheduled, false ; "pending to scheduled rejected")]
#[test_case(TaskStatus::Backlog, TaskStatus::Cancelled, false ; "backlog to cancelled rejected")]
#[test_case(TaskStatus::Completed, TaskStatus::Pending, false ; "completed to pending rejected")]
fn update_task_status_transition(from: TaskStatus, to: TaskStatus, should_succeed: bool) {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = from;
    seed_task(&store, &task);

    let mut input = empty_update_input();
    input.status = Some(to);

    let result = update_task(&store, "task-1", input, test_now());
    if should_succeed {
        let updated = result.expect("transition should succeed");
        assert_eq!(updated.status, to);
    } else {
        assert!(
            matches!(result, Err(AppError::Validation(ref msg)) if msg.contains("not allowed")),
            "expected Validation error, got: {result:?}"
        );
    }
}

/// Leaving `Scheduled` for `Backlog` must delete the task's non-fixed,
/// non-completed ("auto") chunks — otherwise they linger as ghosts on the
/// calendar, since the incremental reschedule this transition used to
/// trigger excludes Backlog tasks from `get_schedulable_tasks`. Fixed
/// (user-pinned) chunks and completed chunks (history) are kept — unlike
/// `cancel_task` (lifecycle.rs), which deletes all scheduled chunks
/// regardless of `is_fixed`.
#[test]
fn update_task_scheduled_to_backlog_deletes_auto_chunks_only() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Scheduled;
    seed_task(&store, &task);

    seed_chunk(
        &store,
        &make_scheduled_chunk("chunk-auto", "task-1", 30).with_fixed(false),
    );
    seed_chunk(
        &store,
        &make_scheduled_chunk("chunk-fixed", "task-1", 30).with_fixed(true),
    );
    seed_chunk(
        &store,
        &make_completed_chunk("chunk-completed", "task-1", 30),
    );

    assert_status_transition_succeeds(&store, empty_update_input(), TaskStatus::Backlog);

    assert!(
        store.get_chunk("chunk-auto").unwrap().is_none(),
        "auto (non-fixed, scheduled) chunk should be deleted"
    );
    assert!(
        store.get_chunk("chunk-fixed").unwrap().is_some(),
        "fixed chunk should be kept"
    );
    assert!(
        store.get_chunk("chunk-completed").unwrap().is_some(),
        "completed chunk should be kept as history"
    );
}

#[test]
fn update_task_pending_to_backlog_no_chunks_succeeds() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Pending;
    seed_task(&store, &task);

    assert_status_transition_succeeds(&store, empty_update_input(), TaskStatus::Backlog);
}

#[test]
fn update_task_backlog_to_pending_leaves_chunks_untouched() {
    let store = test_store();
    let mut task = make_task("task-1");
    task.status = TaskStatus::Backlog;
    seed_task(&store, &task);
    seed_chunk(
        &store,
        &make_scheduled_chunk("chunk-auto", "task-1", 30).with_fixed(false),
    );

    assert_status_transition_succeeds(&store, empty_update_input(), TaskStatus::Pending);
    assert!(
        store.get_chunk("chunk-auto").unwrap().is_some(),
        "reverse transition (Backlog to Pending) must not delete any chunks"
    );
}

#[test]
fn create_task_rejects_start_after_deadline() {
    let store = test_store();
    let mut input = valid_create_input();
    input.deadline = utc(2026, 6, 1, 0, 0);
    input.start_date = Some(utc(2026, 6, 10, 0, 0)); // after deadline
    let result = create_task(&store, input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error for start > deadline, got: {result:?}"
    );
}

#[test_case(utc(2026, 6, 1, 0, 0), utc(2026, 6, 10, 0, 0), true  ; "start before deadline ok")]
#[test_case(utc(2026, 6, 1, 0, 0), utc(2026, 6, 1, 0, 0),  true  ; "start equals deadline ok")]
#[test_case(utc(2026, 6, 10, 0, 0), utc(2026, 6, 1, 0, 0), false ; "start after deadline fail")]
fn update_task_date_patch_both(start: DateTime<Utc>, deadline: DateTime<Utc>, ok: bool) {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));
    let mut input = empty_update_input();
    input.start_date = Some(Some(start));
    input.deadline = Some(deadline);
    let result = update_task(&store, "task-1", input, test_now());
    assert_eq!(
        result.is_ok(),
        ok,
        "start={start}, deadline={deadline}: {result:?}"
    );
}

#[test_case(
    None, Some(utc(2026, 6, 10, 0, 0)),
    Some(Some(utc(2026, 6, 15, 0, 0))), None ;
    "start_date patch after existing deadline"
)]
#[test_case(
    Some(utc(2026, 6, 1, 0, 0)), Some(utc(2026, 6, 10, 0, 0)),
    None, Some(utc(2026, 5, 1, 0, 0)) ;
    "deadline patch before existing start_date"
)]
#[allow(clippy::option_option)] // UpdateTaskInput.start_date is Option<Option<T>>: None=no-patch, Some(None)=clear, Some(Some(v))=set
fn update_task_rejects_inconsistent_date_patch(
    task_start: Option<DateTime<Utc>>,
    task_deadline: Option<DateTime<Utc>>,
    input_start: Option<Option<DateTime<Utc>>>,
    input_deadline: Option<DateTime<Utc>>,
) {
    let store = test_store();
    let mut task = make_task("task-1");
    task.start_date = task_start;
    task.deadline = task_deadline;
    seed_task(&store, &task);

    let mut input = empty_update_input();
    input.start_date = input_start;
    input.deadline = input_deadline;

    let result = update_task(&store, "task-1", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test_case(TaskStatus::Completed ; "completed terminal task skips date check")]
#[test_case(TaskStatus::Cancelled ; "cancelled terminal task skips date check")]
fn update_task_terminal_task_skips_date_validation(status: TaskStatus) {
    let store = test_store();
    // Bypass service validation: seed directly with inverted dates.
    let mut task = make_task("task-term");
    task.status = status;
    task.start_date = Some(utc(2026, 9, 1, 0, 0));
    task.deadline = Some(utc(2026, 1, 1, 0, 0)); // start > deadline (inverted)
    seed_task(&store, &task);

    // Cosmetic description edit — must succeed for terminal tasks regardless of
    // legacy bad dates (capacity and date checks are both skipped).
    let mut input = empty_update_input();
    input.description = Some(Some("edited description".to_owned()));
    let result = update_task(&store, "task-term", input, test_now());
    assert!(
        result.is_ok(),
        "terminal task cosmetic edit must bypass date validation: {result:?}"
    );
}

#[test]
fn update_task_blank_title_rejected() {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));

    let mut input = empty_update_input();
    input.title = Some("  ".to_owned());

    let result = update_task(&store, "task-1", input, test_now());
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error for blank title, got: {result:?}"
    );
}

#[test]
fn update_task_stamps_injected_now() {
    let store = test_store();
    seed_task(&store, &make_task("task-1"));
    let now = utc(2026, 5, 15, 10, 0);
    let updated = update_task(&store, "task-1", empty_update_input(), now).expect("should succeed");
    assert_eq!(updated.updated_at, now);
}

#[test]
fn create_task_stamps_injected_now() {
    let store = test_store();
    let now = utc(2026, 5, 15, 10, 0);
    let task = create_task(&store, valid_create_input(), now).expect("should succeed");
    assert_eq!(task.created_at, now);
    assert_eq!(task.updated_at, now);
}

#[test]
fn delete_task_non_recurring() {
    let store = test_store();
    let task = make_task("task-1");
    seed_task(&store, &task);

    delete_task(&store, "task-1", test_now()).expect("should succeed");

    assert!(store.get_task("task-1").unwrap().is_none());
}

#[test]
fn delete_task_recurring_cancels() {
    let store = test_store();
    let task = make_task("task-1").with_template("template-1");
    seed_task(&store, &task);

    delete_task(&store, "task-1", test_now()).expect("should succeed");

    // Task should still exist but be cancelled
    let cancelled = store
        .get_task("task-1")
        .unwrap()
        .expect("task should still exist");
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
}

#[test]
fn delete_task_recurring_stamps_updated_at() {
    let store = test_store();
    let task = make_task("task-1").with_template("template-1");
    seed_task(&store, &task);
    let now = utc(2026, 5, 15, 10, 0);

    delete_task(&store, "task-1", now).expect("should succeed");

    let cancelled = store
        .get_task("task-1")
        .unwrap()
        .expect("task should still exist");
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    assert_eq!(cancelled.updated_at, now);
}

#[test]
fn delete_task_not_found() {
    let store = test_store();
    let result = delete_task(&store, "nonexistent", test_now());
    assert!(
        matches!(result, Err(AppError::NotFound { .. })),
        "expected NotFound, got: {result:?}"
    );
}
