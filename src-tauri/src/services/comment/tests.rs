// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Comment service tests: CRUD validation, M12.3 ownership rules, and the
//! M12.5 system-comment content builders.

use test_case::test_case;

use super::{
    chunk_completed_content, chunk_reopened_content, create_comment, delete_comment,
    format_minutes, list_comments, system_comment, update_comment, DEFAULT_AUTHOR, SYSTEM_AUTHOR,
};
use crate::db::sqlite::SqliteStore;
use crate::domain::inputs::{CreateCommentInput, UpdateCommentInput};
use crate::domain::models::{Comment, Task};
use crate::error::AppError;
use crate::test_support::{seed_task, test_store, utc};
use crate::traits::storage::CommentStore;

/// A store with one persisted task; returns (store, task id).
fn store_with_task() -> (SqliteStore, String) {
    let store = test_store();
    let task = Task::test_default().with_id("task-1");
    seed_task(&store, &task);
    (store, task.id)
}

fn create_input(task_id: &str, content: &str, author: Option<&str>) -> CreateCommentInput {
    CreateCommentInput {
        task_id: task_id.to_owned(),
        content: content.to_owned(),
        author: author.map(str::to_owned),
    }
}

fn content_patch(content: &str) -> UpdateCommentInput {
    UpdateCommentInput {
        content: Some(content.to_owned()),
    }
}

fn assert_validation(err: &AppError, expected: &str) {
    assert!(
        matches!(err, AppError::Validation(msg) if msg == expected),
        "expected Validation({expected:?}), got: {err}"
    );
}

#[test]
fn create_persists_comment_with_now_timestamps() {
    let (store, task_id) = store_with_task();
    let now = utc(2026, 7, 13, 10, 0);

    let comment = create_comment(&store, create_input(&task_id, "First!", Some("Alice")), now)
        .expect("create");

    assert_eq!(comment.task_id, task_id);
    assert_eq!(comment.author, "Alice");
    assert_eq!(comment.content, "First!");
    assert_eq!(comment.created_at, now);
    assert_eq!(comment.updated_at, now);
    assert!(!comment.id.is_empty());
    let fetched = store.get_comment(&comment.id).expect("get").expect("row");
    assert_eq!(fetched, comment);
}

#[test]
fn create_defaults_author_to_user_when_none() {
    let (store, task_id) = store_with_task();
    let comment = create_comment(
        &store,
        create_input(&task_id, "hi", None),
        utc(2026, 7, 13, 10, 0),
    )
    .expect("create");
    assert_eq!(comment.author, DEFAULT_AUTHOR);
}

#[test]
fn create_trims_author_but_preserves_content_verbatim() {
    // Content is Markdown — leading whitespace can be intentional
    // (indented code blocks), so only the author is normalized.
    let (store, task_id) = store_with_task();
    let comment = create_comment(
        &store,
        create_input(&task_id, "    indented code\n", Some("  Alice  ")),
        utc(2026, 7, 13, 10, 0),
    )
    .expect("create");
    assert_eq!(comment.author, "Alice");
    assert_eq!(comment.content, "    indented code\n");
}

#[test_case("" ; "empty")]
#[test_case("   \n\t" ; "whitespace only")]
fn create_rejects_blank_content(content: &str) {
    let (store, task_id) = store_with_task();
    let err = create_comment(
        &store,
        create_input(&task_id, content, None),
        utc(2026, 7, 13, 10, 0),
    )
    .expect_err("must reject");
    assert_validation(&err, "Comment content must not be empty");
}

#[test_case("SYSTEM" ; "exact")]
#[test_case("  SYSTEM  " ; "padded")]
fn create_rejects_reserved_system_author(author: &str) {
    let (store, task_id) = store_with_task();
    let err = create_comment(
        &store,
        create_input(&task_id, "hi", Some(author)),
        utc(2026, 7, 13, 10, 0),
    )
    .expect_err("must reject");
    assert_validation(&err, "Author \"SYSTEM\" is reserved for system comments");
}

#[test]
fn create_rejects_blank_author() {
    let (store, task_id) = store_with_task();
    let err = create_comment(
        &store,
        create_input(&task_id, "hi", Some("   ")),
        utc(2026, 7, 13, 10, 0),
    )
    .expect_err("must reject");
    assert_validation(&err, "Comment author must not be empty");
}

#[test]
fn create_for_missing_task_returns_not_found() {
    let (store, _task_id) = store_with_task();
    let err = create_comment(
        &store,
        create_input("no-such-task", "hi", None),
        utc(2026, 7, 13, 10, 0),
    )
    .expect_err("must reject");
    assert!(
        matches!(err, AppError::NotFound { ref entity, ref id } if entity == "Task" && id == "no-such-task"),
        "got: {err}"
    );
}

/// Seed a comment through the service and return it.
fn seeded_comment(store: &SqliteStore, task_id: &str, author: &str) -> Comment {
    create_comment(
        store,
        create_input(task_id, "original", Some(author)),
        utc(2026, 7, 13, 9, 0),
    )
    .expect("seed comment")
}

/// Seed a SYSTEM comment directly through the store (the service refuses).
fn seeded_system_comment(store: &SqliteStore, task_id: &str) -> Comment {
    let comment = system_comment(
        task_id,
        "Chunk completed".to_owned(),
        utc(2026, 7, 13, 9, 0),
    );
    store.create_comment(&comment).expect("seed system comment");
    comment
}

#[test]
fn update_replaces_content_and_bumps_updated_at() {
    let (store, task_id) = store_with_task();
    let original = seeded_comment(&store, &task_id, "User");
    let later = utc(2026, 7, 13, 12, 30);

    let updated = update_comment(
        &store,
        &original.id,
        &content_patch("edited"),
        "User",
        later,
    )
    .expect("update");

    assert_eq!(updated.content, "edited");
    assert_eq!(updated.updated_at, later);
    assert_eq!(updated.created_at, original.created_at);
    let fetched = store.get_comment(&original.id).expect("get").expect("row");
    assert_eq!(fetched, updated);
}

#[test]
fn update_with_none_content_is_a_noop() {
    let (store, task_id) = store_with_task();
    let original = seeded_comment(&store, &task_id, "User");

    let result = update_comment(
        &store,
        &original.id,
        &UpdateCommentInput::default(),
        "User",
        utc(2026, 7, 13, 12, 30),
    )
    .expect("no-op update");

    assert_eq!(result, original);
    let fetched = store.get_comment(&original.id).expect("get").expect("row");
    assert_eq!(fetched, original);
}

#[test]
fn update_rejects_blank_content() {
    let (store, task_id) = store_with_task();
    let original = seeded_comment(&store, &task_id, "User");
    let err = update_comment(
        &store,
        &original.id,
        &content_patch("  \n"),
        "User",
        utc(2026, 7, 13, 12, 30),
    )
    .expect_err("must reject");
    assert_validation(&err, "Comment content must not be empty");
}

#[test]
fn update_missing_comment_returns_not_found() {
    let (store, _task_id) = store_with_task();
    let err = update_comment(
        &store,
        "ghost",
        &content_patch("x"),
        "User",
        utc(2026, 7, 13, 12, 30),
    )
    .expect_err("must reject");
    assert!(
        matches!(err, AppError::NotFound { ref entity, ref id } if entity == "Comment" && id == "ghost"),
        "got: {err}"
    );
}

#[test]
fn update_rejects_system_comment() {
    let (store, task_id) = store_with_task();
    let system = seeded_system_comment(&store, &task_id);
    let err = update_comment(
        &store,
        &system.id,
        &content_patch("x"),
        SYSTEM_AUTHOR,
        utc(2026, 7, 13, 12, 30),
    )
    .expect_err("must reject");
    assert_validation(&err, "System comments cannot be edited");
}

#[test]
fn update_rejects_foreign_author() {
    let (store, task_id) = store_with_task();
    let original = seeded_comment(&store, &task_id, "Alice");
    let err = update_comment(
        &store,
        &original.id,
        &content_patch("x"),
        "User",
        utc(2026, 7, 13, 12, 30),
    )
    .expect_err("must reject");
    assert_validation(&err, "Comments can only be edited by their author");
}

#[test]
fn delete_removes_own_comment() {
    let (store, task_id) = store_with_task();
    let comment = seeded_comment(&store, &task_id, "User");

    delete_comment(&store, &comment.id, "User").expect("delete");
    assert_eq!(store.get_comment(&comment.id).expect("get"), None);
}

#[test]
fn delete_missing_comment_returns_not_found() {
    let (store, _task_id) = store_with_task();
    let err = delete_comment(&store, "ghost", "User").expect_err("must reject");
    assert!(
        matches!(err, AppError::NotFound { ref entity, .. } if entity == "Comment"),
        "got: {err}"
    );
}

#[test]
fn delete_rejects_system_comment() {
    let (store, task_id) = store_with_task();
    let system = seeded_system_comment(&store, &task_id);
    let err = delete_comment(&store, &system.id, SYSTEM_AUTHOR).expect_err("must reject");
    assert_validation(&err, "System comments cannot be deleted");
}

#[test]
fn delete_rejects_foreign_author() {
    let (store, task_id) = store_with_task();
    let comment = seeded_comment(&store, &task_id, "Alice");
    let err = delete_comment(&store, &comment.id, "User").expect_err("must reject");
    assert_validation(&err, "Comments can only be deleted by their author");
    assert!(store.get_comment(&comment.id).expect("get").is_some());
}

#[test]
fn list_returns_newest_first() {
    let (store, task_id) = store_with_task();
    for (content, hour) in [("oldest", 8), ("newest", 12), ("middle", 10)] {
        create_comment(
            &store,
            create_input(&task_id, content, None),
            utc(2026, 7, 13, hour, 0),
        )
        .expect("create");
    }

    let contents: Vec<String> = list_comments(&store, &task_id)
        .expect("list")
        .into_iter()
        .map(|c| c.content)
        .collect();
    assert_eq!(contents, vec!["newest", "middle", "oldest"]);
}

#[test]
fn list_for_missing_task_returns_not_found() {
    let (store, _task_id) = store_with_task();
    let err = list_comments(&store, "no-such-task").expect_err("must reject");
    assert!(
        matches!(err, AppError::NotFound { ref entity, .. } if entity == "Task"),
        "got: {err}"
    );
}

#[test]
fn system_comment_uses_reserved_author_and_now() {
    let now = utc(2026, 7, 13, 18, 45);
    let comment = system_comment("task-1", "Chunk completed".to_owned(), now);

    assert_eq!(comment.author, SYSTEM_AUTHOR);
    assert_eq!(comment.task_id, "task-1");
    assert_eq!(comment.content, "Chunk completed");
    assert_eq!(comment.created_at, now);
    assert_eq!(comment.updated_at, now);
    assert!(!comment.id.is_empty());
}

#[test_case(0, "0m" ; "zero")]
#[test_case(45, "45m" ; "minutes only")]
#[test_case(60, "1h" ; "exact hour")]
#[test_case(75, "1h 15m" ; "hours and minutes")]
#[test_case(120, "2h" ; "exact hours")]
fn format_minutes_renders_compact_duration(minutes: i64, expected: &str) {
    assert_eq!(format_minutes(minutes), expected);
}

#[test]
fn chunk_completed_content_matches_m12_format() {
    assert_eq!(
        chunk_completed_content(45, 75, 120),
        "Chunk completed: +45m logged (1h 15m / 2h total)"
    );
}

#[test]
fn chunk_reopened_content_matches_m12_format() {
    assert_eq!(
        chunk_reopened_content(45, 30, 120),
        "Chunk reopened: -45m logged (30m / 2h total)"
    );
}
