// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for `TaskStore` + `ChunkStore` cross-module interactions.
//!
//! These tests focus on scenarios that span both stores (e.g. cascade deletes,
//! interleaved data across tasks, complex filter combinations) rather than
//! duplicating basic CRUD already covered in `sqlite.rs`.

use chrono::{DateTime, TimeZone, Utc};
use test_case::test_case;

use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{ChunkStatus, Priority, TaskStatus};
use crate::domain::inputs::TaskFilter;
use crate::domain::models::{Chunk, RecurringTemplate, Task};
use crate::test_support::test_now;
use crate::traits::storage::{ChunkStore, RecurringTemplateStore, ScheduleStore, TaskStore};

/// Create a [`Task`] with sensible defaults using the store's default schedule.
fn make_task(store: &SqliteStore) -> Task {
    let schedule = store.get_default_schedule().expect("get default schedule");
    Task {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Integration test task".to_owned(),
        description: Some("Integration test description".to_owned()),
        deadline: Some(Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap()),
        schedule_id: schedule.id,
        created_at: test_now(),
        updated_at: test_now(),
        ..Task::test_default()
    }
}

/// Create a [`Task`] with `deadline` set, persisted to the store.
fn task_with_deadline(store: &SqliteStore, deadline: DateTime<Utc>) -> Task {
    let mut task = make_task(store);
    task.deadline = Some(deadline);
    store.create_task(&task).expect("create task");
    task
}

/// Create a [`Chunk`] for the given task at the specified hour offsets on 2026-03-15.
fn make_chunk(task_id: &str, start_hour: u32, end_hour: u32) -> Chunk {
    let now = test_now();
    Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task_id.to_owned(),
        start_time: Utc.with_ymd_and_hms(2026, 3, 15, start_hour, 0, 0).unwrap(),
        end_time: Utc.with_ymd_and_hms(2026, 3, 15, end_hour, 0, 0).unwrap(),
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

/// Create a [`RecurringTemplate`] using the store's trait method (proper FK).
fn make_template(store: &SqliteStore) -> RecurringTemplate {
    let schedule = store.get_default_schedule().expect("get default schedule");
    RecurringTemplate {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Template".to_owned(),
        duration_minutes: 30,
        schedule_id: schedule.id,
        start_date: test_now(),
        created_at: test_now(),
        updated_at: test_now(),
        ..RecurringTemplate::test_default()
    }
}

fn seed_task(store: &SqliteStore) -> Task {
    let task = make_task(store);
    store.create_task(&task).expect("create task");
    task
}

/// Assert that `filter` selects exactly `expected` and no other task.
fn assert_filter_selects_one(store: &SqliteStore, filter: &TaskFilter, expected: &Task) {
    let found = store.list_tasks(filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, expected.id);
}

#[test]
fn task_with_multiple_chunks_returns_all_via_get_chunks_for_task() {
    let store = SqliteStore::new_in_memory();
    let task = seed_task(&store);

    let c1 = make_chunk(&task.id, 10, 11);
    let c2 = make_chunk(&task.id, 12, 13);
    let c3 = make_chunk(&task.id, 14, 15);
    store.create_chunk(&c1).expect("create c1");
    store.create_chunk(&c2).expect("create c2");
    store.create_chunk(&c3).expect("create c3");

    let chunks = store.get_chunks_for_task(&task.id).expect("get chunks");
    assert_eq!(chunks.len(), 3);

    let ids: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&c1.id.as_str()));
    assert!(ids.contains(&c2.id.as_str()));
    assert!(ids.contains(&c3.id.as_str()));
}

#[test]
fn multiple_tasks_chunks_are_correctly_isolated() {
    let store = SqliteStore::new_in_memory();

    let task_a = seed_task(&store);
    let task_b = seed_task(&store);

    let ca1 = make_chunk(&task_a.id, 10, 11);
    let ca2 = make_chunk(&task_a.id, 12, 13);
    let cb1 = make_chunk(&task_b.id, 14, 15);
    store.create_chunk(&ca1).expect("create ca1");
    store.create_chunk(&ca2).expect("create ca2");
    store.create_chunk(&cb1).expect("create cb1");

    let chunks_a = store.get_chunks_for_task(&task_a.id).expect("chunks a");
    assert_eq!(chunks_a.len(), 2);
    assert!(chunks_a.iter().all(|c| c.task_id == task_a.id));

    let chunks_b = store.get_chunks_for_task(&task_b.id).expect("chunks b");
    assert_eq!(chunks_b.len(), 1);
    assert_eq!(chunks_b[0].task_id, task_b.id);
}

#[test]
fn delete_task_cascades_to_all_chunks_but_not_sibling_tasks() {
    let store = SqliteStore::new_in_memory();

    let task_a = seed_task(&store);
    let task_b = seed_task(&store);

    let c1 = make_chunk(&task_a.id, 10, 11);
    let c2 = make_chunk(&task_a.id, 12, 13);
    let c3 = make_chunk(&task_a.id, 14, 15);
    store.create_chunk(&c1).expect("create c1");
    store.create_chunk(&c2).expect("create c2");
    store.create_chunk(&c3).expect("create c3");

    let sibling_chunk = make_chunk(&task_b.id, 18, 19);
    store
        .create_chunk(&sibling_chunk)
        .expect("create sibling_chunk");

    store.delete_task(&task_a.id).expect("delete task_a");

    // All of task_a's chunks should be gone via CASCADE.
    assert!(store.get_chunk(&c1.id).expect("get c1").is_none());
    assert!(store.get_chunk(&c2.id).expect("get c2").is_none());
    assert!(store.get_chunk(&c3.id).expect("get c3").is_none());
    assert!(store
        .get_chunks_for_task(&task_a.id)
        .expect("list")
        .is_empty());

    // Sibling task's chunk must survive.
    assert!(store
        .get_chunk(&sibling_chunk.id)
        .expect("get sibling")
        .is_some());
}

#[test]
fn get_auto_chunks_and_fixed_completed_across_multiple_tasks() {
    let store = SqliteStore::new_in_memory();

    let task_a = seed_task(&store);
    let task_b = seed_task(&store);

    let mut ca_auto = make_chunk(&task_a.id, 10, 11);
    ca_auto.is_fixed = false;
    ca_auto.status = ChunkStatus::Scheduled;
    store.create_chunk(&ca_auto).expect("create ca_auto");

    let mut ca_fixed = make_chunk(&task_a.id, 12, 13);
    ca_fixed.is_fixed = true;
    ca_fixed.status = ChunkStatus::Scheduled;
    store.create_chunk(&ca_fixed).expect("create ca_fixed");

    let mut cb_comp = make_chunk(&task_b.id, 14, 15);
    cb_comp.is_fixed = false;
    cb_comp.status = ChunkStatus::Completed;
    cb_comp.completed_at = Some(test_now());
    store.create_chunk(&cb_comp).expect("create cb_comp");

    let mut chunk_b_auto = make_chunk(&task_b.id, 16, 17);
    chunk_b_auto.is_fixed = false;
    chunk_b_auto.status = ChunkStatus::Scheduled;
    store
        .create_chunk(&chunk_b_auto)
        .expect("create chunk_b_auto");

    let auto = store.get_auto_chunks().expect("get_auto_chunks");
    assert_eq!(auto.len(), 2);
    let auto_ids: Vec<&str> = auto.iter().map(|c| c.id.as_str()).collect();
    assert!(auto_ids.contains(&ca_auto.id.as_str()));
    assert!(auto_ids.contains(&chunk_b_auto.id.as_str()));

    let fixed_comp = store.get_all_fixed_and_completed().expect("get_fixed_comp");
    assert_eq!(fixed_comp.len(), 2);
    let fc_ids: Vec<&str> = fixed_comp.iter().map(|c| c.id.as_str()).collect();
    assert!(fc_ids.contains(&ca_fixed.id.as_str()));
    assert!(fc_ids.contains(&cb_comp.id.as_str()));
}

#[test]
fn filter_multiple_criteria_simultaneously() {
    let store = SqliteStore::new_in_memory();

    // Task matching all criteria
    let mut matching = make_task(&store);
    matching.status = TaskStatus::Pending;
    matching.priority = Priority::High;
    matching.title = "Fix the login page".to_owned();
    matching.labels = vec!["urgent".to_owned(), "frontend".to_owned()];
    store.create_task(&matching).expect("create matching");

    // Same labels but wrong status
    let mut wrong_status = make_task(&store);
    wrong_status.status = TaskStatus::Completed;
    wrong_status.priority = Priority::High;
    wrong_status.title = "Fix the settings page".to_owned();
    wrong_status.labels = vec!["urgent".to_owned()];
    store
        .create_task(&wrong_status)
        .expect("create wrong_status");

    // Same status/priority but no matching label
    let mut wrong_label = make_task(&store);
    wrong_label.status = TaskStatus::Pending;
    wrong_label.priority = Priority::High;
    wrong_label.title = "Fix the backend".to_owned();
    wrong_label.labels = vec!["backend".to_owned()];
    store.create_task(&wrong_label).expect("create wrong_label");

    // Same everything except priority
    let mut wrong_priority = make_task(&store);
    wrong_priority.status = TaskStatus::Pending;
    wrong_priority.priority = Priority::Low;
    wrong_priority.title = "Fix the footer login".to_owned();
    wrong_priority.labels = vec!["urgent".to_owned()];
    store
        .create_task(&wrong_priority)
        .expect("create wrong_priority");

    let filter = TaskFilter {
        statuses: Some(vec![TaskStatus::Pending]),
        labels: Some(vec!["urgent".to_owned()]),
        priorities: Some(vec![Priority::High]),
        search_text: Some("login".to_owned()),
        ..TaskFilter::default()
    };
    assert_filter_selects_one(&store, &filter, &matching);
}

#[test]
fn filter_deadline_range_combined() {
    let store = SqliteStore::new_in_memory();

    let _early = task_with_deadline(&store, Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap());
    let mid = task_with_deadline(&store, Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap());
    let _late = task_with_deadline(&store, Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0).unwrap());

    // Range: after March 5, before March 25 => only mid
    let filter = TaskFilter {
        deadline_after: Some(Utc.with_ymd_and_hms(2026, 3, 5, 0, 0, 0).unwrap()),
        deadline_before: Some(Utc.with_ymd_and_hms(2026, 3, 25, 0, 0, 0).unwrap()),
        ..TaskFilter::default()
    };
    assert_filter_selects_one(&store, &filter, &mid);
}

#[test]
fn filter_schedule_id_plus_statuses() {
    let store = SqliteStore::new_in_memory();
    let schedule = store.get_default_schedule().expect("get default");

    let mut pending = make_task(&store);
    pending.status = TaskStatus::Pending;
    store.create_task(&pending).expect("create pending");

    let mut completed = make_task(&store);
    completed.status = TaskStatus::Completed;
    store.create_task(&completed).expect("create completed");

    let filter = TaskFilter {
        schedule_id: Some(schedule.id.clone()),
        statuses: Some(vec![TaskStatus::Pending]),
        ..TaskFilter::default()
    };
    assert_filter_selects_one(&store, &filter, &pending);
}

#[test]
fn filter_recurring_template_id_plus_labels() {
    let store = SqliteStore::new_in_memory();

    let template = make_template(&store);
    store.create_template(&template).expect("create template");

    let mut with_label = make_task(&store);
    with_label.recurring_template_id = Some(template.id.clone());
    with_label.labels = vec!["exercise".to_owned()];
    store.create_task(&with_label).expect("create with_label");

    let mut without_label = make_task(&store);
    without_label.recurring_template_id = Some(template.id.clone());
    without_label.labels = vec!["cooking".to_owned()];
    store
        .create_task(&without_label)
        .expect("create without_label");

    let filter = TaskFilter {
        recurring_template_id: Some(template.id.clone()),
        labels: Some(vec!["exercise".to_owned()]),
        ..TaskFilter::default()
    };
    assert_filter_selects_one(&store, &filter, &with_label);
}

#[test]
fn filter_no_match_returns_empty() {
    let store = SqliteStore::new_in_memory();
    let task = make_task(&store);
    store.create_task(&task).expect("create task");

    let filter = TaskFilter {
        statuses: Some(vec![TaskStatus::Cancelled]),
        labels: Some(vec!["nonexistent-label".to_owned()]),
        priorities: Some(vec![Priority::Critical]),
        search_text: Some("zzz_no_match_zzz".to_owned()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert!(found.is_empty());
}

#[test]
fn search_text_percent_is_treated_as_like_wildcard() {
    let store = SqliteStore::new_in_memory();

    let mut task_a = make_task(&store);
    task_a.title = "Task ABC".to_owned();
    store.create_task(&task_a).expect("create a");

    let mut task_b = make_task(&store);
    task_b.title = "Task XYZ".to_owned();
    store.create_task(&task_b).expect("create b");

    // Searching for "%" — the pattern becomes "%%%", which is equivalent to "%"
    // in LIKE, matching everything. This verifies the current unescaped behavior.
    let filter = TaskFilter {
        search_text: Some("%".to_owned()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(
        found.len(),
        2,
        "% in search_text acts as LIKE wildcard, matches all"
    );
}

#[test]
fn search_text_underscore_is_treated_as_like_wildcard() {
    let store = SqliteStore::new_in_memory();

    let mut task_a = make_task(&store);
    task_a.title = "Task A".to_owned();
    task_a.description = None;
    store.create_task(&task_a).expect("create a");

    let mut task_b = make_task(&store);
    task_b.title = "Task BB".to_owned();
    task_b.description = None;
    store.create_task(&task_b).expect("create b");

    // Searching for "Task _" — the pattern becomes "%Task _%".
    // _ matches exactly one character, so "Task A" matches but "Task BB" doesn't
    // (BB is two chars after the space, _ only matches one, but the trailing %
    // makes it match anyway). Actually: "%Task _%":
    //   "Task A"  — "Task " + "A" (one char) matches _ + trailing %  => match
    //   "Task BB" — "Task " + "BB" (two chars) — _ matches "B", then trailing "B"
    //               matches trailing % => also matches
    // So both match because trailing % consumes the rest.
    let filter = TaskFilter {
        search_text: Some("Task _".to_owned()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    // Both match because the pattern "%Task _%" has trailing % which matches
    // any remaining characters after the single-char _ match.
    assert_eq!(
        found.len(),
        2,
        "_ in search_text acts as LIKE single-char wildcard"
    );
}

#[test_case(TaskStatus::Backlog, 60, 0, false ; "backlog excluded")]
#[test_case(TaskStatus::Completed, 60, 0, false ; "completed excluded")]
#[test_case(TaskStatus::Cancelled, 60, 0, false ; "cancelled excluded")]
#[test_case(TaskStatus::Pending, 60, 60, false ; "pending fully logged excluded")]
#[test_case(TaskStatus::Scheduled, 60, 60, false ; "scheduled fully logged excluded")]
#[test_case(TaskStatus::Pending, 60, 0, true ; "pending with remaining included")]
#[test_case(TaskStatus::Scheduled, 60, 30, true ; "scheduled partially logged included")]
#[test_case(TaskStatus::Pending, 60, 59, true ; "pending one minute remaining included")]
fn schedulable_tasks_by_status_and_logged(
    status: TaskStatus,
    duration: i64,
    logged: i64,
    should_include: bool,
) {
    let store = SqliteStore::new_in_memory();
    let mut task = make_task(&store);
    task.status = status;
    task.duration_minutes = duration;
    task.time_logged_minutes = logged;
    store.create_task(&task).expect("create task");

    let schedulable = store.get_schedulable_tasks().expect("get_schedulable");
    if should_include {
        assert_eq!(schedulable.len(), 1, "expected task to be schedulable");
        assert_eq!(schedulable[0].id, task.id);
    } else {
        assert!(
            schedulable.is_empty(),
            "expected task to NOT be schedulable"
        );
    }
}

#[test]
fn chunks_in_range_with_interleaved_tasks() {
    let store = SqliteStore::new_in_memory();

    let task_a = seed_task(&store);
    let task_b = seed_task(&store);

    let ca = make_chunk(&task_a.id, 10, 11);
    store.create_chunk(&ca).expect("create ca");

    let mut cb = make_chunk(&task_b.id, 10, 11);
    cb.start_time = Utc.with_ymd_and_hms(2026, 3, 15, 10, 30, 0).unwrap();
    cb.end_time = Utc.with_ymd_and_hms(2026, 3, 15, 11, 30, 0).unwrap();
    store.create_chunk(&cb).expect("create cb");

    let ca2 = make_chunk(&task_a.id, 14, 15);
    store.create_chunk(&ca2).expect("create ca2");

    // Query 10:00–12:00: should find ca and cb
    let range_10_12 = store
        .get_chunks_in_range(
            Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap(),
        )
        .expect("range 10-12");
    assert_eq!(range_10_12.len(), 2);

    // Query 13:00–16:00: should find only ca2
    let range_13_16 = store
        .get_chunks_in_range(
            Utc.with_ymd_and_hms(2026, 3, 15, 13, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 3, 15, 16, 0, 0).unwrap(),
        )
        .expect("range 13-16");
    assert_eq!(range_13_16.len(), 1);
    assert_eq!(range_13_16[0].id, ca2.id);
}

#[test]
fn chunks_in_range_boundary_half_open_interval() {
    let store = SqliteStore::new_in_memory();
    let task = seed_task(&store);

    let chunk = make_chunk(&task.id, 10, 11);
    store.create_chunk(&chunk).expect("create chunk");

    // Query exactly [11:00, 12:00) — chunk.end_time(11:00) > start(11:00) is false
    let adjacent_after = store
        .get_chunks_in_range(
            Utc.with_ymd_and_hms(2026, 3, 15, 11, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap(),
        )
        .expect("adjacent after");
    assert!(
        adjacent_after.is_empty(),
        "adjacent-after should not overlap"
    );

    // Query exactly [9:00, 10:00) — chunk.start_time(10:00) < end(10:00) is false
    let adjacent_before = store
        .get_chunks_in_range(
            Utc.with_ymd_and_hms(2026, 3, 15, 9, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap(),
        )
        .expect("adjacent before");
    assert!(
        adjacent_before.is_empty(),
        "adjacent-before should not overlap"
    );

    // Query [10:30, 10:45) — entirely within chunk
    let inside = store
        .get_chunks_in_range(
            Utc.with_ymd_and_hms(2026, 3, 15, 10, 30, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 3, 15, 10, 45, 0).unwrap(),
        )
        .expect("inside");
    assert_eq!(inside.len(), 1, "query inside chunk should overlap");
}

#[test]
fn chunks_in_range_empty_when_no_chunks() {
    let store = SqliteStore::new_in_memory();

    let result = store
        .get_chunks_in_range(
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap(),
        )
        .expect("empty range");
    assert!(result.is_empty());
}

#[test]
fn auto_and_fixed_completed_with_all_four_combinations() {
    let store = SqliteStore::new_in_memory();

    let task = seed_task(&store);

    let auto_sched = make_chunk(&task.id, 10, 11);
    store.create_chunk(&auto_sched).expect("create auto_sched");

    let mut fixed_sched = make_chunk(&task.id, 12, 13);
    fixed_sched.is_fixed = true;
    store
        .create_chunk(&fixed_sched)
        .expect("create fixed_sched");

    let mut nonfixed_comp = make_chunk(&task.id, 14, 15);
    nonfixed_comp.status = ChunkStatus::Completed;
    nonfixed_comp.completed_at = Some(test_now());
    nonfixed_comp.logged_minutes = Some(60);
    store
        .create_chunk(&nonfixed_comp)
        .expect("create nonfixed_comp");

    let mut fixed_comp = make_chunk(&task.id, 16, 17);
    fixed_comp.is_fixed = true;
    fixed_comp.status = ChunkStatus::Completed;
    fixed_comp.completed_at = Some(test_now());
    fixed_comp.logged_minutes = Some(60);
    store.create_chunk(&fixed_comp).expect("create fixed_comp");

    let auto = store.get_auto_chunks().expect("auto");
    assert_eq!(auto.len(), 1);
    assert_eq!(auto[0].id, auto_sched.id);

    let fc = store
        .get_all_fixed_and_completed()
        .expect("fixed_completed");
    assert_eq!(fc.len(), 3);
    let fc_ids: Vec<&str> = fc.iter().map(|c| c.id.as_str()).collect();
    assert!(fc_ids.contains(&fixed_sched.id.as_str()));
    assert!(fc_ids.contains(&nonfixed_comp.id.as_str()));
    assert!(fc_ids.contains(&fixed_comp.id.as_str()));
    assert!(!fc_ids.contains(&auto_sched.id.as_str()));
}

#[test]
fn auto_and_fixed_completed_empty_when_no_chunks() {
    let store = SqliteStore::new_in_memory();

    let auto = store.get_auto_chunks().expect("auto");
    assert!(auto.is_empty());

    let fc = store
        .get_all_fixed_and_completed()
        .expect("fixed_completed");
    assert!(fc.is_empty());
}

#[test]
fn auto_chunks_span_multiple_tasks_correctly() {
    let store = SqliteStore::new_in_memory();

    let task_a = seed_task(&store);
    let task_b = seed_task(&store);
    let task_c = seed_task(&store);

    // Task A: 1 auto chunk
    let ca = make_chunk(&task_a.id, 10, 11);
    store.create_chunk(&ca).expect("create ca");

    // Task B: 1 fixed chunk (not auto)
    let mut cb = make_chunk(&task_b.id, 12, 13);
    cb.is_fixed = true;
    store.create_chunk(&cb).expect("create cb");

    // Task C: 2 auto chunks
    let cc1 = make_chunk(&task_c.id, 14, 15);
    store.create_chunk(&cc1).expect("create cc1");
    let cc2 = make_chunk(&task_c.id, 16, 17);
    store.create_chunk(&cc2).expect("create cc2");

    let auto = store.get_auto_chunks().expect("auto");
    assert_eq!(auto.len(), 3);
    let auto_ids: Vec<&str> = auto.iter().map(|c| c.id.as_str()).collect();
    assert!(auto_ids.contains(&ca.id.as_str()));
    assert!(auto_ids.contains(&cc1.id.as_str()));
    assert!(auto_ids.contains(&cc2.id.as_str()));
    assert!(!auto_ids.contains(&cb.id.as_str()));
}
