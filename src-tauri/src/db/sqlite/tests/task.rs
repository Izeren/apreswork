// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `TaskStore` and `LabelStore` implementations and task
//! conversion helpers.

use chrono::{DateTime, TimeZone, Utc};
use test_case::test_case;

use super::{create_labeled_task, make_test_task, make_test_template};
use crate::db::sqlite::task::{fetch_labels_for_tasks, status_from_str, status_to_str};
use crate::db::sqlite::{priority_from_i64, priority_to_i64, SqliteStore};
use crate::domain::enums::{Priority, TaskStatus};
use crate::domain::inputs::TaskFilter;
use crate::domain::models::Task;
use crate::test_support::fixture_base;
use crate::traits::storage::{LabelStore, RecurringTemplateStore, Store, TaskStore};

#[test]
fn create_and_get_task_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);

    store.create_task(&task).expect("create_task");
    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");

    assert_eq!(loaded.id, task.id);
    assert_eq!(loaded.title, task.title);
    assert_eq!(loaded.description, task.description);
    assert_eq!(loaded.duration_minutes, task.duration_minutes);
    assert_eq!(loaded.time_logged_minutes, task.time_logged_minutes);
    assert_eq!(loaded.priority, task.priority);
    assert_eq!(loaded.status, task.status);
    assert_eq!(loaded.deadline, task.deadline);
    assert_eq!(loaded.start_date, task.start_date);
    assert_eq!(loaded.schedule_id, task.schedule_id);
    assert_eq!(loaded.min_chunk_minutes, task.min_chunk_minutes);
    assert_eq!(loaded.no_split, task.no_split);
    assert_eq!(loaded.recurring_template_id, task.recurring_template_id);
    assert_eq!(loaded.expire_at, task.expire_at);
    assert!(loaded.labels.is_empty());
}

#[test]
fn create_task_with_labels() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.labels = vec!["urgent".to_owned(), "personal".to_owned()];

    store.create_task(&task).expect("create_task");
    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");

    let mut labels = loaded.labels;
    labels.sort();
    assert_eq!(labels, vec!["personal", "urgent"]);
}

#[test]
fn get_task_not_found() {
    let store = SqliteStore::new_in_memory();
    let result = store.get_task("nonexistent-id").expect("get_task");
    assert!(result.is_none());
}

#[test]
fn update_task_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    store.create_task(&task).expect("create_task");

    task.title = "Updated title".to_owned();
    task.description = Some("Updated description".to_owned());
    task.duration_minutes = 120;
    task.time_logged_minutes = 30;
    task.priority = Priority::High;
    task.status = TaskStatus::Scheduled;
    task.no_split = true;
    task.min_chunk_minutes = 15;
    task.expire_at = Some(Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap());
    store.update_task(&task).expect("update_task");

    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");
    assert_eq!(loaded.title, "Updated title");
    assert_eq!(loaded.description, Some("Updated description".to_owned()));
    assert_eq!(loaded.duration_minutes, 120);
    assert_eq!(loaded.time_logged_minutes, 30);
    assert_eq!(loaded.priority, Priority::High);
    assert_eq!(loaded.status, TaskStatus::Scheduled);
    assert!(loaded.no_split);
    assert_eq!(loaded.min_chunk_minutes, 15);
    assert_eq!(loaded.expire_at, task.expire_at);
}

#[test]
fn update_task_replaces_labels() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.labels = vec!["a".to_owned(), "b".to_owned()];
    store.create_task(&task).expect("create_task");

    task.labels = vec!["c".to_owned()];
    store.update_task(&task).expect("update_task");

    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");
    assert_eq!(loaded.labels, vec!["c"]);
}

#[test]
fn delete_task_removes_task_and_labels() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.labels = vec!["label1".to_owned()];
    store.create_task(&task).expect("create_task");

    store.delete_task(&task.id).expect("delete_task");

    let loaded = store.get_task(&task.id).expect("get_task");
    assert!(loaded.is_none());

    // Verify labels were cascade-deleted.
    let conn = store.conn.lock().expect("lock");
    let label_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_labels WHERE task_id = ?1",
            [&task.id],
            |row| row.get(0),
        )
        .expect("count labels");
    assert_eq!(label_count, 0);
}

#[test]
fn list_tasks_empty_filter() {
    let store = SqliteStore::new_in_memory();
    let first = make_test_task(&store);
    let second = make_test_task(&store);
    store.create_task(&first).expect("create first");
    store.create_task(&second).expect("create second");

    let filter = TaskFilter::default();
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 2);
}

#[test]
fn list_tasks_filter_by_status() {
    let store = SqliteStore::new_in_memory();

    let mut task_pending = make_test_task(&store);
    task_pending.status = TaskStatus::Pending;
    store.create_task(&task_pending).expect("create pending");

    let mut task_completed = make_test_task(&store);
    task_completed.status = TaskStatus::Completed;
    store
        .create_task(&task_completed)
        .expect("create completed");

    let mut task_backlog = make_test_task(&store);
    task_backlog.status = TaskStatus::Backlog;
    store.create_task(&task_backlog).expect("create backlog");

    let filter = TaskFilter {
        statuses: Some(vec![TaskStatus::Pending]),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].status, TaskStatus::Pending);
}

/// Seed four tasks with known label sets for the match-all filter cases.
fn seed_labeled_tasks(store: &SqliteStore) {
    for (title, labels) in [
        ("both", &["work", "urgent"][..]),
        ("work-only", &["work"][..]),
        ("personal-only", &["personal"][..]),
        ("unlabeled", &[][..]),
    ] {
        let mut task = make_test_task(store);
        task.title = title.to_owned();
        task.labels = labels.iter().map(|&l| l.to_owned()).collect();
        store.create_task(&task).expect("create labeled task");
    }
}

/// Seed the standard labeled tasks, run `filter`, and assert the sorted result
/// titles equal `expected_titles`.
fn assert_filter_titles(filter: &TaskFilter, expected_titles: &[&str]) {
    let store = SqliteStore::new_in_memory();
    seed_labeled_tasks(&store);
    let mut titles: Vec<String> = store
        .list_tasks(filter)
        .expect("list_tasks")
        .into_iter()
        .map(|task| task.title)
        .collect();
    titles.sort();
    assert_eq!(titles, expected_titles);
}

/// Seed the standard labeled tasks, run `filter`, and assert exactly one match
/// whose title is `expected_title`.
fn assert_filter_single_title(filter: &TaskFilter, expected_title: &str) {
    let store = SqliteStore::new_in_memory();
    seed_labeled_tasks(&store);
    let found = store.list_tasks(filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, expected_title);
}

/// A single task carrying `task_labels` is returned when `filter` sets only
/// empty-collection fields (which must behave like `None`).
fn assert_unfiltered_single(task_labels: &[&str], filter: &TaskFilter) {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.labels = task_labels.iter().map(|&l| l.to_owned()).collect();
    store.create_task(&task).expect("create_task");
    let found = store.list_tasks(filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
}

#[test_case(&["work"], &["both", "work-only"]; "single label matches every carrier")]
#[test_case(&["work", "urgent"], &["both"]; "two labels require both")]
#[test_case(&["work", "personal"], &[]; "labels never co-carried match nothing")]
#[test_case(&["work", "work"], &["both", "work-only"]; "duplicate labels deduplicate")]
#[test_case(&["missing"], &[]; "unknown label matches nothing")]
fn list_tasks_filter_by_labels_matches_all(filter_labels: &[&str], expected_titles: &[&str]) {
    let filter = TaskFilter {
        labels: Some(filter_labels.iter().map(|&l| l.to_owned()).collect()),
        ..TaskFilter::default()
    };
    assert_filter_titles(&filter, expected_titles);
}

#[test]
fn list_tasks_filter_combines_statuses_and_labels() {
    let store = SqliteStore::new_in_memory();

    for (title, status, labels) in [
        ("scheduled-work", TaskStatus::Scheduled, &["work"][..]),
        ("backlog-work", TaskStatus::Backlog, &["work"][..]),
        ("scheduled-plain", TaskStatus::Scheduled, &[][..]),
    ] {
        let mut task = make_test_task(&store);
        task.title = title.to_owned();
        task.status = status;
        task.labels = labels.iter().map(|&l| l.to_owned()).collect();
        store.create_task(&task).expect("create task");
    }

    let filter = TaskFilter {
        statuses: Some(vec![TaskStatus::Scheduled]),
        labels: Some(vec!["work".to_owned()]),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "scheduled-work");
}

#[test_case(&["work"], &["personal-only", "unlabeled"]; "excluding a label removes every carrier")]
#[test_case(&["work", "personal"], &["unlabeled"]; "excluding several labels removes any carrier")]
#[test_case(&["urgent"], &["personal-only", "unlabeled", "work-only"]; "exclusion removes only actual carriers")]
#[test_case(&["missing"], &["both", "personal-only", "unlabeled", "work-only"]; "unknown excluded label removes nothing")]
fn list_tasks_filter_by_excluded_labels_matches_none(excluded: &[&str], expected_titles: &[&str]) {
    let filter = TaskFilter {
        excluded_labels: Some(excluded.iter().map(|&l| l.to_owned()).collect()),
        ..TaskFilter::default()
    };
    assert_filter_titles(&filter, expected_titles);
}

#[test]
fn list_tasks_filter_combines_labels_and_excluded_labels() {
    // Include "work" carriers, then drop the ones also carrying "urgent".
    let filter = TaskFilter {
        labels: Some(vec!["work".to_owned()]),
        excluded_labels: Some(vec!["urgent".to_owned()]),
        ..TaskFilter::default()
    };
    assert_filter_single_title(&filter, "work-only");
}

#[test_case(true, &["unlabeled"]; "true keeps only tasks with no labels")]
#[test_case(false, &["both", "personal-only", "work-only"]; "false keeps only labeled tasks")]
fn list_tasks_filter_by_unlabeled(unlabeled: bool, expected_titles: &[&str]) {
    let filter = TaskFilter {
        unlabeled: Some(unlabeled),
        ..TaskFilter::default()
    };
    assert_filter_titles(&filter, expected_titles);
}

#[test]
fn list_tasks_filter_combines_unlabeled_with_excluded_labels() {
    // Labeled tasks only, minus every "work" carrier.
    let filter = TaskFilter {
        unlabeled: Some(false),
        excluded_labels: Some(vec!["work".to_owned()]),
        ..TaskFilter::default()
    };
    assert_filter_single_title(&filter, "personal-only");
}

#[test]
fn list_tasks_unlabeled_true_with_label_include_matches_nothing() {
    let store = SqliteStore::new_in_memory();
    seed_labeled_tasks(&store);

    // An unlabeled task cannot carry "work": the AND combination is empty.
    let filter = TaskFilter {
        unlabeled: Some(true),
        labels: Some(vec!["work".to_owned()]),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert!(found.is_empty());
}

#[test]
fn list_tasks_filter_by_search_text() {
    let store = SqliteStore::new_in_memory();

    let mut login_title = make_test_task(&store);
    login_title.title = "Fix the login bug".to_owned();
    login_title.description = Some("Description A".to_owned());
    store.create_task(&login_title).expect("create login_title");

    let mut login_desc = make_test_task(&store);
    login_desc.title = "Add feature X".to_owned();
    login_desc.description = Some("This involves login improvements".to_owned());
    store.create_task(&login_desc).expect("create login_desc");

    let mut unrelated = make_test_task(&store);
    unrelated.title = "Refactor module Y".to_owned();
    unrelated.description = None;
    store.create_task(&unrelated).expect("create unrelated");

    // Search for "login" — should match login_title (title) and login_desc (description).
    let filter = TaskFilter {
        search_text: Some("login".to_owned()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 2);
}

#[test]
fn list_tasks_filter_by_priority() {
    let store = SqliteStore::new_in_memory();

    let mut high_prio = make_test_task(&store);
    high_prio.priority = Priority::High;
    store.create_task(&high_prio).expect("create high");

    let mut low_prio = make_test_task(&store);
    low_prio.priority = Priority::Low;
    store.create_task(&low_prio).expect("create low");

    let filter = TaskFilter {
        priorities: Some(vec![Priority::High]),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].priority, Priority::High);
}

#[test]
fn list_tasks_filter_by_multiple_priorities_matches_any() {
    let store = SqliteStore::new_in_memory();

    for priority in [Priority::Critical, Priority::High, Priority::Low] {
        let mut task = make_test_task(&store);
        task.priority = priority;
        store.create_task(&task).expect("create task");
    }

    let filter = TaskFilter {
        priorities: Some(vec![Priority::High, Priority::Critical]),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|t| t.priority != Priority::Low));
}

#[test]
fn list_tasks_empty_priorities_is_unconstrained() {
    let store = SqliteStore::new_in_memory();

    let mut task = make_test_task(&store);
    task.priority = Priority::Low;
    store.create_task(&task).expect("create task");

    let filter = TaskFilter {
        priorities: Some(vec![]),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
}

#[test]
fn get_schedulable_tasks_filters_correctly() {
    let store = SqliteStore::new_in_memory();

    // Pending with remaining time — schedulable.
    let mut task_pending = make_test_task(&store);
    task_pending.status = TaskStatus::Pending;
    task_pending.duration_minutes = 60;
    task_pending.time_logged_minutes = 0;
    store.create_task(&task_pending).expect("create pending");

    // Scheduled with remaining time — schedulable.
    let mut task_scheduled = make_test_task(&store);
    task_scheduled.status = TaskStatus::Scheduled;
    task_scheduled.duration_minutes = 60;
    task_scheduled.time_logged_minutes = 30;
    store
        .create_task(&task_scheduled)
        .expect("create scheduled");

    // Completed — not schedulable.
    let mut task_completed = make_test_task(&store);
    task_completed.status = TaskStatus::Completed;
    store
        .create_task(&task_completed)
        .expect("create completed");

    // Pending but fully logged — not schedulable.
    let mut task_fully_logged = make_test_task(&store);
    task_fully_logged.status = TaskStatus::Pending;
    task_fully_logged.duration_minutes = 60;
    task_fully_logged.time_logged_minutes = 60;
    store
        .create_task(&task_fully_logged)
        .expect("create fully logged");

    // Backlog — not schedulable.
    let mut task_backlog = make_test_task(&store);
    task_backlog.status = TaskStatus::Backlog;
    store.create_task(&task_backlog).expect("create backlog");

    // Cancelled — not schedulable.
    let mut task_cancelled = make_test_task(&store);
    task_cancelled.status = TaskStatus::Cancelled;
    store
        .create_task(&task_cancelled)
        .expect("create cancelled");

    let schedulable = store
        .get_schedulable_tasks()
        .expect("get_schedulable_tasks");
    assert_eq!(schedulable.len(), 2);

    let ids: Vec<&str> = schedulable.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&task_pending.id.as_str()));
    assert!(ids.contains(&task_scheduled.id.as_str()));
}

#[test_case(Priority::Low, 0 ; "low")]
#[test_case(Priority::Medium, 1 ; "medium")]
#[test_case(Priority::High, 2 ; "high")]
#[test_case(Priority::Critical, 3 ; "critical")]
fn priority_round_trips_all_variants(variant: Priority, expected_i64: i64) {
    assert_eq!(priority_to_i64(variant), expected_i64);
    assert_eq!(priority_from_i64(expected_i64).unwrap(), variant);
}

#[test]
fn priority_from_i64_rejects_unknown() {
    assert!(priority_from_i64(99).is_err());
}

#[test_case(TaskStatus::Backlog, "backlog" ; "backlog")]
#[test_case(TaskStatus::Pending, "pending" ; "pending")]
#[test_case(TaskStatus::Scheduled, "scheduled" ; "scheduled")]
#[test_case(TaskStatus::Completed, "completed" ; "completed")]
#[test_case(TaskStatus::Cancelled, "cancelled" ; "cancelled")]
fn status_round_trips_all_variants(variant: TaskStatus, expected_str: &str) {
    assert_eq!(status_to_str(variant), expected_str);
    assert_eq!(status_from_str(expected_str).unwrap(), variant);
}

#[test]
fn status_from_str_rejects_unknown() {
    assert!(status_from_str("bogus").is_err());
}

#[test]
fn create_and_get_task_without_deadline_or_start_date() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.start_date = None;
    task.deadline = None;

    store.create_task(&task).expect("create_task");
    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");

    assert!(loaded.start_date.is_none());
    assert!(loaded.deadline.is_none());
}

#[test_case(Priority::Low; "low")]
#[test_case(Priority::Medium; "medium")]
#[test_case(Priority::High; "high")]
#[test_case(Priority::Critical; "critical")]
fn create_task_with_priority_roundtrip(priority: Priority) {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.priority = priority;
    store.create_task(&task).expect("create_task");
    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");
    assert_eq!(loaded.priority, priority);
}

#[test_case(TaskStatus::Backlog; "backlog")]
#[test_case(TaskStatus::Pending; "pending")]
#[test_case(TaskStatus::Scheduled; "scheduled")]
#[test_case(TaskStatus::Completed; "completed")]
#[test_case(TaskStatus::Cancelled; "cancelled")]
fn create_task_with_status_roundtrip(status: TaskStatus) {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.status = status;
    store.create_task(&task).expect("create_task");
    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");
    assert_eq!(loaded.status, status);
}

/// Create a [`Task`] with `deadline` set, persisted to the store.
fn test_task_with_deadline(store: &SqliteStore, deadline: DateTime<Utc>) -> Task {
    let mut task = make_test_task(store);
    task.deadline = Some(deadline);
    store.create_task(&task).expect("create task");
    task
}

#[test]
fn list_tasks_filter_by_deadline_range() {
    let store = SqliteStore::new_in_memory();

    let _early =
        test_task_with_deadline(&store, Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap());
    let _mid = test_task_with_deadline(&store, Utc.with_ymd_and_hms(2026, 3, 20, 0, 0, 0).unwrap());
    let _late =
        test_task_with_deadline(&store, Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0).unwrap());

    // deadline_before: only early (deadline < 2026-03-15)
    let filter = TaskFilter {
        deadline_before: Some(Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list before");
    assert_eq!(found.len(), 1);

    // deadline_after: only late (deadline > 2026-03-25)
    let filter = TaskFilter {
        deadline_after: Some(Utc.with_ymd_and_hms(2026, 3, 25, 0, 0, 0).unwrap()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list after");
    assert_eq!(found.len(), 1);
}

#[test]
fn list_tasks_filter_by_schedule_id() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);
    let schedule_id = task.schedule_id.clone();
    store.create_task(&task).expect("create_task");

    // Matches the task's schedule_id.
    let filter = TaskFilter {
        schedule_id: Some(schedule_id),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 1);

    // Non-matching schedule_id returns nothing.
    let filter = TaskFilter {
        schedule_id: Some("nonexistent".to_owned()),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert!(found.is_empty());
}

#[test]
fn list_tasks_filter_by_recurring_template_id() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);

    // Insert a template first so the FK is valid.
    let template_id = uuid::Uuid::now_v7().to_string();
    let now = fixture_base().to_rfc3339();
    {
        let conn = store.conn.lock().expect("lock");
        conn.execute(
            "INSERT INTO recurring_templates (id, title, duration_minutes, priority, schedule_id, cadence_type, cadence_data, created_at, updated_at) \
             VALUES (?1, 'T', 30, 1, ?2, 'weekly', '{}', ?3, ?4)",
            rusqlite::params![template_id, task.schedule_id, now, now],
        )
        .expect("insert template");
    }
    task.recurring_template_id = Some(template_id.clone());
    store.create_task(&task).expect("create_task");

    let filter = TaskFilter {
        recurring_template_id: Some(template_id),
        ..TaskFilter::default()
    };
    let found = store.list_tasks(&filter).expect("list_tasks");
    assert_eq!(found.len(), 1);
}

#[test_case(
    &TaskFilter { statuses: Some(Vec::new()), ..TaskFilter::default() },
    &[];
    "empty statuses is unconstrained"
)]
#[test_case(
    &TaskFilter { labels: Some(Vec::new()), ..TaskFilter::default() },
    &[];
    "empty labels is unconstrained"
)]
#[test_case(
    &TaskFilter { excluded_labels: Some(Vec::new()), ..TaskFilter::default() },
    &["work"];
    "empty excluded_labels is unconstrained"
)]
fn list_tasks_with_empty_collection_returns_all(filter: &TaskFilter, task_labels: &[&str]) {
    assert_unfiltered_single(task_labels, filter);
}

#[test]
fn task_expire_at_none_roundtrips() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.expire_at = None;

    store.create_task(&task).expect("create_task");
    let loaded = store
        .get_task(&task.id)
        .expect("get_task")
        .expect("task should exist");

    assert!(loaded.expire_at.is_none());
}

/// Flatten [`crate::domain::inputs::LabelCount`]s into comparable tuples.
fn label_pairs(store: &SqliteStore) -> Vec<(String, i64)> {
    store
        .list_labels()
        .expect("list_labels")
        .into_iter()
        .map(|l| (l.label, l.task_count))
        .collect()
}

#[test]
fn list_labels_empty_store_returns_empty() {
    let store = SqliteStore::new_in_memory();
    assert!(label_pairs(&store).is_empty());
}

#[test]
fn list_labels_counts_tasks_and_sorts_by_label() {
    let store = SqliteStore::new_in_memory();
    let mut a = make_test_task(&store);
    a.labels = vec!["deep".to_owned(), "agent".to_owned()];
    let mut b = make_test_task(&store);
    b.labels = vec!["agent".to_owned()];
    store.create_task(&a).expect("create a");
    store.create_task(&b).expect("create b");

    assert_eq!(
        label_pairs(&store),
        vec![("agent".to_owned(), 2), ("deep".to_owned(), 1)]
    );
}

#[test]
fn list_labels_includes_template_only_labels_with_zero_count() {
    let store = SqliteStore::new_in_memory();
    create_labeled_task(&store, &["shared"]);

    // "shared" also on a template must not inflate the task count;
    // "template-only" must still appear, with a count of 0.
    let mut template = make_test_template(&store);
    template.labels = vec!["shared".to_owned(), "template-only".to_owned()];
    store.create_template(&template).expect("create template");

    assert_eq!(
        label_pairs(&store),
        vec![("shared".to_owned(), 1), ("template-only".to_owned(), 0)]
    );
}

#[test]
fn list_labels_inside_transaction_sees_uncommitted_labels() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.labels = vec!["tx-label".to_owned()];

    store
        .with_tx(&mut |tx| {
            tx.create_task(&task)?;
            let labels = tx.list_labels()?;
            assert_eq!(labels.len(), 1);
            assert_eq!(labels[0].label, "tx-label");
            assert_eq!(labels[0].task_count, 1);
            Ok(())
        })
        .expect("with_tx");
}

#[test]
fn fetch_labels_for_tasks_empty_input() {
    let store = SqliteStore::new_in_memory();
    let result = store
        .with_conn_for_test(|conn| fetch_labels_for_tasks(conn, &[]).expect("fetch empty input"));
    assert!(result.is_empty());
}

#[test]
fn fetch_labels_for_tasks_single_task_multiple_labels() {
    let store = SqliteStore::new_in_memory();
    let mut task = make_test_task(&store);
    task.labels = vec!["b-label".to_owned(), "a-label".to_owned()];
    store.create_task(&task).expect("create task");

    let result = store.with_conn_for_test(|conn| {
        fetch_labels_for_tasks(conn, &[task.id.as_str()]).expect("fetch single task")
    });
    let mut labels = result.get(&task.id).cloned().unwrap_or_default();
    labels.sort();
    assert_eq!(labels, vec!["a-label", "b-label"]);
}

#[test]
fn fetch_labels_for_tasks_multiple_tasks_no_cross_contamination() {
    let store = SqliteStore::new_in_memory();
    let task_a = create_labeled_task(&store, &["foo"]);
    let task_b = create_labeled_task(&store, &["bar"]);

    let result = store.with_conn_for_test(|conn| {
        fetch_labels_for_tasks(conn, &[task_a.id.as_str(), task_b.id.as_str()])
            .expect("fetch multiple tasks")
    });
    assert_eq!(
        result.get(&task_a.id).cloned().unwrap_or_default(),
        vec!["foo"]
    );
    assert_eq!(
        result.get(&task_b.id).cloned().unwrap_or_default(),
        vec!["bar"]
    );
}

#[test]
fn fetch_labels_for_tasks_unlabeled_task_absent_from_map() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);
    store.create_task(&task).expect("create task");

    let result = store.with_conn_for_test(|conn| {
        fetch_labels_for_tasks(conn, &[task.id.as_str()]).expect("fetch unlabeled")
    });
    assert!(
        !result.contains_key(&task.id),
        "task with no labels must not appear in the map"
    );
}
