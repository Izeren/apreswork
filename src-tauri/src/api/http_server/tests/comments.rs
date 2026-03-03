// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Comment endpoint tests (M12): create / list / edit / delete, the
//! default-author contract, and the system-comment mutation guards.

use super::{
    assert_guard_fired, assert_not_found, assert_validation_error, body_json, get_request,
    json_request, make_http_test_task, memory_state, request,
};
use crate::api::http_server::build_router;
use crate::domain::models::Comment;
use crate::services::comment::system_comment;
use crate::state::AppState;
use crate::test_support::test_now;
use axum::http::{Method, StatusCode};
use serde_json::json;
use test_case::test_case;

/// Seed a task and return (state, task id) — every comment hangs off a task.
fn state_with_task() -> (std::sync::Arc<AppState>, String) {
    let state = memory_state();
    let task = make_http_test_task(&state, "Task with comments");
    state.store.create_task(&task).expect("seed task");
    (state, task.id)
}

/// Seed a task with one system comment and return `(state, system comment)` —
/// shared by the patch/delete-on-system-comment guard tests.
fn state_with_system_comment() -> (std::sync::Arc<AppState>, Comment) {
    let (state, task_id) = state_with_task();
    let system = system_comment(&task_id, "Chunk completed".to_owned(), test_now());
    state
        .store
        .create_comment(&system)
        .expect("seed system comment");
    (state, system)
}

/// Seed a task, post one comment with `"original"` content, and return
/// `(app, comment_json)` — shared setup for the profile-guard-on-update and
/// patch-updates-content tests.
async fn app_with_posted_original() -> (axum::Router, serde_json::Value) {
    let (state, task_id) = state_with_task();
    let app = build_router(state);
    let created = post_comment(app.clone(), &task_id, json!({ "content": "original" })).await;
    (app, created)
}

/// POST a comment and return its JSON body (asserts `201`).
async fn post_comment(
    app: axum::Router,
    task_id: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let response = json_request(
        app,
        Method::POST,
        &format!("/api/tasks/{task_id}/comments"),
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    body_json(response).await
}

#[tokio::test]
async fn profile_guard_fires_on_create_comment() {
    let (state, task_id) = state_with_task();
    let uri = format!("/api/tasks/{task_id}/comments?expected_profile_id=wrong");
    let app = build_router(state);
    let response = json_request(app, Method::POST, &uri, json!({ "content": "hi" })).await;
    assert_guard_fired(response).await;
}

#[tokio::test]
async fn profile_guard_fires_on_update_comment() {
    let (app, created) = app_with_posted_original().await;
    let id = created["id"].as_str().expect("id").to_owned();
    let uri = format!("/api/comments/{id}?expected_profile_id=wrong");
    let response = json_request(app, Method::PATCH, &uri, json!({ "content": "edited" })).await;
    assert_guard_fired(response).await;
}

#[tokio::test]
async fn create_comment_returns_201_with_default_author() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    let json = post_comment(app, &task_id, json!({ "content": "First!" })).await;
    assert_eq!(json["task_id"], task_id);
    assert_eq!(json["author"], "User");
    assert_eq!(json["content"], "First!");
    assert!(json["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(json["created_at"], json["updated_at"]);
}

#[tokio::test]
async fn create_comment_accepts_explicit_author() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    let json = post_comment(app, &task_id, json!({ "content": "hi", "author": "Agent" })).await;
    assert_eq!(json["author"], "Agent");
}

#[tokio::test]
async fn create_comment_empty_content_returns_400() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    let response = json_request(
        app,
        Method::POST,
        &format!("/api/tasks/{task_id}/comments"),
        json!({ "content": "   " }),
    )
    .await;
    assert_validation_error(response, "").await;
}

#[tokio::test]
async fn create_comment_system_author_returns_400() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    let response = json_request(
        app,
        Method::POST,
        &format!("/api/tasks/{task_id}/comments"),
        json!({ "content": "hi", "author": "SYSTEM" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test_case(Method::POST, "/api/tasks/no-such-task/comments", Some(json!({ "content": "hi" })) ; "create_comment")]
#[test_case(Method::GET, "/api/tasks/no-such-task/comments", None ; "list_comments")]
#[test_case(Method::PATCH, "/api/comments/ghost", Some(json!({ "content": "x" })) ; "patch_comment")]
#[test_case(Method::DELETE, "/api/comments/ghost", None ; "delete_comment")]
#[tokio::test]
async fn missing_entity_returns_404(method: Method, uri: &str, body: Option<serde_json::Value>) {
    let (state, _) = state_with_task();
    let app = build_router(state);
    let response = match body {
        Some(b) => json_request(app, method, uri, b).await,
        None => request(app, method, uri).await,
    };
    assert_not_found(response).await;
}

#[tokio::test]
async fn list_comments_returns_newest_first() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    for content in ["first", "second", "third"] {
        post_comment(app.clone(), &task_id, json!({ "content": content })).await;
    }

    let response = get_request(app, &format!("/api/tasks/{task_id}/comments")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let contents: Vec<&str> = json
        .as_array()
        .expect("array body")
        .iter()
        .map(|c| c["content"].as_str().expect("content string"))
        .collect();
    assert_eq!(contents, vec!["third", "second", "first"]);
}

#[tokio::test]
async fn patch_comment_updates_content() {
    let (app, created) = app_with_posted_original().await;
    let id = created["id"].as_str().expect("id");

    let response = json_request(
        app,
        Method::PATCH,
        &format!("/api/comments/{id}"),
        json!({ "content": "edited" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["content"], "edited");
    assert_eq!(json["created_at"], created["created_at"]);
}

#[tokio::test]
async fn patch_foreign_author_comment_returns_400() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    // Created as "Agent"; the REST surface acts as the default author "User".
    let created = post_comment(
        app.clone(),
        &task_id,
        json!({ "content": "hi", "author": "Agent" }),
    )
    .await;
    let id = created["id"].as_str().expect("id");

    let response = json_request(
        app,
        Method::PATCH,
        &format!("/api/comments/{id}"),
        json!({ "content": "x" }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(
        json["message"],
        "Comments can only be edited by their author"
    );
}

#[test_case(Method::PATCH, Some(json!({ "content": "tampered" })), "System comments cannot be edited" ; "patch")]
#[test_case(Method::DELETE, None, "System comments cannot be deleted" ; "delete")]
#[tokio::test]
async fn system_comment_mutation_returns_400(
    method: Method,
    body: Option<serde_json::Value>,
    expected_msg: &str,
) {
    let (state, system) = state_with_system_comment();
    let app = build_router(state);
    let uri = format!("/api/comments/{}", system.id);
    let response = match body {
        Some(b) => json_request(app, method, &uri, b).await,
        None => request(app, method, &uri).await,
    };
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["message"], expected_msg);
}

#[tokio::test]
async fn delete_comment_returns_204_and_removes_it() {
    let (state, task_id) = state_with_task();
    let app = build_router(state);

    let created = post_comment(app.clone(), &task_id, json!({ "content": "bye" })).await;
    let id = created["id"].as_str().expect("id");

    let response = request(app.clone(), Method::DELETE, &format!("/api/comments/{id}")).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = get_request(app, &format!("/api/tasks/{task_id}/comments")).await;
    let json = body_json(response).await;
    assert_eq!(json.as_array().expect("array").len(), 0);
}
