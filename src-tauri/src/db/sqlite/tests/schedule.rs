// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `ScheduleStore` implementation and weekday conversion helpers.

use chrono::{NaiveTime, Weekday};
use test_case::test_case;

use super::make_test_schedule;
use crate::db::sqlite::schedule::{weekday_from_i64, weekday_to_i64};
use crate::db::sqlite::SqliteStore;
use crate::domain::models::{Schedule, ScheduleWindow};
use crate::test_support::fixture_base;
use crate::traits::storage::ScheduleStore;

/// Fetch a schedule by id, unwrapping the `Result<Option<_>>` — the schedule
/// is assumed to exist (the test just created or updated it).
fn fetch(store: &SqliteStore, id: &str) -> Schedule {
    store
        .get_schedule(id)
        .expect("get_schedule")
        .expect("schedule should exist")
}

fn count_windows(store: &SqliteStore, schedule_id: &str) -> i64 {
    let conn = store.conn.lock().expect("lock");
    conn.query_row(
        "SELECT COUNT(*) FROM schedule_windows WHERE schedule_id = ?1",
        [schedule_id],
        |row| row.get(0),
    )
    .expect("count windows")
}

#[test]
fn create_and_get_schedule_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let schedule = make_test_schedule();

    store.create_schedule(&schedule).expect("create_schedule");
    let loaded = fetch(&store, &schedule.id);

    assert_eq!(loaded.id, schedule.id);
    assert_eq!(loaded.name, schedule.name);
    assert_eq!(loaded.is_default, schedule.is_default);
    assert_eq!(loaded.windows.len(), 2);

    let mut windows = loaded.windows;
    windows.sort_by_key(|w| weekday_to_i64(w.day_of_week));

    assert_eq!(windows[0].day_of_week, Weekday::Mon);
    assert_eq!(
        windows[0].start_time,
        NaiveTime::from_hms_opt(9, 0, 0).unwrap()
    );
    assert_eq!(
        windows[0].end_time,
        NaiveTime::from_hms_opt(17, 0, 0).unwrap()
    );
    assert_eq!(windows[0].schedule_id, schedule.id);

    assert_eq!(windows[1].day_of_week, Weekday::Wed);
    assert_eq!(
        windows[1].start_time,
        NaiveTime::from_hms_opt(18, 0, 0).unwrap()
    );
    assert_eq!(
        windows[1].end_time,
        NaiveTime::from_hms_opt(22, 0, 0).unwrap()
    );
}

#[test]
fn get_schedule_not_found() {
    let store = SqliteStore::new_in_memory();
    let result = store.get_schedule("nonexistent-id").expect("get_schedule");
    assert!(result.is_none());
}

#[test]
fn get_default_schedule_returns_seed() {
    let store = SqliteStore::new_in_memory();
    let default = store.get_default_schedule().expect("get_default_schedule");

    assert_eq!(default.name, "Default");
    assert!(default.is_default);
    // Seed schedule has 12 windows: 5 weekdays * 2 + 2 weekend * 1.
    assert_eq!(default.windows.len(), 12);
}

#[test]
fn get_default_schedule_not_found() {
    let store = SqliteStore::new_in_memory();

    let default = store.get_default_schedule().expect("get seed");
    store.delete_schedule(&default.id).expect("delete seed");

    let result = store.get_default_schedule();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, crate::error::AppError::NotFound { ref entity, ref id }
            if entity == "Schedule" && id == "default"),
        "expected NotFound for default schedule, got: {err:?}"
    );
}

#[test]
fn update_schedule_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let mut schedule = make_test_schedule();
    store.create_schedule(&schedule).expect("create_schedule");

    schedule.name = "Updated Schedule".to_owned();
    schedule.is_default = true;
    schedule.windows = vec![ScheduleWindow {
        id: "test-window-fri".to_owned(),
        schedule_id: schedule.id.clone(),
        day_of_week: Weekday::Fri,
        start_time: NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
    }];
    store.update_schedule(&schedule).expect("update_schedule");

    let loaded = fetch(&store, &schedule.id);
    assert_eq!(loaded.name, "Updated Schedule");
    assert!(loaded.is_default);
    assert_eq!(loaded.windows.len(), 1);
    assert_eq!(loaded.windows[0].day_of_week, Weekday::Fri);
    assert_eq!(
        loaded.windows[0].start_time,
        NaiveTime::from_hms_opt(19, 0, 0).unwrap()
    );
    assert_eq!(
        loaded.windows[0].end_time,
        NaiveTime::from_hms_opt(23, 0, 0).unwrap()
    );
}

#[test]
fn update_schedule_replaces_windows() {
    let store = SqliteStore::new_in_memory();
    let mut schedule = make_test_schedule();
    assert_eq!(schedule.windows.len(), 2);
    store.create_schedule(&schedule).expect("create_schedule");

    schedule.windows = vec![ScheduleWindow {
        id: "test-window-sat".to_owned(),
        schedule_id: schedule.id.clone(),
        day_of_week: Weekday::Sat,
        start_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(14, 0, 0).unwrap(),
    }];
    store.update_schedule(&schedule).expect("update_schedule");

    let loaded = fetch(&store, &schedule.id);
    assert_eq!(loaded.windows.len(), 1);
    assert_eq!(loaded.windows[0].day_of_week, Weekday::Sat);

    assert_eq!(count_windows(&store, &schedule.id), 1);
}

#[test]
fn delete_schedule_removes_schedule_and_windows() {
    let store = SqliteStore::new_in_memory();
    let schedule = make_test_schedule();
    store.create_schedule(&schedule).expect("create_schedule");

    store
        .delete_schedule(&schedule.id)
        .expect("delete_schedule");

    let loaded = store.get_schedule(&schedule.id).expect("get_schedule");
    assert!(loaded.is_none());

    assert_eq!(count_windows(&store, &schedule.id), 0);
}

#[test]
fn list_schedules_includes_all() {
    let store = SqliteStore::new_in_memory();
    let schedule = make_test_schedule();
    store.create_schedule(&schedule).expect("create_schedule");

    let all = store.list_schedules().expect("list_schedules");
    // Seed "Default" schedule + our test schedule.
    assert_eq!(all.len(), 2);

    let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Default"));
    assert!(names.contains(&"Test Schedule"));
}

#[test]
fn list_schedules_includes_windows() {
    let store = SqliteStore::new_in_memory();
    let schedule = make_test_schedule();
    store.create_schedule(&schedule).expect("create_schedule");

    let all = store.list_schedules().expect("list_schedules");

    for sched in &all {
        if sched.name == "Default" {
            assert_eq!(
                sched.windows.len(),
                12,
                "seed schedule should have 12 windows"
            );
        } else {
            assert_eq!(
                sched.windows.len(),
                2,
                "test schedule should have 2 windows"
            );
        }
    }
}

#[test_case(Weekday::Mon, 0 ; "Mon roundtrips to 0")]
#[test_case(Weekday::Tue, 1 ; "Tue roundtrips to 1")]
#[test_case(Weekday::Wed, 2 ; "Wed roundtrips to 2")]
#[test_case(Weekday::Thu, 3 ; "Thu roundtrips to 3")]
#[test_case(Weekday::Fri, 4 ; "Fri roundtrips to 4")]
#[test_case(Weekday::Sat, 5 ; "Sat roundtrips to 5")]
#[test_case(Weekday::Sun, 6 ; "Sun roundtrips to 6")]
fn weekday_helpers_roundtrip(weekday: Weekday, expected_i64: i64) {
    assert_eq!(weekday_to_i64(weekday), expected_i64);
    assert_eq!(weekday_from_i64(expected_i64).unwrap(), weekday);
}

#[test_case(7  ; "out of range high")]
#[test_case(-1 ; "out of range low")]
#[test_case(99 ; "far out of range")]
fn weekday_from_i64_rejects_unknown(input: i64) {
    assert!(weekday_from_i64(input).is_err());
}

#[test]
fn create_schedule_with_no_windows() {
    let store = SqliteStore::new_in_memory();
    let mut schedule = make_test_schedule();
    schedule.windows = Vec::new();

    store.create_schedule(&schedule).expect("create_schedule");
    let loaded = fetch(&store, &schedule.id);

    assert_eq!(loaded.id, schedule.id);
    assert!(loaded.windows.is_empty());
}

#[test_case("07:00" ; "morning time 07:00")]
#[test_case("23:59" ; "late night 23:59")]
#[test_case("00:00" ; "midnight 00:00")]
#[test_case("12:30" ; "midday 12:30")]
fn schedule_window_naive_time_roundtrip(time_str: &str) {
    let store = SqliteStore::new_in_memory();
    let schedule_id = "test-schedule-time".to_owned();
    let time = NaiveTime::parse_from_str(time_str, "%H:%M").unwrap();
    let now = fixture_base();

    let schedule = Schedule {
        id: schedule_id.clone(),
        name: format!("Time Test {time_str}"),
        is_default: false,
        windows: vec![ScheduleWindow {
            id: "test-window-time".to_owned(),
            schedule_id: schedule_id.clone(),
            day_of_week: Weekday::Mon,
            start_time: time,
            end_time: time,
        }],
        created_at: now,
        updated_at: now,
    };

    store.create_schedule(&schedule).expect("create_schedule");
    let loaded = store
        .get_schedule(&schedule_id)
        .expect("get_schedule")
        .expect("schedule should exist");

    assert_eq!(loaded.windows.len(), 1);
    assert_eq!(loaded.windows[0].start_time, time);
    assert_eq!(loaded.windows[0].end_time, time);
}
