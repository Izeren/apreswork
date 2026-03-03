// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Foreign-key-aware seeding helpers that write through the real store API.
//!
//! Each `seed_*` helper goes through the real write API and auto-creates any
//! missing foreign-key parents (a schedule for a task/template, a task for a
//! chunk) so tests stay terse without tripping referential integrity.

use chrono::{NaiveTime, Weekday};

use crate::db::sqlite::SqliteStore;
use crate::domain::models::{Chunk, RecurringTemplate, Schedule, ScheduleWindow, Task};
use crate::traits::storage::{ChunkStore, RecurringTemplateStore, ScheduleStore, TaskStore};

/// Ensure a schedule with `id` exists (insert a schedule with one adequate
/// window if absent).
///
/// The auto-created schedule gets a Monday 18:00–23:00 window (300 min),
/// which satisfies capacity validation for any typical test task
/// (`duration` ≤ 300 min, `min_chunk` ≤ 300 min).
fn ensure_schedule(store: &SqliteStore, id: &str) {
    if store.get_schedule(id).expect("get_schedule").is_none() {
        let schedule = Schedule::test_default()
            .with_id(id)
            .with_windows(vec![ScheduleWindow {
                id: format!("{id}-auto-win"),
                schedule_id: id.to_owned(),
                day_of_week: Weekday::Mon,
                start_time: NaiveTime::from_hms_opt(18, 0, 0).expect("valid time"),
                end_time: NaiveTime::from_hms_opt(23, 0, 0).expect("valid time"),
            }]);
        store
            .create_schedule(&schedule)
            .expect("create FK-parent schedule");
    }
}

/// Build (but do not insert) a schedule with a single Monday window of
/// `window_minutes` duration starting at 18:00, and a unique name derived
/// from `id` (avoids UNIQUE collisions when a test creates several schedules).
///
/// `window_minutes` must be 1..=300 so the window ends by 23:00 (same day).
pub(crate) fn schedule_with_window(id: &str, window_minutes: i64) -> Schedule {
    assert!(
        (1..=300).contains(&window_minutes),
        "window_minutes must be 1..=300, got {window_minutes}"
    );
    let start = NaiveTime::from_hms_opt(18, 0, 0).expect("valid time");
    let end_h = 18 + (window_minutes / 60);
    let end_m = window_minutes % 60;
    // Asserted precondition: end_h ≤ 23, end_m ≤ 59 — both fit in u32.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let end = NaiveTime::from_hms_opt(end_h as u32, end_m as u32, 0).expect("valid time");
    let mut sched = Schedule::test_default()
        .with_id(id)
        .with_windows(vec![ScheduleWindow {
            id: format!("{id}-win"),
            schedule_id: id.to_owned(),
            day_of_week: Weekday::Mon,
            start_time: start,
            end_time: end,
        }]);
    sched.name = format!("Sched-{id}");
    sched
}

fn ensure_template(store: &SqliteStore, id: &str) {
    if store.get_template(id).expect("get_template").is_none() {
        let template = RecurringTemplate::test_default().with_id(id);
        ensure_schedule(store, &template.schedule_id);
        store
            .create_template(&template)
            .expect("create FK-parent template");
    }
}

fn ensure_task(store: &SqliteStore, id: &str) {
    if store.get_task(id).expect("get_task").is_none() {
        let task = Task::test_default().with_id(id);
        ensure_schedule(store, &task.schedule_id);
        store.create_task(&task).expect("create FK-parent task");
    }
}

pub(crate) fn seed_schedule(store: &SqliteStore, schedule: &Schedule) {
    store.create_schedule(schedule).expect("seed schedule");
}

pub(crate) fn seed_template(store: &SqliteStore, template: &RecurringTemplate) {
    ensure_schedule(store, &template.schedule_id);
    store.create_template(template).expect("seed template");
}

pub(crate) fn seed_task(store: &SqliteStore, task: &Task) {
    ensure_schedule(store, &task.schedule_id);
    if let Some(template_id) = &task.recurring_template_id {
        ensure_template(store, template_id);
    }
    store.create_task(task).expect("seed task");
}

pub(crate) fn seed_chunk(store: &SqliteStore, chunk: &Chunk) {
    ensure_task(store, &chunk.task_id);
    store.create_chunk(chunk).expect("seed chunk");
}

#[cfg(test)]
mod tests {
    use super::{seed_chunk, seed_schedule, seed_task, seed_template};
    use crate::domain::models::{Chunk, RecurringTemplate, Schedule, Task};
    use crate::test_support::test_store;
    use crate::traits::storage::{ChunkStore, RecurringTemplateStore, ScheduleStore, TaskStore};

    #[test]
    fn seed_task_auto_creates_missing_schedule() {
        let store = test_store();
        let task = Task::test_default().with_id("t1");
        // "default-schedule-id" is not the migration default → must be created.
        seed_task(&store, &task);
        assert!(store.get_task("t1").expect("get").is_some());
        assert!(store
            .get_schedule(&task.schedule_id)
            .expect("get")
            .is_some());
    }

    #[test]
    fn seed_task_reuses_existing_schedule() {
        let store = test_store();
        seed_schedule(
            &store,
            &Schedule::test_default().with_id("default-schedule-id"),
        );
        // Schedule already present → ensure takes the no-op branch.
        seed_task(&store, &Task::test_default().with_id("t1"));
        assert_eq!(store.list_schedules().expect("list").len(), 2); // default + ours
    }

    #[test]
    fn seed_task_auto_creates_missing_template() {
        let store = test_store();
        let task = Task::test_default().with_id("t1").with_template("tmpl-x");
        seed_task(&store, &task);
        assert!(store.get_template("tmpl-x").expect("get").is_some());
    }

    #[test]
    fn seed_template_auto_creates_schedule() {
        let store = test_store();
        seed_template(&store, &RecurringTemplate::test_default().with_id("tmpl-1"));
        assert!(store.get_template("tmpl-1").expect("get").is_some());
    }

    #[test]
    fn seed_chunk_auto_creates_missing_task() {
        let store = test_store();
        let chunk = Chunk::test_default().with_id("c1").with_task("task-1");
        seed_chunk(&store, &chunk);
        assert!(store.get_chunk("c1").expect("get").is_some());
        assert!(store.get_task("task-1").expect("get").is_some());
    }

    #[test]
    fn seed_chunk_reuses_existing_task() {
        let store = test_store();
        seed_task(&store, &Task::test_default().with_id("task-1"));
        seed_chunk(
            &store,
            &Chunk::test_default().with_id("c1").with_task("task-1"),
        );
        assert!(store.get_chunk("c1").expect("get").is_some());
    }
}
