// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `ScheduleStore` + `RecurringTemplateStore` +
//! `ConfigStore` cross-module interactions.
//!
//! These tests focus on FK constraints, cascades, and cross-store interactions
//! that are NOT already covered by the unit tests in `sqlite.rs`.

use chrono::{NaiveTime, TimeZone, Utc, Weekday};
use test_case::test_case;

use crate::db::sqlite::SqliteStore;
use crate::domain::cadence::{Cadence, Period, Window};
use crate::domain::enums::Priority;
use crate::domain::inputs::TaskFilter;
use crate::domain::models::{AppConfig, RecurringTemplate, Schedule, ScheduleWindow, Task};
use crate::test_support::test_now;
use crate::traits::storage::{ConfigStore, RecurringTemplateStore, ScheduleStore, TaskStore};

/// Create a non-default, window-less [`Schedule`] shell. Callers set
/// `windows` themselves, using `schedule.id` for each window's `schedule_id`.
fn schedule_shell(name: &str) -> Schedule {
    let now = test_now();
    Schedule {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_owned(),
        is_default: false,
        windows: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

fn make_schedule(name: &str) -> Schedule {
    let mut schedule = schedule_shell(name);
    schedule.windows = vec![ScheduleWindow {
        id: uuid::Uuid::now_v7().to_string(),
        schedule_id: schedule.id.clone(),
        day_of_week: Weekday::Mon,
        start_time: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
    }];
    schedule
}

fn make_schedule_multi_window(name: &str) -> Schedule {
    let mut schedule = schedule_shell(name);
    let schedule_id = schedule.id.clone();
    schedule.windows = vec![
        ScheduleWindow {
            id: uuid::Uuid::now_v7().to_string(),
            schedule_id: schedule_id.clone(),
            day_of_week: Weekday::Mon,
            start_time: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
        },
        ScheduleWindow {
            id: uuid::Uuid::now_v7().to_string(),
            schedule_id: schedule_id.clone(),
            day_of_week: Weekday::Wed,
            start_time: NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        },
        ScheduleWindow {
            id: uuid::Uuid::now_v7().to_string(),
            schedule_id,
            day_of_week: Weekday::Fri,
            start_time: NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
        },
    ];
    schedule
}

fn make_task_for_schedule(schedule_id: &str) -> Task {
    Task {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Integration test task".to_owned(),
        deadline: Some(Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap()),
        schedule_id: schedule_id.to_owned(),
        created_at: test_now(),
        updated_at: test_now(),
        ..Task::test_default()
    }
}

fn make_template_for_schedule(schedule_id: &str) -> RecurringTemplate {
    RecurringTemplate {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Integration test template".to_owned(),
        duration_minutes: 30,
        schedule_id: schedule_id.to_owned(),
        start_date: test_now(),
        created_at: test_now(),
        updated_at: test_now(),
        ..RecurringTemplate::test_default()
    }
}

/// Create a second template with `b_labels` on `schedule_id`, delete `template_a_id`,
/// assert `template_a_id` is gone, and return the persisted `template_b`.
fn seed_second_and_delete_first(
    store: &SqliteStore,
    schedule_id: &str,
    template_a_id: &str,
    b_labels: Vec<String>,
) -> RecurringTemplate {
    let mut template_b = make_template_for_schedule(schedule_id);
    template_b.labels = b_labels;
    store
        .create_template(&template_b)
        .expect("create template_b");
    store
        .delete_template(template_a_id)
        .expect("delete template_a");
    assert!(store.get_template(template_a_id).expect("get a").is_none());
    store
        .get_template(&template_b.id)
        .expect("get b")
        .expect("template_b should exist")
}

/// Create an in-memory store with a template seeded on the default schedule.
/// Returns `(store, default_schedule_id, template)`.
fn store_with_default_schedule_template() -> (SqliteStore, String, RecurringTemplate) {
    let store = SqliteStore::new_in_memory();
    let default = store.get_default_schedule().expect("get default schedule");
    let template = make_template_for_schedule(&default.id);
    store.create_template(&template).expect("create template");
    (store, default.id, template)
}

#[test]
fn delete_schedule_fails_when_task_references_it() {
    let store = SqliteStore::new_in_memory();

    let schedule = make_schedule("Weekend");
    store.create_schedule(&schedule).expect("create schedule");

    let task = make_task_for_schedule(&schedule.id);
    store.create_task(&task).expect("create task");

    // Attempting to delete the schedule should fail: tasks.schedule_id has no
    // ON DELETE CASCADE, so the FK constraint is violated.
    let result = store.delete_schedule(&schedule.id);
    assert!(
        result.is_err(),
        "delete_schedule with referencing task should fail"
    );

    // Task should still exist.
    let loaded = store.get_task(&task.id).expect("get task");
    assert!(loaded.is_some(), "task must survive the failed delete");
}

#[test]
fn create_task_with_nonexistent_schedule_id_fails() {
    let store = SqliteStore::new_in_memory();

    let task = make_task_for_schedule("nonexistent-schedule-id");
    let result = store.create_task(&task);
    assert!(
        result.is_err(),
        "creating a task with a bogus schedule_id should fail"
    );
}

#[test]
fn task_schedule_id_readable_after_schedule_operations() {
    let store = SqliteStore::new_in_memory();

    let schedule = make_schedule("Evening");
    store.create_schedule(&schedule).expect("create schedule");

    let task = make_task_for_schedule(&schedule.id);
    store.create_task(&task).expect("create task");

    // Update the schedule (name change, not id).
    let mut updated = schedule.clone();
    updated.name = "Evening Revised".to_owned();
    store.update_schedule(&updated).expect("update schedule");

    // Task's schedule_id must still point to the same schedule.
    let loaded_task = store
        .get_task(&task.id)
        .expect("get task")
        .expect("task should exist");
    assert_eq!(
        loaded_task.schedule_id, schedule.id,
        "schedule_id must remain intact after schedule update"
    );

    let loaded_schedule = store
        .get_schedule(&schedule.id)
        .expect("get schedule")
        .expect("schedule should exist");
    assert_eq!(loaded_schedule.name, "Evening Revised");
}

#[test]
fn delete_schedule_fails_when_template_references_it() {
    let store = SqliteStore::new_in_memory();

    let schedule = make_schedule("Work Hours");
    store.create_schedule(&schedule).expect("create schedule");

    let template = make_template_for_schedule(&schedule.id);
    store.create_template(&template).expect("create template");

    // recurring_templates.schedule_id has no ON DELETE CASCADE.
    let result = store.delete_schedule(&schedule.id);
    assert!(
        result.is_err(),
        "delete_schedule with referencing template should fail"
    );

    // Template should still exist.
    let loaded = store.get_template(&template.id).expect("get template");
    assert!(loaded.is_some(), "template must survive the failed delete");
}

#[test]
fn create_template_with_nonexistent_schedule_id_fails() {
    let store = SqliteStore::new_in_memory();

    let template = make_template_for_schedule("nonexistent-schedule-id");
    let result = store.create_template(&template);
    assert!(
        result.is_err(),
        "creating a template with a bogus schedule_id should fail"
    );
}

#[test]
fn delete_template_nulls_recurring_template_id_on_tasks() {
    let (store, default_id, template) = store_with_default_schedule_template();

    let mut task = make_task_for_schedule(&default_id);
    task.recurring_template_id = Some(template.id.clone());
    store.create_task(&task).expect("create task");

    // Deleting the template should trigger ON DELETE SET NULL on tasks.
    store
        .delete_template(&template.id)
        .expect("delete template");

    // Template is gone.
    assert!(
        store
            .get_template(&template.id)
            .expect("get template")
            .is_none(),
        "template should be deleted"
    );

    // Task must still exist and have recurring_template_id = NULL.
    let loaded = store
        .get_task(&task.id)
        .expect("get task")
        .expect("task should still exist");
    assert!(
        loaded.recurring_template_id.is_none(),
        "recurring_template_id must be NULL after template deletion"
    );
}

#[test]
fn delete_template_does_not_delete_referencing_tasks() {
    let (store, default_id, template) = store_with_default_schedule_template();

    // Create two tasks referencing the template.
    let mut task_a = make_task_for_schedule(&default_id);
    task_a.recurring_template_id = Some(template.id.clone());
    store.create_task(&task_a).expect("create task_a");

    let mut task_b = make_task_for_schedule(&default_id);
    task_b.recurring_template_id = Some(template.id.clone());
    store.create_task(&task_b).expect("create task_b");

    store
        .delete_template(&template.id)
        .expect("delete template");

    // Both tasks must survive.
    assert!(
        store.get_task(&task_a.id).expect("get a").is_some(),
        "task_a must survive template deletion"
    );
    assert!(
        store.get_task(&task_b.id).expect("get b").is_some(),
        "task_b must survive template deletion"
    );
}

#[test]
fn delete_template_cascades_its_labels_not_sibling_labels() {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get default schedule");

    let mut template_a = make_template_for_schedule(&default.id);
    template_a.labels = vec!["fitness".to_owned(), "evening".to_owned()];
    store
        .create_template(&template_a)
        .expect("create template_a");

    // Delete template_a — its labels cascade; template_b's labels survive.
    let loaded_b = seed_second_and_delete_first(
        &store,
        &default.id,
        &template_a.id,
        vec!["cooking".to_owned()],
    );
    assert_eq!(
        loaded_b.labels,
        vec!["cooking"],
        "template_b labels must survive template_a deletion"
    );
}

#[test]
fn delete_template_with_multiple_labels_removes_all() {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get default schedule");
    let mut template_a = make_template_for_schedule(&default.id);
    template_a.labels = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
    store
        .create_template(&template_a)
        .expect("create template_a");

    // Create a second template so we can verify label isolation via list_templates.
    let loaded_b = seed_second_and_delete_first(
        &store,
        &default.id,
        &template_a.id,
        vec!["survivor".to_owned()],
    );
    assert_eq!(
        loaded_b.labels,
        vec!["survivor"],
        "template_b labels must be intact after template_a deletion"
    );

    // template_a no longer appears in list_templates, confirming full removal.
    let all = store.list_templates().expect("list templates");
    assert!(
        all.iter().all(|t| t.id != template_a.id),
        "template_a must not appear in list_templates"
    );
}

#[test]
fn delete_schedule_cascades_its_windows_not_sibling_windows() {
    let store = SqliteStore::new_in_memory();

    let schedule_a = make_schedule_multi_window("Schedule A");
    store
        .create_schedule(&schedule_a)
        .expect("create schedule_a");

    let schedule_b = make_schedule("Schedule B");
    store
        .create_schedule(&schedule_b)
        .expect("create schedule_b");

    let loaded_a = store
        .get_schedule(&schedule_a.id)
        .expect("get a")
        .expect("a should exist");
    assert_eq!(loaded_a.windows.len(), 3);

    // Delete schedule_a — its windows cascade.
    store
        .delete_schedule(&schedule_a.id)
        .expect("delete schedule_a");

    assert!(
        store
            .get_schedule(&schedule_a.id)
            .expect("get a after delete")
            .is_none(),
        "schedule_a should be deleted"
    );

    let loaded_b = store
        .get_schedule(&schedule_b.id)
        .expect("get b")
        .expect("schedule_b should survive");
    assert_eq!(
        loaded_b.windows.len(),
        1,
        "schedule_b windows must survive schedule_a deletion"
    );
}

#[test]
fn deleted_schedule_windows_are_absent_from_db() {
    let store = SqliteStore::new_in_memory();

    let schedule = make_schedule_multi_window("Three Windows");
    store.create_schedule(&schedule).expect("create schedule");

    // Verify 3 windows exist before deletion.
    let before = store
        .get_schedule(&schedule.id)
        .expect("get before")
        .expect("schedule should exist before delete");
    assert_eq!(before.windows.len(), 3);

    store
        .delete_schedule(&schedule.id)
        .expect("delete schedule");

    let after = store.get_schedule(&schedule.id).expect("get after");
    assert!(
        after.is_none(),
        "schedule and its windows must be absent after deletion"
    );

    let all = store.list_schedules().expect("list schedules");
    assert!(
        all.iter().all(|s| s.id != schedule.id),
        "deleted schedule must not appear in list_schedules"
    );
}

#[test_case(
    Some(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()),
    None,
    None,
    None
    ; "reschedule only"
)]
#[test_case(
    None,
    Some(Utc.with_ymd_and_hms(2026, 3, 5, 12, 0, 0).unwrap()),
    None,
    None
    ; "mutation only"
)]
#[test_case(
    None,
    None,
    Some(Utc.with_ymd_and_hms(2026, 3, 10, 8, 30, 0).unwrap()),
    None
    ; "sync only"
)]
#[test_case(
    None,
    None,
    None,
    Some(Utc.with_ymd_and_hms(2026, 3, 15, 23, 59, 0).unwrap())
    ; "busy_sync only"
)]
#[test_case(
    Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()),
    Some(Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap()),
    Some(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()),
    Some(Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap())
    ; "all timestamps set"
)]
// test_case generates function signatures that pass chrono types by value.
#[allow(clippy::needless_pass_by_value)]
fn config_timestamp_combinations_roundtrip(
    last_reschedule: Option<chrono::DateTime<Utc>>,
    last_mutation: Option<chrono::DateTime<Utc>>,
    last_sync: Option<chrono::DateTime<Utc>>,
    last_busy_sync: Option<chrono::DateTime<Utc>>,
) {
    let store = SqliteStore::new_in_memory();

    let config = AppConfig {
        planning_horizon_days: 30,
        timezone: "UTC".to_owned(),
        max_continuous_minutes: 120,
        min_break_minutes: 5,
        last_reschedule,
        last_mutation,
        last_sync,
        last_busy_sync,
    };

    store.update_config(&config).expect("update_config");
    let loaded = store.get_config().expect("get_config");

    assert_eq!(loaded.last_reschedule, last_reschedule);
    assert_eq!(loaded.last_mutation, last_mutation);
    assert_eq!(loaded.last_sync, last_sync);
    assert_eq!(loaded.last_busy_sync, last_busy_sync);
}

#[test]
fn config_update_multiple_times_only_last_value_survives() {
    let store = SqliteStore::new_in_memory();

    let ts1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();

    let config_v1 = AppConfig {
        planning_horizon_days: 14,
        timezone: "America/New_York".to_owned(),
        max_continuous_minutes: 60,
        min_break_minutes: 15,
        last_reschedule: Some(ts1),
        last_mutation: None,
        last_sync: None,
        last_busy_sync: None,
    };
    store.update_config(&config_v1).expect("update v1");

    let config_v2 = AppConfig {
        planning_horizon_days: 90,
        timezone: "Europe/London".to_owned(),
        max_continuous_minutes: 180,
        min_break_minutes: 10,
        last_reschedule: None,
        last_mutation: Some(ts2),
        last_sync: Some(ts2),
        last_busy_sync: Some(ts2),
    };
    store.update_config(&config_v2).expect("update v2");

    let loaded = store.get_config().expect("get_config");
    assert_eq!(loaded.planning_horizon_days, 90);
    assert_eq!(loaded.timezone, "Europe/London");
    assert_eq!(loaded.max_continuous_minutes, 180);
    assert_eq!(loaded.min_break_minutes, 10);
    assert!(
        loaded.last_reschedule.is_none(),
        "v1 reschedule overwritten"
    );
    assert_eq!(loaded.last_mutation, Some(ts2));
    assert_eq!(loaded.last_sync, Some(ts2));
    assert_eq!(loaded.last_busy_sync, Some(ts2));
}

#[test]
fn config_update_does_not_affect_tasks_or_schedules() {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get default schedule");
    let task = make_task_for_schedule(&default.id);
    store.create_task(&task).expect("create task");

    let config = AppConfig {
        planning_horizon_days: 60,
        timezone: "Asia/Tokyo".to_owned(),
        max_continuous_minutes: 90,
        min_break_minutes: 5,
        last_reschedule: None,
        last_mutation: None,
        last_sync: None,
        last_busy_sync: None,
    };
    store.update_config(&config).expect("update_config");

    assert!(
        store.get_task(&task.id).expect("get task").is_some(),
        "task must survive config update"
    );
    let schedules = store.list_schedules().expect("list schedules");
    assert!(
        schedules.iter().any(|s| s.id == default.id),
        "default schedule must survive config update"
    );
}

#[test]
fn list_schedules_returns_all_with_correct_window_counts() {
    let store = SqliteStore::new_in_memory();

    let sched_1 = make_schedule("Alpha");
    let sched_2 = make_schedule_multi_window("Beta");
    store.create_schedule(&sched_1).expect("create Alpha");
    store.create_schedule(&sched_2).expect("create Beta");

    let all = store.list_schedules().expect("list schedules");
    // Seed default + Alpha + Beta = 3.
    assert_eq!(all.len(), 3);

    for s in &all {
        match s.name.as_str() {
            "Default" => assert_eq!(s.windows.len(), 12),
            "Alpha" => assert_eq!(s.windows.len(), 1),
            "Beta" => assert_eq!(s.windows.len(), 3),
            other => panic!("unexpected schedule name: {other}"),
        }
    }
}

#[test]
fn tasks_on_different_schedules_are_isolated() {
    let store = SqliteStore::new_in_memory();

    let sched_a = make_schedule("Sched A");
    store.create_schedule(&sched_a).expect("create sched_a");
    let sched_b = make_schedule("Sched B");
    store.create_schedule(&sched_b).expect("create sched_b");

    let task_a = make_task_for_schedule(&sched_a.id);
    store.create_task(&task_a).expect("create task_a");
    let task_b = make_task_for_schedule(&sched_b.id);
    store.create_task(&task_b).expect("create task_b");

    let filter_a = TaskFilter {
        schedule_id: Some(sched_a.id.clone()),
        ..TaskFilter::default()
    };
    let found_a = store.list_tasks(&filter_a).expect("list tasks a");
    assert_eq!(found_a.len(), 1);
    assert_eq!(found_a[0].id, task_a.id);

    let filter_b = TaskFilter {
        schedule_id: Some(sched_b.id.clone()),
        ..TaskFilter::default()
    };
    let found_b = store.list_tasks(&filter_b).expect("list tasks b");
    assert_eq!(found_b.len(), 1);
    assert_eq!(found_b[0].id, task_b.id);
}

#[test]
fn only_one_default_schedule_get_default_returns_seeded() {
    let store = SqliteStore::new_in_memory();

    // Create two non-default schedules.
    let extra_1 = make_schedule("Extra 1");
    store.create_schedule(&extra_1).expect("create extra_1");
    let extra_2 = make_schedule("Extra 2");
    store.create_schedule(&extra_2).expect("create extra_2");

    let default = store.get_default_schedule().expect("get default");
    assert_eq!(default.name, "Default");
    assert!(default.is_default);
}

#[test_case(Cadence::weekly(vec![Weekday::Mon, Weekday::Wed, Weekday::Fri]) ; "weekly three days")]
#[test_case(Cadence::weekly_every(2, vec![Weekday::Sat, Weekday::Sun]) ; "biweekly weekend")]
#[test_case(Cadence::monthly(1) ; "monthly first")]
#[test_case(Cadence::monthly_every(3, 28) ; "quarterly 28th")]
#[test_case(Cadence::weekly(vec![Weekday::Tue]) ; "weekly single day")]
// test_case generates callers that pass Cadence by value.
#[allow(clippy::needless_pass_by_value)]
fn cadence_roundtrips_via_create_and_get_template(cadence: Cadence) {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get default schedule");
    let template = RecurringTemplate {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Cadence Test".to_owned(),
        duration_minutes: 30,
        schedule_id: default.id.clone(),
        cadence: cadence.clone(),
        ..RecurringTemplate::test_default()
    };

    store.create_template(&template).expect("create");

    let loaded = store
        .get_template(&template.id)
        .expect("get")
        .expect("template should exist");
    assert_eq!(
        loaded.cadence, cadence,
        "cadence must roundtrip via create/get"
    );
}

#[test]
fn cadence_survives_update_template() {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get default schedule");

    let weekly = Cadence::weekly(vec![Weekday::Mon, Weekday::Wed]);
    let mut template = RecurringTemplate {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Cadence Update Test".to_owned(),
        duration_minutes: 30,
        schedule_id: default.id.clone(),
        cadence: weekly.clone(),
        ..RecurringTemplate::test_default()
    };
    store.create_template(&template).expect("create");

    let monthly = Cadence::monthly(15);
    template.cadence = monthly.clone();
    store.update_template(&template).expect("update");

    let loaded = store
        .get_template(&template.id)
        .expect("get")
        .expect("should exist");
    assert_eq!(
        loaded.cadence, monthly,
        "cadence must reflect updated monthly value"
    );
    assert_ne!(loaded.cadence, weekly, "old weekly cadence must be gone");
}

#[test]
fn weekly_cadence_multiple_days_all_days_preserved() {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get default schedule");
    let all_weekdays = vec![
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let cadence = Cadence::weekly(all_weekdays);

    let template = RecurringTemplate {
        id: uuid::Uuid::now_v7().to_string(),
        title: "All Days".to_owned(),
        priority: Priority::Low,
        schedule_id: default.id.clone(),
        cadence,
        ..RecurringTemplate::test_default()
    };
    store.create_template(&template).expect("create");

    let loaded = store
        .get_template(&template.id)
        .expect("get")
        .expect("should exist");
    assert_eq!(loaded.cadence.period(), Period::Weekly);
    assert_eq!(
        loaded.cadence.windows(),
        &[
            Window { start: 0, end: 0 },
            Window { start: 1, end: 1 },
            Window { start: 2, end: 2 },
            Window { start: 3, end: 3 },
            Window { start: 4, end: 4 },
            Window { start: 5, end: 5 },
            Window { start: 6, end: 6 },
        ],
        "all 7 weekday singleton windows must roundtrip"
    );
}
