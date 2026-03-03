// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`CommentStore`] tests: CRUD roundtrips, newest-first ordering, FK
//! integrity (cascade delete with the task), and transaction-scoped access.

use chrono::{TimeZone, Utc};

use super::make_test_task;
use crate::db::sqlite::SqliteStore;
use crate::domain::models::Comment;
use crate::error::AppError;
use crate::traits::storage::{CommentStore, Store, TaskStore};

/// Create a store with one persisted task; returns (store, task id).
fn store_with_task() -> (SqliteStore, String) {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);
    store.create_task(&task).expect("create task");
    (store, task.id)
}

fn make_comment(task_id: &str, id: &str) -> Comment {
    Comment::test_default().with_id(id).with_task(task_id)
}

/// Seed one comment per `(id, hour)` pair (same day, `hour` as both the
/// created/updated timestamp), then return the listed ids in order.
fn seed_and_list_ids(store: &SqliteStore, task_id: &str, entries: &[(&str, u32)]) -> Vec<String> {
    for (id, hour) in entries {
        let ts = Utc.with_ymd_and_hms(2026, 7, 13, *hour, 0, 0).unwrap();
        store
            .create_comment(&make_comment(task_id, id).with_timestamps(ts, ts))
            .expect("create");
    }
    store
        .list_comments_for_task(task_id)
        .expect("list")
        .into_iter()
        .map(|c| c.id)
        .collect()
}

#[test]
fn create_and_get_roundtrip() {
    let (store, task_id) = store_with_task();
    let created = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
    let updated = Utc.with_ymd_and_hms(2026, 7, 13, 11, 0, 0).unwrap();
    let comment = make_comment(&task_id, "c-1")
        .with_author("User")
        .with_timestamps(created, updated);

    store.create_comment(&comment).expect("create comment");
    let fetched = store
        .get_comment("c-1")
        .expect("get comment")
        .expect("comment exists");

    assert_eq!(fetched, comment);
}

#[test]
fn get_missing_returns_none() {
    let (store, _task_id) = store_with_task();
    assert_eq!(store.get_comment("nope").expect("get"), None);
}

#[test]
fn update_writes_editable_fields_only() {
    let (store, task_id) = store_with_task();
    let comment = make_comment(&task_id, "c-1");
    store.create_comment(&comment).expect("create");

    let new_updated = Utc.with_ymd_and_hms(2026, 7, 14, 9, 0, 0).unwrap();
    let mut edited = comment.clone();
    edited.content = "Edited content".to_owned();
    edited.updated_at = new_updated;
    // A tampered author must NOT be persisted — immutable after creation (M12.3).
    edited.author = "Mallory".to_owned();
    store.update_comment(&edited).expect("update");

    let fetched = store.get_comment("c-1").expect("get").expect("exists");
    assert_eq!(fetched.content, "Edited content");
    assert_eq!(fetched.updated_at, new_updated);
    assert_eq!(fetched.created_at, comment.created_at);
    assert_eq!(fetched.author, comment.author);
}

#[test]
fn update_missing_returns_not_found() {
    let (store, task_id) = store_with_task();
    let ghost = make_comment(&task_id, "ghost");

    let err = store.update_comment(&ghost).expect_err("must fail");
    assert!(
        matches!(err, AppError::NotFound { ref entity, ref id } if entity == "Comment" && id == "ghost"),
        "got: {err}"
    );
}

#[test]
fn delete_removes_comment() {
    let (store, task_id) = store_with_task();
    store
        .create_comment(&make_comment(&task_id, "c-1"))
        .expect("create");

    store.delete_comment("c-1").expect("delete");
    assert_eq!(store.get_comment("c-1").expect("get"), None);
}

#[test]
fn delete_missing_is_ok() {
    let (store, _task_id) = store_with_task();
    store.delete_comment("nope").expect("idempotent delete");
}

// ── Listing order (M12.4: newest first) ─────────────────────────────────

#[test]
fn list_returns_newest_first() {
    let (store, task_id) = store_with_task();
    let ids = seed_and_list_ids(
        &store,
        &task_id,
        &[("c-old", 8), ("c-new", 12), ("c-mid", 10)],
    );
    assert_eq!(ids, vec!["c-new", "c-mid", "c-old"]);
}

#[test]
fn list_breaks_created_at_ties_by_id_descending() {
    let (store, task_id) = store_with_task();
    let ids = seed_and_list_ids(&store, &task_id, &[("c-a", 10), ("c-b", 10)]);
    assert_eq!(ids, vec!["c-b", "c-a"]);
}

#[test]
fn list_for_task_without_comments_is_empty() {
    let (store, task_id) = store_with_task();
    assert!(store
        .list_comments_for_task(&task_id)
        .expect("list")
        .is_empty());
}

#[test]
fn list_excludes_other_tasks_comments() {
    let (store, task_id) = store_with_task();
    let other = make_test_task(&store);
    store.create_task(&other).expect("create other task");

    store
        .create_comment(&make_comment(&task_id, "c-mine"))
        .expect("create");
    store
        .create_comment(&make_comment(&other.id, "c-theirs"))
        .expect("create");

    let ids: Vec<String> = store
        .list_comments_for_task(&task_id)
        .expect("list")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec!["c-mine"]);
}

// ── Foreign-key integrity (M12.8) ───────────────────────────────────────

#[test]
fn create_for_missing_task_fails() {
    let (store, _task_id) = store_with_task();
    let orphan = make_comment("no-such-task", "c-orphan");

    let err = store.create_comment(&orphan).expect_err("FK must reject");
    assert!(matches!(err, AppError::Database(_)), "got: {err}");
}

#[test]
fn deleting_task_cascades_to_comments() {
    let (store, task_id) = store_with_task();
    store
        .create_comment(&make_comment(&task_id, "c-1"))
        .expect("create");

    store.delete_task(&task_id).expect("delete task");
    assert_eq!(store.get_comment("c-1").expect("get"), None);
}

#[test]
fn comment_crud_works_inside_with_tx() {
    let (store, task_id) = store_with_task();
    let comment = make_comment(&task_id, "c-tx");

    store
        .with_tx(&mut |tx| {
            tx.create_comment(&comment)?;
            let listed = tx.list_comments_for_task(&task_id)?;
            assert_eq!(listed.len(), 1);
            Ok(())
        })
        .expect("tx commits");

    assert!(store.get_comment("c-tx").expect("get").is_some());
}

#[test]
fn failed_tx_rolls_back_comment_insert() {
    let (store, task_id) = store_with_task();
    let comment = make_comment(&task_id, "c-rollback");

    let result = store.with_tx(&mut |tx| {
        tx.create_comment(&comment)?;
        Err(AppError::Validation("boom".to_owned()))
    });
    assert!(result.is_err());
    assert_eq!(store.get_comment("c-rollback").expect("get"), None);
}
