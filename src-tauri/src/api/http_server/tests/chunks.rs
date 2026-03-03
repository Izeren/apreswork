// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Chunk-oriented endpoint tests: `GET /api/agenda` and `POST /api/chunks/:id/move`.

use axum::http::StatusCode;

use super::{
    assert_guard_fired, assert_not_found, assert_validation_error, body_json, get_ok_array,
    json_request, make_http_test_chunk, memory_state, seed_auto_chunk, seed_task, seed_task_with,
};
use crate::api::http_server::build_router;

fn base_time() -> chrono::DateTime<chrono::Utc> {
    crate::test_support::utc(2026, 4, 15, 10, 0)
}

/// Seed a "Movable task" plus one auto chunk at 2026-04-15T10:00Z (30 min).
/// Returns `(state, task_id, chunk_id)`.
fn seed_movable_task_with_chunk() -> (std::sync::Arc<crate::state::AppState>, String, String) {
    let state = memory_state();
    let task = seed_task(&state, "Movable task");
    let chunk_id = seed_auto_chunk(&state, &task.id, base_time(), 30);
    (state, task.id, chunk_id)
}

async fn move_chunk_request(
    app: axum::Router,
    chunk_id: &str,
    new_start: &str,
    new_end: &str,
) -> axum::response::Response {
    json_request(
        app,
        axum::http::Method::POST,
        &format!("/api/chunks/{chunk_id}/move"),
        serde_json::json!({ "new_start": new_start, "new_end": new_end }),
    )
    .await
}

#[tokio::test]
async fn profile_guard_fires_on_move_chunk() {
    let (state, _task_id, chunk_id) = seed_movable_task_with_chunk();
    let uri = format!("/api/chunks/{chunk_id}/move?expected_profile_id=wrong");
    let response = json_request(
        build_router(state),
        axum::http::Method::POST,
        &uri,
        serde_json::json!({
            "new_start": "2026-04-16T08:00:00Z",
            "new_end": "2026-04-16T08:45:00Z"
        }),
    )
    .await;
    assert_guard_fired(response).await;
}

#[tokio::test]
async fn get_agenda_returns_items_in_range() {
    let state = memory_state();
    let task = seed_task_with(&state, "Scheduled agent task", |task| {
        task.labels = vec!["agent".to_owned()];
    });

    let chunk = make_http_test_chunk(&task.id, base_time(), 30);
    state.store.create_chunk(&chunk).expect("create chunk");

    let app = build_router(state);
    let items = get_ok_array(
        app,
        "/api/agenda?start=2026-04-15T09:00:00Z&end=2026-04-15T12:00:00Z",
    )
    .await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["task_title"], "Scheduled agent task");
    assert_eq!(items[0]["chunk"]["task_id"], task.id);
}

#[tokio::test]
async fn get_agenda_filters_by_label() {
    let state = memory_state();
    let agent_task = seed_task_with(&state, "Agent task", |task| {
        task.labels = vec!["agent".to_owned()];
    });
    let other_task = seed_task_with(&state, "Other task", |task| {
        task.labels = vec!["ops".to_owned()];
    });

    state
        .store
        .create_chunk(&make_http_test_chunk(&agent_task.id, base_time(), 30))
        .expect("create agent chunk");
    state
        .store
        .create_chunk(&make_http_test_chunk(
            &other_task.id,
            base_time() + chrono::Duration::minutes(45),
            30,
        ))
        .expect("create other chunk");

    let app = build_router(state);
    let items = get_ok_array(
        app,
        "/api/agenda?start=2026-04-15T09:00:00Z&end=2026-04-15T12:00:00Z&label=agent",
    )
    .await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["task_title"], "Agent task");
}

#[tokio::test]
async fn get_agenda_invalid_datetime_returns_400() {
    let state = memory_state();
    let app = build_router(state);
    let response =
        super::get_request(app, "/api/agenda?start=not-a-date&end=2026-04-15T12:00:00Z").await;
    assert_validation_error(response, "start").await;
}

#[tokio::test]
async fn move_chunk_endpoint_moves_and_pins_chunk() {
    let (state, task_id, chunk_id) = seed_movable_task_with_chunk();

    // 2099 keeps the moved chunk well in the future so release_stale_fixed_locks,
    // called by the synchronous reschedule_incremental (debounce=0 in tests),
    // does not unlock it. See Clock debt tasks for the wall-clock carve-out.
    let response = move_chunk_request(
        build_router(state.clone()),
        &chunk_id,
        "2099-04-16T08:00:00Z",
        "2099-04-16T08:45:00Z",
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["task_id"], task_id);
    assert_eq!(json["is_fixed"], true);

    let persisted = state
        .store
        .get_chunk(&chunk_id)
        .expect("load chunk")
        .expect("chunk exists");
    assert!(persisted.is_fixed);
    assert_eq!(
        persisted.start_time,
        crate::test_support::utc(2099, 4, 16, 8, 0)
    );
    assert_eq!(
        persisted.end_time,
        crate::test_support::utc(2099, 4, 16, 8, 45)
    );
}

#[tokio::test]
async fn move_chunk_endpoint_invalid_datetime_returns_400() {
    let (state, _task_id, chunk_id) = seed_movable_task_with_chunk();

    let response = move_chunk_request(
        build_router(state),
        &chunk_id,
        "not-a-date",
        "2026-04-16T08:45:00Z",
    )
    .await;

    assert_validation_error(response, "new_start").await;
}

#[tokio::test]
async fn move_chunk_endpoint_not_found_returns_404() {
    let state = memory_state();
    let response = move_chunk_request(
        build_router(state),
        "nonexistent-id",
        "2026-04-16T08:00:00Z",
        "2026-04-16T08:45:00Z",
    )
    .await;

    assert_not_found(response).await;
}
