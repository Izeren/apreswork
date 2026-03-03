// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared fixtures and assertions for the `DefaultScheduler` integration tests.

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Timelike, Utc, Weekday};

use crate::db::sqlite::SqliteStore;
use crate::domain::enums::Priority;
use crate::domain::models::{Chunk, ExternalEventRecord, Schedule, ScheduleWindow, Task};
use crate::test_support::default_schedule_id;
use crate::traits::storage::{ExternalEventStore, ScheduleStore};

/// Replace the seeded default schedule with a clean 09:00–17:00 all-week layout to avoid interference with slot placement expectations in tests.
pub(super) fn seed_all_day_schedule(store: &SqliteStore) {
    store
        .delete_schedule(&default_schedule_id(store))
        .expect("drop seed default schedule");
    store
        .create_schedule(&make_all_day_schedule())
        .expect("create schedule");
}

/// Create a schedule with 09:00–17:00 windows for all weekdays and weekend days, coupled to `monday_now()` (2026-03-23 10:00 UTC): the Monday window opens at 09:00, ensuring the first available slot starts at `now` and keeps slot offsets predictable.
fn make_all_day_schedule() -> Schedule {
    let days = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let windows: Vec<ScheduleWindow> = days
        .iter()
        .map(|&day| ScheduleWindow {
            id: format!("win-{day:?}"),
            schedule_id: "sched-default".to_owned(),
            day_of_week: day,
            start_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        })
        .collect();
    Schedule::test_default()
        .with_id("sched-default")
        .with_default(true)
        .with_windows(windows)
}

pub(super) fn monday_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 23, 10, 0, 0).unwrap()
}

pub(super) fn make_schedulable_task(id: &str, priority: Priority, duration_minutes: i64) -> Task {
    let now = monday_now();
    Task {
        id: id.to_owned(),
        title: format!("Task {id}"),
        duration_minutes,
        priority,
        deadline: Some(now + Duration::days(14)),
        schedule_id: "sched-default".to_owned(),
        ..Task::test_default()
    }
}

pub(super) fn assert_chunks_valid_and_non_overlapping(chunks: &[Chunk]) {
    let mut sorted = chunks.to_vec();
    sorted.sort_unstable_by_key(|c| c.start_time);

    for window in sorted.windows(2) {
        let a = &window[0];
        let b = &window[1];
        assert!(
            a.end_time <= b.start_time,
            "chunks overlap: [{}, {}) vs [{}, {})",
            a.start_time,
            a.end_time,
            b.start_time,
            b.end_time
        );
    }

    for chunk in chunks {
        let start_hour = chunk.start_time.hour();
        let end_hour = chunk.end_time.hour();
        let end_min = chunk.end_time.minute();

        let start_ok = start_hour >= 9;
        let end_ok = end_hour < 17 || (end_hour == 17 && end_min == 0);

        assert!(
            start_ok,
            "chunk starts before 09:00 UTC: {} ({}-{})",
            chunk.id, chunk.start_time, chunk.end_time
        );
        assert!(
            end_ok,
            "chunk ends after 17:00 UTC: {} ({}-{})",
            chunk.id, chunk.start_time, chunk.end_time
        );
    }
}

pub(super) fn assert_total_chunk_duration(chunks: &[Chunk], task_id: &str, expected_minutes: i64) {
    let total: i64 = chunks
        .iter()
        .filter(|c| c.task_id == task_id)
        .map(|c| (c.end_time - c.start_time).num_minutes())
        .sum();
    assert_eq!(
        total, expected_minutes,
        "task {task_id}: expected {expected_minutes} min of chunks, got {total}"
    );
}

/// Seed a busy external event. The replace window is widened by one hour on each side so
/// the event is guaranteed to fall inside it rather than landing on the boundary.
pub(super) fn seed_busy_event(
    store: &SqliteStore,
    now: DateTime<Utc>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) {
    let event = ExternalEventRecord {
        id: "ev-busy".to_owned(),
        calendar_id: "cal-1".to_owned(),
        event_id: "gcal-busy".to_owned(),
        title: "Busy block".to_owned(),
        description: None,
        start_time: start,
        end_time: end,
        busy: true,
        declined: false,
        all_day: false,
        updated_at: now,
    };
    store
        .replace_external_events_in_window(
            "cal-1",
            start - Duration::hours(1),
            end + Duration::hours(1),
            &[event],
        )
        .expect("seed busy external event");
}

pub(super) fn assert_minute_aligned(placed: &[Chunk]) {
    assert!(!placed.is_empty(), "placed chunks must not be empty");
    for chunk in placed {
        for (label, t) in [("start", chunk.start_time), ("end", chunk.end_time)] {
            assert_eq!(
                (t.second(), t.nanosecond()),
                (0, 0),
                "chunk {label} must be minute-aligned, got {t}"
            );
        }
    }
}

pub(super) fn assert_no_overlap_with_busy(
    placed: &[Chunk],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) {
    for chunk in placed {
        let overlaps = chunk.start_time < end && chunk.end_time > start;
        assert!(
            !overlaps,
            "chunk [{}, {}) overlaps busy interval [{start}, {end})",
            chunk.start_time, chunk.end_time
        );
    }
}
