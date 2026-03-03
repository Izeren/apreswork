// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the schedule service (child module of `services::schedule`).

use chrono::{NaiveTime, Weekday};
use test_case::test_case;

use super::{create_schedule, delete_schedule, get_schedule, list_schedules, update_schedule};

use crate::domain::enums::TaskStatus;
use crate::domain::inputs::{CreateScheduleInput, ScheduleWindowInput, UpdateScheduleInput};
use crate::domain::models::{RecurringTemplate, Schedule, ScheduleWindow, Task};
use crate::error::AppError;
use crate::test_support::{
    assert_not_found, assert_validation, assert_validation_contains, default_schedule_id,
    seed_schedule, seed_task, seed_template, test_now, test_store,
};
use crate::traits::storage::{ConfigStore, RecurringTemplateStore, ScheduleStore, TaskStore};

/// Shorthand: `hm(18, 30)` -> `NaiveTime 18:30:00`
fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
}

fn make_window_input(day: Weekday, start: NaiveTime, end: NaiveTime) -> ScheduleWindowInput {
    ScheduleWindowInput {
        day_of_week: day,
        start_time: start,
        end_time: end,
    }
}

fn valid_create_input() -> CreateScheduleInput {
    CreateScheduleInput {
        name: "Evening".to_owned(),
        windows: vec![
            make_window_input(Weekday::Mon, hm(18, 0), hm(20, 0)),
            make_window_input(Weekday::Wed, hm(19, 0), hm(21, 0)),
        ],
    }
}

fn empty_update_input() -> UpdateScheduleInput {
    UpdateScheduleInput {
        name: None,
        windows: None,
    }
}

/// Single Tue 19:00–`end` window wrapped in an `UpdateScheduleInput`.
/// Used across all schedule-shrink/grow tests that need exactly one window.
fn single_tue_window(end: NaiveTime) -> UpdateScheduleInput {
    let mut input = empty_update_input();
    input.windows = Some(vec![make_window_input(Weekday::Tue, hm(19, 0), end)]);
    input
}

/// A non-default schedule with one Tuesday window. Name is derived from
/// `id` to satisfy the `schedules.name UNIQUE` constraint when multiple
/// schedules are seeded in the same test. Kept as a local helper because
/// it encodes module-specific window shape (Tue 19–21) used across
/// multiple update/delete tests.
fn make_custom_schedule(id: &str) -> Schedule {
    let now = test_now();
    Schedule {
        id: id.to_owned(),
        name: format!("Custom-{id}"),
        is_default: false,
        windows: vec![ScheduleWindow {
            id: format!("{id}-window-1"),
            schedule_id: id.to_owned(),
            day_of_week: Weekday::Tue,
            start_time: hm(19, 0),
            end_time: hm(21, 0),
        }],
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn create_schedule_happy_path() {
    let store = test_store();
    let input = valid_create_input();
    let schedule = create_schedule(&store, input, test_now()).expect("should succeed");

    assert_eq!(schedule.name, "Evening");
    assert!(!schedule.is_default);
    assert_eq!(schedule.windows.len(), 2);
    assert_eq!(schedule.created_at, test_now());
    assert_eq!(schedule.updated_at, test_now());

    for window in &schedule.windows {
        assert!(!window.id.is_empty());
        assert_eq!(window.schedule_id, schedule.id);
    }

    assert_eq!(schedule.windows[0].day_of_week, Weekday::Mon);
    assert_eq!(schedule.windows[0].start_time, hm(18, 0));
    assert_eq!(schedule.windows[0].end_time, hm(20, 0));

    assert_eq!(schedule.windows[1].day_of_week, Weekday::Wed);
    assert_eq!(schedule.windows[1].start_time, hm(19, 0));
    assert_eq!(schedule.windows[1].end_time, hm(21, 0));

    let persisted = store.get_schedule(&schedule.id).unwrap().unwrap();
    assert_eq!(persisted.created_at, test_now());
    assert_eq!(persisted.updated_at, test_now());
}

#[test]
fn create_schedule_generates_unique_ids() {
    let store = test_store();
    let s1 = create_schedule(&store, valid_create_input(), test_now()).unwrap();
    let mut input2 = valid_create_input();
    input2.name = "Evening-2".to_owned(); // schedules.name is UNIQUE
    let s2 = create_schedule(&store, input2, test_now()).unwrap();

    assert_ne!(s1.id, s2.id);
    assert_ne!(s1.windows[0].id, s2.windows[0].id);
}

#[test_case(""   ; "empty name")]
#[test_case("  " ; "whitespace-only name")]
fn create_schedule_empty_name_rejected(name: &str) {
    let store = test_store();
    let mut input = valid_create_input();
    input.name = name.to_owned();
    let result = create_schedule(&store, input, test_now());
    assert_validation(&result);
}

#[test]
fn create_schedule_empty_windows_rejected() {
    let store = test_store();
    let mut input = valid_create_input();
    input.windows = vec![];
    let result = create_schedule(&store, input, test_now());
    assert_validation(&result);
}

#[test]
fn create_schedule_overlapping_windows_rejected() {
    let store = test_store();
    let mut input = valid_create_input();
    input.windows = vec![
        make_window_input(Weekday::Mon, hm(18, 0), hm(20, 0)),
        make_window_input(Weekday::Mon, hm(19, 0), hm(21, 0)),
    ];
    let result = create_schedule(&store, input, test_now());
    assert_validation(&result);
}

#[test]
fn create_schedule_invalid_window_time_rejected() {
    let store = test_store();
    let mut input = valid_create_input();
    input.windows = vec![make_window_input(Weekday::Mon, hm(20, 0), hm(18, 0))];
    let result = create_schedule(&store, input, test_now());
    assert_validation(&result);
}

#[test]
fn create_schedule_windows_populated_with_ids() {
    let store = test_store();
    let input = valid_create_input();
    let schedule = create_schedule(&store, input, test_now()).unwrap();

    for window in &schedule.windows {
        assert!(!window.id.is_empty(), "window ID should be non-empty");
        assert_eq!(window.schedule_id, schedule.id);
    }

    let ids: Vec<&str> = schedule.windows.iter().map(|w| w.id.as_str()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "window IDs must be unique");
}

#[test]
fn get_schedule_found() {
    let store = test_store();
    let id = default_schedule_id(&store);
    let result = get_schedule(&store, &id);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().name, "Default");
}

#[test]
fn get_schedule_not_found() {
    let store = test_store();
    let result = get_schedule(&store, "nonexistent");
    assert_not_found(&result, "Schedule", "nonexistent");
}

#[test]
fn list_schedules_includes_default() {
    let store = test_store();
    let result = list_schedules(&store).expect("should succeed");
    assert_eq!(result.len(), 1);
    assert!(result[0].is_default);
}

#[test]
fn list_schedules_includes_custom() {
    let store = test_store();
    let custom = make_custom_schedule("sched-list");
    seed_schedule(&store, &custom);

    let result = list_schedules(&store).expect("should succeed");
    assert_eq!(result.len(), 2);
}

#[test]
fn update_schedule_name_change() {
    let store = test_store();
    let custom = make_custom_schedule("sched-1");
    seed_schedule(&store, &custom);

    let mut input = empty_update_input();
    input.name = Some("New Name".to_owned());

    let result = update_schedule(&store, "sched-1", input, test_now()).expect("should succeed");
    assert_eq!(result.name, "New Name");
    assert!(result.updated_at >= custom.updated_at);
    assert_eq!(result.windows.len(), 1);
    assert_eq!(result.windows[0].day_of_week, Weekday::Tue);
}

#[test]
fn update_schedule_window_replace() {
    let store = test_store();
    let custom = make_custom_schedule("sched-2");
    seed_schedule(&store, &custom);

    let mut input = empty_update_input();
    input.windows = Some(vec![
        make_window_input(Weekday::Fri, hm(17, 0), hm(19, 0)),
        make_window_input(Weekday::Sat, hm(10, 0), hm(14, 0)),
    ]);

    let result = update_schedule(&store, "sched-2", input, test_now()).expect("should succeed");
    assert_eq!(result.windows.len(), 2);
    assert_eq!(result.windows[0].day_of_week, Weekday::Fri);
    assert_eq!(result.windows[1].day_of_week, Weekday::Sat);
    assert_ne!(result.windows[0].id, custom.windows[0].id);

    // Verify the new windows are persisted, not just returned in-memory.
    let stored = get_schedule(&store, "sched-2").expect("read back");
    assert_eq!(stored.windows.len(), 2);
    assert_eq!(stored.windows[0].day_of_week, Weekday::Fri);
    assert_eq!(stored.windows[1].day_of_week, Weekday::Sat);
}

#[test]
fn update_schedule_patch_semantics_no_op() {
    let store = test_store();
    let custom = make_custom_schedule("sched-3");
    seed_schedule(&store, &custom);

    let input = empty_update_input();
    let result = update_schedule(&store, "sched-3", input, test_now()).expect("should succeed");

    assert_eq!(result.name, "Custom-sched-3"); // unchanged
    assert_eq!(result.windows.len(), 1); // unchanged
    assert!(result.updated_at >= custom.updated_at);
}

#[test]
fn update_schedule_not_found() {
    let store = test_store();
    let input = empty_update_input();
    let result = update_schedule(&store, "nonexistent", input, test_now());
    assert_not_found(&result, "Schedule", "nonexistent");
}

#[test]
fn update_default_schedule_name_change_rejected() {
    let store = test_store();
    let id = default_schedule_id(&store);
    let mut input = empty_update_input();
    input.name = Some("Renamed Default".to_owned());

    let result = update_schedule(&store, &id, input, test_now());
    assert_validation_contains(&result, "default");
}

#[test]
fn update_default_schedule_same_name_allowed() {
    let store = test_store();
    let id = default_schedule_id(&store);
    let default_name = store.get_schedule(&id).unwrap().unwrap().name;
    let mut input = empty_update_input();
    input.name = Some(default_name); // same name — should be OK

    let result = update_schedule(&store, &id, input, test_now());
    assert!(result.is_ok(), "setting same name should be allowed");
}

#[test]
fn update_default_schedule_windows_allowed() {
    let store = test_store();
    let id = default_schedule_id(&store);
    let mut input = empty_update_input();
    input.windows = Some(vec![make_window_input(Weekday::Fri, hm(17, 0), hm(20, 0))]);

    let result = update_schedule(&store, &id, input, test_now());
    assert!(
        result.is_ok(),
        "updating default schedule windows is allowed"
    );
    let updated = result.unwrap();
    assert_eq!(updated.windows.len(), 1);
    assert_eq!(updated.windows[0].day_of_week, Weekday::Fri);
}

#[test]
fn update_schedule_window_validation_on_update() {
    let store = test_store();
    let custom = make_custom_schedule("sched-val");
    seed_schedule(&store, &custom);

    let mut input = empty_update_input();
    input.windows = Some(vec![
        make_window_input(Weekday::Mon, hm(18, 0), hm(20, 0)),
        make_window_input(Weekday::Mon, hm(19, 0), hm(21, 0)), // overlap
    ]);

    let result = update_schedule(&store, "sched-val", input, test_now());
    assert_validation(&result);
}

#[test]
fn update_schedule_empty_windows_rejected() {
    let store = test_store();
    let custom = make_custom_schedule("sched-empty");
    seed_schedule(&store, &custom);

    let mut input = empty_update_input();
    input.windows = Some(vec![]);

    let result = update_schedule(&store, "sched-empty", input, test_now());
    assert_validation(&result);
}

#[test]
fn update_schedule_sets_last_mutation() {
    let store = test_store();
    let custom = make_custom_schedule("sched-mut");
    seed_schedule(&store, &custom);

    let config_before = store.get_config().unwrap();
    assert!(config_before.last_mutation.is_none());

    let input = empty_update_input();
    update_schedule(&store, "sched-mut", input, test_now()).unwrap();

    let config_after = store.get_config().unwrap();
    assert!(
        config_after.last_mutation.is_some(),
        "last_mutation should be set after update"
    );
}

#[test]
fn delete_schedule_happy_path() {
    let store = test_store();
    let custom = make_custom_schedule("sched-del");
    seed_schedule(&store, &custom);

    delete_schedule(&store, "sched-del", test_now()).expect("should succeed");

    assert!(store.get_schedule("sched-del").unwrap().is_none());
}

#[test]
fn delete_schedule_not_found() {
    let store = test_store();
    let result = delete_schedule(&store, "nonexistent", test_now());
    assert_not_found(&result, "Schedule", "nonexistent");
}

#[test]
fn delete_default_schedule_rejected() {
    let store = test_store();
    let id = default_schedule_id(&store);
    let result = delete_schedule(&store, &id, test_now());
    assert_validation_contains(&result, "default");
}

#[test_case(false ; "tasks reassigned to default")]
#[test_case(true ; "templates reassigned to default")]
#[allow(clippy::too_many_lines)] // Task and RecurringTemplate struct construction differs enough to require separate branches
fn delete_schedule_reassigns_entities_to_default(is_template: bool) {
    let store = test_store();
    let def_id = default_schedule_id(&store);
    let custom = make_custom_schedule("sched-reassign");
    seed_schedule(&store, &custom);

    if is_template {
        let tmpl1 = RecurringTemplate::test_default()
            .with_id("tmpl-1")
            .with_schedule("sched-reassign");
        let tmpl2 = RecurringTemplate::test_default()
            .with_id("tmpl-2")
            .with_schedule("sched-reassign");
        let tmpl_other = RecurringTemplate::test_default()
            .with_id("tmpl-other")
            .with_schedule("other-schedule");
        seed_template(&store, &tmpl1);
        seed_template(&store, &tmpl2);
        seed_template(&store, &tmpl_other);

        delete_schedule(&store, "sched-reassign", test_now()).unwrap();

        assert_eq!(
            store.get_template("tmpl-1").unwrap().unwrap().schedule_id,
            def_id
        );
        assert_eq!(
            store.get_template("tmpl-2").unwrap().unwrap().schedule_id,
            def_id
        );
        assert_eq!(
            store
                .get_template("tmpl-other")
                .unwrap()
                .unwrap()
                .schedule_id,
            "other-schedule"
        );
    } else {
        let task_a = Task::test_default()
            .with_id("task-1")
            .with_schedule("sched-reassign");
        let task_b = Task::test_default()
            .with_id("task-2")
            .with_schedule("sched-reassign");
        let task_other = Task::test_default()
            .with_id("task-other")
            .with_schedule("other-schedule");
        seed_task(&store, &task_a);
        seed_task(&store, &task_b);
        seed_task(&store, &task_other);

        delete_schedule(&store, "sched-reassign", test_now()).unwrap();

        assert_eq!(
            store.get_task("task-1").unwrap().unwrap().schedule_id,
            def_id
        );
        assert_eq!(
            store.get_task("task-2").unwrap().unwrap().schedule_id,
            def_id
        );
        assert_eq!(
            store.get_task("task-other").unwrap().unwrap().schedule_id,
            "other-schedule"
        );
    }
}

#[test]
fn delete_schedule_sets_last_mutation() {
    let store = test_store();
    let custom = make_custom_schedule("sched-mut-del");
    seed_schedule(&store, &custom);

    let config_before = store.get_config().unwrap();
    assert!(config_before.last_mutation.is_none());

    delete_schedule(&store, "sched-mut-del", test_now()).unwrap();

    let config_after = store.get_config().unwrap();
    assert!(
        config_after.last_mutation.is_some(),
        "last_mutation should be set after delete"
    );
}

#[test]
fn delete_schedule_with_no_tasks_or_templates() {
    let store = test_store();
    let custom = make_custom_schedule("sched-empty-del");
    seed_schedule(&store, &custom);

    delete_schedule(&store, "sched-empty-del", test_now())
        .expect("should succeed with no assigned items");

    assert!(store.get_schedule("sched-empty-del").unwrap().is_none());
}

#[test_case(false ; "tasks get updated timestamp")]
#[test_case(true ; "templates get updated timestamp")]
fn delete_schedule_reassigned_entities_have_updated_timestamp(is_template: bool) {
    let store = test_store();
    if is_template {
        let custom = make_custom_schedule("sched-ts2");
        seed_schedule(&store, &custom);

        let tmpl = RecurringTemplate::test_default()
            .with_id("tmpl-ts")
            .with_schedule("sched-ts2");
        let original_updated = tmpl.updated_at;
        seed_template(&store, &tmpl);

        delete_schedule(&store, "sched-ts2", test_now()).unwrap();

        let reassigned = store.get_template("tmpl-ts").unwrap().unwrap();
        assert!(
            reassigned.updated_at >= original_updated,
            "reassigned template should have updated timestamp"
        );
    } else {
        let custom = make_custom_schedule("sched-ts");
        seed_schedule(&store, &custom);

        let task = Task::test_default()
            .with_id("task-ts")
            .with_schedule("sched-ts");
        let original_updated = task.updated_at;
        seed_task(&store, &task);

        delete_schedule(&store, "sched-ts", test_now()).unwrap();

        let reassigned = store.get_task("task-ts").unwrap().unwrap();
        assert!(
            reassigned.updated_at >= original_updated,
            "reassigned task should have updated timestamp"
        );
    }
}

// (min_chunk, duration, no_split, is_template, shrink_end)
// make_custom_schedule has Tue 19:00–21:00 = 120-min window.
#[test_case(90, 180, false, false, hm(20, 20) ; "splittable task min_chunk fails after shrink to 80 min")]
#[test_case(30, 60,  true,  false, hm(19, 50) ; "no-split task duration fails after shrink to 50 min")]
#[test_case(0,  90,  false, true,  hm(20, 20) ; "active template duration fails after shrink to 80 min")]
fn update_schedule_shrink_rejects_oversized_entity(
    min_chunk: i64,
    duration: i64,
    no_split: bool,
    is_template: bool,
    shrink_end: NaiveTime,
) {
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-shrink-param"));
    if is_template {
        let tmpl = RecurringTemplate {
            id: "entity-sh-param".to_owned(),
            duration_minutes: duration,
            schedule_id: "sched-shrink-param".to_owned(),
            is_active: true,
            ..RecurringTemplate::test_default()
        };
        store.create_template(&tmpl).expect("seed template");
    } else {
        let task = Task {
            id: "entity-sh-param".to_owned(),
            duration_minutes: duration,
            min_chunk_minutes: min_chunk,
            no_split,
            schedule_id: "sched-shrink-param".to_owned(),
            ..Task::test_default()
        };
        store.create_task(&task).expect("seed task");
    }
    let result = update_schedule(
        &store,
        "sched-shrink-param",
        single_tue_window(shrink_end),
        test_now(),
    );
    assert_validation(&result);
}

#[test]
fn update_schedule_shrink_succeeds_when_only_terminal_task_is_oversized() {
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-terminal"));

    // Completed task with min_chunk that would fail splittable check
    let task = Task {
        id: "task-done".to_owned(),
        status: TaskStatus::Completed,
        duration_minutes: 180,
        min_chunk_minutes: 90,
        no_split: false,
        schedule_id: "sched-terminal".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed completed task");

    // Shrink to 80-min window: would fail for a non-terminal task, but not here
    let result = update_schedule(
        &store,
        "sched-terminal",
        single_tue_window(hm(20, 20)),
        test_now(),
    );
    assert!(
        result.is_ok(),
        "terminal tasks must not block schedule shrinking: {result:?}"
    );
}

#[test]
fn update_schedule_shrink_succeeds_when_only_inactive_template_is_oversized() {
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-inactive-tmpl"));

    // Inactive template: duration exceeds new window, but should not block edit
    let tmpl = RecurringTemplate {
        id: "tmpl-inactive".to_owned(),
        duration_minutes: 90,
        schedule_id: "sched-inactive-tmpl".to_owned(),
        is_active: false,
        ..RecurringTemplate::test_default()
    };
    store
        .create_template(&tmpl)
        .expect("seed inactive template");

    // Shrink to 80-min window
    let result = update_schedule(
        &store,
        "sched-inactive-tmpl",
        single_tue_window(hm(20, 20)),
        test_now(),
    );
    assert!(
        result.is_ok(),
        "inactive templates must not block schedule shrinking: {result:?}"
    );
}

#[test]
fn update_schedule_rename_only_skips_capacity_check() {
    let store = test_store();
    // Schedule with only a window-less test default
    let custom = make_custom_schedule("sched-rename");
    seed_schedule(&store, &custom);

    // Seed a task that would fail capacity if windows were checked
    let task = Task {
        id: "task-rename".to_owned(),
        duration_minutes: 300,
        min_chunk_minutes: 200,
        no_split: false,
        schedule_id: "sched-rename".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    // Rename only (no windows change) → capacity check not triggered
    let mut input = empty_update_input();
    input.name = Some("Renamed Evening".to_owned());
    let result = update_schedule(&store, "sched-rename", input, test_now());
    assert!(
        result.is_ok(),
        "rename-only update must skip capacity check: {result:?}"
    );
}

#[test]
fn update_schedule_window_growth_always_succeeds() {
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-grow"));

    // No-split task that fits the original 120-min window
    let task = Task {
        id: "task-grow".to_owned(),
        duration_minutes: 60,
        no_split: true,
        schedule_id: "sched-grow".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    // Grow window to 180 min — must always succeed (nothing becomes unfit)
    let result = update_schedule(
        &store,
        "sched-grow",
        single_tue_window(hm(22, 0)),
        test_now(),
    );
    assert!(
        result.is_ok(),
        "window growth must always succeed: {result:?}"
    );
    assert_eq!(result.unwrap().windows[0].end_time, hm(22, 0));
}

#[test]
fn update_schedule_shrink_error_names_offending_task() {
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-msg"));

    let task = Task {
        id: "task-msg".to_owned(),
        title: "Read a book".to_owned(),
        duration_minutes: 60,
        min_chunk_minutes: 30,
        no_split: true,
        schedule_id: "sched-msg".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    let result = update_schedule(
        &store,
        "sched-msg",
        single_tue_window(hm(19, 50)),
        test_now(),
    );
    let msg = match result {
        Err(AppError::Validation(m)) => m,
        other => panic!("expected Validation error, got: {other:?}"),
    };
    assert!(
        msg.contains("Custom-sched-msg"),
        "schedule name missing: {msg}"
    );
    assert!(msg.contains("Read a book"), "task title missing: {msg}");
}

#[test]
fn update_schedule_shrink_error_names_offending_template() {
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-tmpl-msg"));

    let tmpl = RecurringTemplate {
        id: "tmpl-msg".to_owned(),
        title: "Weekly fitness".to_owned(),
        duration_minutes: 90,
        schedule_id: "sched-tmpl-msg".to_owned(),
        is_active: true,
        ..RecurringTemplate::test_default()
    };
    store.create_template(&tmpl).expect("seed template");

    let result = update_schedule(
        &store,
        "sched-tmpl-msg",
        single_tue_window(hm(20, 20)),
        test_now(),
    );
    let msg = match result {
        Err(AppError::Validation(m)) => m,
        other => panic!("expected Validation error, got: {other:?}"),
    };
    assert!(
        msg.contains("Custom-sched-tmpl-msg"),
        "schedule name missing: {msg}"
    );
    assert!(
        msg.contains("Weekly fitness"),
        "template title missing: {msg}"
    );
}

#[test]
fn update_schedule_shrink_auto_no_split_task_uses_duration_as_required_window() {
    // Covers the `||` short-circuit path in required_window_minutes where
    // no_split=false but duration ≤ min_chunk (auto-no-split); the required
    // window is duration (not min_chunk).
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-auto-ns"));

    // no_split=false, duration=20, min_chunk=30 → effective_no_split via
    // duration ≤ min_chunk (20 ≤ 30).  required_window_minutes returns
    // duration (20), not min_chunk (30).
    let task = Task {
        id: "task-auto-ns".to_owned(),
        duration_minutes: 20,
        min_chunk_minutes: 30,
        no_split: false,
        schedule_id: "sched-auto-ns".to_owned(),
        ..Task::test_default()
    };
    store.create_task(&task).expect("seed task");

    // Shrink to 15-min window: duration(20) > 15 → reject
    let result = update_schedule(
        &store,
        "sched-auto-ns",
        single_tue_window(hm(19, 15)),
        test_now(),
    );
    assert_validation(&result);
}

#[test]
fn update_schedule_window_check_ignores_templates_from_other_schedules() {
    // Covers the false branch of the schedule_id filter in the active-template
    // capacity check: a template on a different schedule must be ignored.
    let store = test_store();
    seed_schedule(&store, &make_custom_schedule("sched-filter"));
    seed_schedule(&store, &make_custom_schedule("other-sched"));

    // An active template on a DIFFERENT schedule with a large duration.
    let other_tmpl = RecurringTemplate {
        id: "tmpl-other-sched".to_owned(),
        duration_minutes: 180,
        schedule_id: "other-sched".to_owned(),
        is_active: true,
        ..RecurringTemplate::test_default()
    };
    store.create_template(&other_tmpl).expect("seed template");

    // Shrink "sched-filter" to 80 min.  The 180-min template is on a
    // different schedule and must NOT block this edit.
    let result = update_schedule(
        &store,
        "sched-filter",
        single_tue_window(hm(20, 20)),
        test_now(),
    );
    assert!(
        result.is_ok(),
        "template on different schedule must not block shrinking: {result:?}"
    );
}
