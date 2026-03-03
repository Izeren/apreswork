// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the full `reschedule` orchestration flow.

use chrono::{DateTime, NaiveTime, TimeZone, Utc, Weekday};
use test_case::test_case;

use super::{
    assert_reschedule_empty_updates_config, chunk_count, make_auto_chunk, make_chunk_at,
    make_fixed_chunk, make_task, stored_task, MockScheduler,
};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::TaskStatus;
use crate::domain::inputs::TaskFilter;
use crate::domain::models::{
    AppConfig, ExternalEventRecord, RecurringTemplate, Schedule, ScheduleWindow, Task,
};
use crate::error::AppError;
use crate::services::scheduling::reschedule;
use crate::test_support::{
    default_config, seed_chunk, seed_task, seed_template, test_now, test_store_with_config,
};
use crate::traits::storage::{
    ChunkStore, ConfigStore, ExternalEventStore, ScheduleStore, TaskStore,
};

fn reschedule_empty_then_task(store: &SqliteStore, task_id: &str) -> (Task, DateTime<Utc>) {
    let scheduler = MockScheduler::empty();
    let now = test_now();
    reschedule(store, &scheduler, now).expect("reschedule should succeed");
    (stored_task(store, task_id).expect("task should exist"), now)
}

#[test]
fn reschedule_empty_no_tasks() {
    let store = test_store_with_config(default_config());
    let scheduler = MockScheduler::empty();
    let now = test_now();

    assert_reschedule_empty_updates_config(&store, &scheduler, now);
}

#[test]
fn reschedule_places_chunks_and_updates_status() {
    let store = test_store_with_config(default_config());
    let task = make_task("task-1", TaskStatus::Pending);
    seed_task(&store, &task);

    let new_chunk = make_auto_chunk("chunk-new-1", "task-1");
    let scheduler = MockScheduler::with_chunks(vec![new_chunk.clone()]);
    let now = test_now();

    reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    assert!(store.get_chunk("chunk-new-1").unwrap().is_some());

    let updated = stored_task(&store, "task-1").expect("task should exist");
    assert_eq!(updated.status, TaskStatus::Scheduled);
    assert_eq!(updated.updated_at, now);
}

#[test]
fn reschedule_removes_old_auto_chunks() {
    let store = test_store_with_config(default_config());
    let task = make_task("task-1", TaskStatus::Pending);
    seed_task(&store, &task);

    let now = test_now();
    let old1 = make_chunk_at(
        "old-chunk-1",
        "task-1",
        now + chrono::Duration::hours(1),
        now + chrono::Duration::hours(2),
        None,
    );
    let old2 = make_chunk_at(
        "old-chunk-2",
        "task-1",
        now + chrono::Duration::hours(3),
        now + chrono::Duration::hours(4),
        None,
    );
    seed_chunk(&store, &old1);
    seed_chunk(&store, &old2);

    let new_chunk = make_chunk_at(
        "chunk-new-1",
        "task-1",
        now + chrono::Duration::hours(5),
        now + chrono::Duration::hours(6),
        None,
    );
    let scheduler = MockScheduler::with_chunks(vec![new_chunk]);

    reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    // Diff pairing: new_chunk starts at now+5h; old-chunk-2 (now+3h, 2h away) is closer
    // than old-chunk-1 (now+1h, 4h away) → old-chunk-2 is UPDATEd, old-chunk-1 is DELETEd.
    assert_eq!(chunk_count(&store), 1);
    assert!(store.get_chunk("old-chunk-1").unwrap().is_none());
}

#[test]
fn reschedule_pending_when_no_chunks_placed() {
    let store = test_store_with_config(default_config());
    let task = make_task("task-1", TaskStatus::Scheduled);
    seed_task(&store, &task);

    let (updated, now) = reschedule_empty_then_task(&store, "task-1");
    assert_eq!(updated.status, TaskStatus::Pending);
    assert_eq!(updated.updated_at, now);
}

#[test]
fn reschedule_fixed_chunks_keep_task_scheduled() {
    let store = test_store_with_config(default_config());
    let task = make_task("task-1", TaskStatus::Scheduled);
    seed_task(&store, &task);

    seed_chunk(&store, &make_fixed_chunk("fixed-chunk-1", "task-1"));

    let (updated, _now) = reschedule_empty_then_task(&store, "task-1");
    assert_eq!(updated.status, TaskStatus::Scheduled);
}

#[test]
fn reschedule_reconciles_recurring_instances() {
    let store = test_store_with_config(default_config());
    let now = test_now();

    // Add an active weekly template.
    let template = RecurringTemplate {
        id: "tmpl-1".to_owned(),
        title: "Gym".to_owned(),
        schedule_id: "sched-1".to_owned(),
        ..RecurringTemplate::test_default()
    };
    seed_template(&store, &template);

    let scheduler = MockScheduler::empty();

    reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    let tasks = store.list_tasks(&TaskFilter::default()).unwrap();
    let instances: Vec<_> = tasks
        .iter()
        .filter(|t| t.recurring_template_id.as_deref() == Some("tmpl-1"))
        .collect();
    assert!(
        !instances.is_empty(),
        "expected at least one recurring instance to be created"
    );
}

#[test_case(test_now() ; "default_now")]
#[test_case(Utc.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap() ; "custom_now")]
fn reschedule_config_timestamps_use_provided_now(now: DateTime<Utc>) {
    let store = test_store_with_config(default_config());
    let scheduler = MockScheduler::empty();

    let cfg_before = store.get_config().unwrap();
    assert!(cfg_before.last_reschedule.is_none());
    assert!(cfg_before.last_mutation.is_none());

    reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    let cfg_after = store.get_config().unwrap();
    assert_eq!(cfg_after.last_reschedule, Some(now));
    assert_eq!(cfg_after.last_mutation, Some(now));
}

#[test_case("Not/ATimezone" ; "garbage_tz")]
#[test_case("Europe/Narnia" ; "fictional_tz")]
#[test_case("" ; "empty_tz")]
fn reschedule_invalid_timezone_returns_error(tz: &str) {
    let cfg = AppConfig {
        timezone: tz.to_owned(),
        ..default_config()
    };
    let store = test_store_with_config(cfg);
    let scheduler = MockScheduler::empty();
    let now = test_now();

    let err = reschedule(&store, &scheduler, now).expect_err("should fail with invalid timezone");

    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation error, got: {err:?}"
    );
}

/// Edge case: task with Pending status and no remaining time should not appear
/// in `get_schedulable_tasks` (`time_logged` == duration). Status stays Pending.
#[test]
fn reschedule_fully_logged_task_is_not_schedulable() {
    let store = test_store_with_config(default_config());

    let fully_logged_task = Task {
        id: "task-done".to_owned(),
        title: "Already done".to_owned(),
        time_logged_minutes: 60,
        schedule_id: "sched-1".to_owned(),
        ..Task::test_default()
    };
    seed_task(&store, &fully_logged_task);

    let scheduler = MockScheduler::empty();

    reschedule(&store, &scheduler, test_now()).expect("reschedule should succeed");

    let task = stored_task(&store, "task-done").expect("task should exist");
    assert_eq!(task.status, TaskStatus::Pending);
}

#[test]
fn reschedule_with_external_events_succeeds() {
    let store = test_store_with_config(default_config());
    let now = test_now();

    let event = ExternalEventRecord {
        id: "ev-1".to_owned(),
        calendar_id: "cal-1".to_owned(),
        event_id: "gcal-ev-1".to_owned(),
        title: "Busy meeting".to_owned(),
        description: None,
        start_time: now + chrono::Duration::hours(1),
        end_time: now + chrono::Duration::hours(2),
        busy: true,
        declined: false,
        all_day: false,
        updated_at: now,
    };
    let window_start = now - chrono::Duration::days(1);
    let window_end = now + chrono::Duration::days(30);
    store
        .replace_external_events_in_window("cal-1", window_start, window_end, &[event])
        .expect("seed external event");

    let scheduler = MockScheduler::empty();
    let result = reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    assert!(result.placed_chunks.is_empty());

    let cfg = store.get_config().unwrap();
    assert_eq!(cfg.last_reschedule, Some(now));
}

#[test]
fn reschedule_with_schedule_windows_succeeds() {
    let store = test_store_with_config(default_config());
    let now = test_now();

    let schedule = Schedule {
        id: "sched-1".to_owned(),
        name: "Windowed Schedule".to_owned(),
        is_default: true,
        windows: vec![ScheduleWindow {
            id: "win-1".to_owned(),
            schedule_id: "sched-1".to_owned(),
            day_of_week: Weekday::Mon,
            start_time: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
        }],
        created_at: now,
        updated_at: now,
    };
    store.create_schedule(&schedule).expect("create schedule");

    let task = make_task("task-1", TaskStatus::Pending);
    seed_task(&store, &task);

    let scheduler = MockScheduler::empty();
    let result = reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    assert!(result.placed_chunks.is_empty());
}

/// Edge case: two tasks, one gets a chunk and one doesn't — verify individual
/// status transitions are independent.
#[test]
fn reschedule_mixed_task_status_transitions() {
    let store = test_store_with_config(default_config());

    let task_a = make_task("task-a", TaskStatus::Pending);
    let task_b = make_task("task-b", TaskStatus::Scheduled);
    seed_task(&store, &task_a);
    seed_task(&store, &task_b);

    let chunk_for_a = make_auto_chunk("chunk-a-1", "task-a");
    let scheduler = MockScheduler::with_chunks(vec![chunk_for_a]);
    let now = test_now();

    reschedule(&store, &scheduler, now).expect("reschedule should succeed");

    let a = stored_task(&store, "task-a").unwrap();
    let b = stored_task(&store, "task-b").unwrap();

    assert_eq!(
        a.status,
        TaskStatus::Scheduled,
        "task-a should become Scheduled"
    );
    assert_eq!(
        b.status,
        TaskStatus::Pending,
        "task-b should revert to Pending"
    );
}
