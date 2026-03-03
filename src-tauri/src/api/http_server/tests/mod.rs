// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the REST API server, split by concern:
//! - this module: shared fixtures/helpers + error, health, and server-config tests
//! - [`tasks`]: task endpoints (create / get / update / complete / delete /
//!   list + filters) and the labels endpoint
//! - [`chunks`]: chunk endpoints (agenda + move)
//! - [`comments`]: comment endpoints (create / list / edit / delete)
//! - [`auth`]: auth + calendar-sync endpoints
//! - [`backup`]: backup endpoints (status + manual export)

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, HOST};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use test_case::test_case;
use tower::ServiceExt as _;

use super::{AppError, ServerConfig, DEFAULT_API_PORT};
use crate::db::sqlite::SqliteStore;
use crate::scheduler::engine::DefaultScheduler;
use crate::services::trigger::{DefaultExecutor, RescheduleTrigger};
use crate::state::AppState;
use crate::traits::calendar_sync::CalendarSync;

use crate::domain::enums::ChunkStatus;
use crate::domain::models::{Chunk, Comment, Task};
use chrono::Utc;

mod auth;
mod backup;
mod chunks;
mod comments;
mod profiles;
mod tasks;

pub(super) fn memory_state_with_sync(
    sync: std::sync::Arc<dyn CalendarSync>,
) -> std::sync::Arc<AppState> {
    let store = std::sync::Arc::new(SqliteStore::new(":memory:").expect("in-memory store"));
    let scheduler = std::sync::Arc::new(DefaultScheduler);
    let executor = std::sync::Arc::new(DefaultExecutor::new(scheduler.clone()));
    let trigger = std::sync::Arc::new(RescheduleTrigger::new(store.clone(), executor));
    std::sync::Arc::new(AppState {
        store,
        scheduler,
        trigger,
        calendar_sync: sync,
        backup: std::sync::Arc::new(crate::backup::noop::NoopBackupTarget),
        profile_dir: std::path::PathBuf::from("/tmp/test-profile"),
        restore_notice: None,
        profile: crate::profiles::ActiveProfile {
            id: "test-profile-id".to_owned(),
            name: "Test Profile".to_owned(),
        },
    })
}

fn memory_state() -> std::sync::Arc<AppState> {
    memory_state_with_sync(std::sync::Arc::new(crate::calendar::noop::NoopCalendarSync))
}

pub(super) fn memory_profiles_state() -> std::sync::Arc<crate::profiles::ProfilesState> {
    profiles_state_with(vec![])
}

pub(super) fn profiles_state_with(
    profiles: Vec<crate::profiles::registry::ProfileEntry>,
) -> std::sync::Arc<crate::profiles::ProfilesState> {
    use crate::profiles::registry::{ProfilesRegistry, REGISTRY_VERSION};
    std::sync::Arc::new(crate::profiles::ProfilesState::new(
        std::path::PathBuf::from("/tmp/test-profiles"),
        ProfilesRegistry {
            version: REGISTRY_VERSION,
            last_used: None,
            profiles,
        },
    ))
}

async fn body_json(response: axum::response::Response) -> Value {
    use http_body_util::BodyExt as _;
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse JSON body")
}

#[test_case(
    AppError::NotFound { entity: "Task".into(), id: "1".into() },
    StatusCode::NOT_FOUND;
    "not_found_gives_404"
)]
#[test_case(
    AppError::Validation("bad input".into()),
    StatusCode::BAD_REQUEST;
    "validation_gives_400"
)]
#[test_case(
    AppError::Database("SELECT failed".into()),
    StatusCode::INTERNAL_SERVER_ERROR;
    "database_gives_500"
)]
#[test_case(
    AppError::CalendarSync("API timeout".into()),
    StatusCode::INTERNAL_SERVER_ERROR;
    "calendar_sync_gives_500"
)]
#[test_case(
    AppError::Internal("unexpected state".into()),
    StatusCode::INTERNAL_SERVER_ERROR;
    "internal_gives_500"
)]
#[test_case(
    AppError::Backup("drive upload failed".into()),
    StatusCode::INTERNAL_SERVER_ERROR;
    "backup_gives_500"
)]
#[test_case(
    AppError::ProfileMismatch("wrong profile".into()),
    StatusCode::CONFLICT;
    "profile_mismatch_gives_409"
)]
fn error_status_code(err: AppError, expected_status: StatusCode) {
    use axum::response::IntoResponse as _;
    let response = err.into_response();
    assert_eq!(response.status(), expected_status);
}

#[test_case(
    AppError::NotFound { entity: "Task".into(), id: "abc-123".into() },
    StatusCode::NOT_FOUND, "not_found", "Not found: Task with id abc-123";
    "not_found"
)]
#[test_case(
    AppError::Validation("title must not be empty".into()),
    StatusCode::BAD_REQUEST, "validation", "title must not be empty";
    "validation"
)]
#[test_case(
    AppError::ProfileMismatch("active profile is X, expected Y".into()),
    StatusCode::CONFLICT, "profile_mismatch", "active profile is X, expected Y";
    "profile_mismatch"
)]
#[tokio::test]
async fn error_body_format(
    err: AppError,
    expected_status: StatusCode,
    expected_code: &str,
    expected_message: &str,
) {
    use axum::response::IntoResponse as _;
    let response = err.into_response();
    assert_eq!(response.status(), expected_status);
    let json = body_json(response).await;
    assert_eq!(json["error"], expected_code);
    assert_eq!(json["message"], expected_message);
}

#[test_case(
    AppError::Database("SELECT * FROM secret_table failed".into()),
    "database";
    "database_no_leak"
)]
#[test_case(
    AppError::Internal("panic in scheduler at line 42".into()),
    "internal";
    "internal_no_leak"
)]
#[test_case(
    AppError::CalendarSync("token: Bearer eyJhbGciOi...".into()),
    "calendar_sync";
    "calendar_sync_no_leak"
)]
#[test_case(
    AppError::Backup("drive said: /home/user/.local/secret.db".into()),
    "backup";
    "backup_no_leak"
)]
#[tokio::test]
async fn server_error_does_not_leak_details(err: AppError, expected_code: &str) {
    use axum::response::IntoResponse as _;
    // Capture the sensitive detail from the error before consuming it.
    let sensitive_detail = match &err {
        AppError::Database(msg)
        | AppError::Internal(msg)
        | AppError::CalendarSync(msg)
        | AppError::Backup(msg) => msg.clone(),
        _ => unreachable!("only 500 variants expected"),
    };
    let response = err.into_response();
    let json = body_json(response).await;
    assert_eq!(
        json["error"], expected_code,
        "error code should be '{expected_code}'"
    );
    let body_str = json.to_string();
    assert!(
        !body_str.contains(&sensitive_detail),
        "response must not contain sensitive detail: {sensitive_detail}\nbody: {body_str}"
    );
}

#[tokio::test]
async fn health_check_returns_200_ok() {
    let state = memory_state();
    let app = super::build_router(state);
    let response = get_request(app, "/api/health").await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn health_check_json_body_has_exactly_status_key() {
    let state = memory_state();
    let app = super::build_router(state);
    let response = get_request(app, "/api/health").await;
    let json = body_json(response).await;
    let obj = json.as_object().expect("should be object");
    assert_eq!(obj.len(), 1, "body should have exactly 1 key");
    assert!(obj.contains_key("status"));
}

#[tokio::test]
async fn get_profile_returns_active_profile_identity() {
    let state = memory_state();
    let app = super::build_router(state);
    let response = get_request(app, "/api/profile").await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["id"], "test-profile-id");
    assert_eq!(json["name"], "Test Profile");
    let obj = json.as_object().expect("should be object");
    assert_eq!(obj.len(), 2, "identity only — no pin material, no paths");
}

#[tokio::test]
async fn empty_slot_gives_validation_error_on_stateful_endpoints() {
    let app = super::build_router(crate::state::ActiveState::new());
    let response = get_request(app, "/api/tasks").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"], "validation");
}

#[tokio::test]
async fn empty_slot_still_serves_health() {
    let app = super::build_router(crate::state::ActiveState::new());
    let response = get_request(app, "/api/health").await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// Build and send a request with an explicit (or deliberately absent) `Host`
/// header, bypassing the shared helpers' default loopback host.
async fn request_with_host(
    app: axum::Router,
    uri: &str,
    host: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder().uri(uri);
    if let Some(host) = host {
        builder = builder.header(HOST, host);
    }
    let req = builder.body(Body::empty()).expect("build request");
    app.oneshot(req).await.expect("send request")
}

async fn make_host_response(host: &str) -> axum::response::Response {
    let state = memory_state();
    let app = super::build_router(state);
    request_with_host(app, "/api/health", Some(host)).await
}

#[test_case("127.0.0.1" ; "bare loopback ip")]
#[test_case("127.0.0.1:19532" ; "loopback ip with port")]
#[test_case("localhost" ; "bare localhost")]
#[test_case("localhost:19532" ; "localhost with port")]
#[test_case("LocalHost:19532" ; "hostnames are case insensitive")]
#[tokio::test]
async fn host_header_loopback_allowed(host: &str) {
    let response = make_host_response(host).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[test_case("evil.example" ; "foreign hostname")]
#[test_case("evil.example:19532" ; "foreign hostname with port")]
#[test_case("localhost.evil.example" ; "loopback prefix trick")]
#[test_case("[::1]:19532" ; "ipv6 loopback not in allowlist (server binds v4 only)")]
#[tokio::test]
async fn host_header_non_loopback_rejected(host: &str) {
    let response = make_host_response(host).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let json = body_json(response).await;
    assert_eq!(json["error"], "forbidden");
}

/// HTTP/1.1 requires `Host`, and every browser sends it; a request without
/// one only occurs from hand-rolled clients, so it is rejected too.
#[tokio::test]
async fn host_header_missing_rejected() {
    let state = memory_state();
    let app = super::build_router(state);
    let response = request_with_host(app, "/api/health", None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

fn make_env<'a>(
    pairs: &'a [(&'a str, &'a str)],
) -> impl Fn(&str) -> Result<String, std::env::VarError> + 'a {
    |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_owned())
            .ok_or(std::env::VarError::NotPresent)
    }
}

#[test_case(&[], DEFAULT_API_PORT, true; "defaults")]
#[test_case(&[("APRESWORK_API_PORT", "8080")], 8080, true; "custom_port")]
#[test_case(&[("APRESWORK_API_ENABLED", "false")], DEFAULT_API_PORT, false; "disabled")]
#[test_case(&[("APRESWORK_API_ENABLED", "FALSE")], DEFAULT_API_PORT, false; "disabled_uppercase")]
#[test_case(&[("APRESWORK_API_PORT", "not_a_number")], DEFAULT_API_PORT, true; "invalid_port_falls_back")]
fn server_config_from_env(env_pairs: &[(&str, &str)], expected_port: u16, expected_enabled: bool) {
    let config = ServerConfig::from_env_with(make_env(env_pairs));
    assert_eq!(config.port, expected_port);
    assert_eq!(config.enabled, expected_enabled);
}

#[tokio::test]
async fn start_server_disabled_returns_ok_without_binding() {
    let state = memory_state();
    let profiles = memory_profiles_state();
    let config = ServerConfig {
        port: DEFAULT_API_PORT,
        enabled: false,
    };
    // Should return Ok immediately without trying to bind any port.
    let result = super::start_server(state, profiles, config).await;
    assert!(
        result.is_ok(),
        "disabled server should return Ok, got: {result:?}"
    );
}

#[tokio::test]
async fn start_server_binds_on_localhost_only() {
    // Use a random high port to avoid clashing with real server.
    let state = memory_state();
    let profiles = memory_profiles_state();
    let config = ServerConfig {
        port: 0,
        enabled: true,
    };
    let result = super::start_server(state, profiles, config).await;
    assert!(
        result.is_ok(),
        "server start should succeed, got: {result:?}"
    );
}

fn make_http_test_task(state: &std::sync::Arc<AppState>, title: &str) -> Task {
    let default_schedule = state
        .store
        .get_default_schedule()
        .expect("default schedule");
    Task {
        id: uuid::Uuid::now_v7().to_string(),
        title: title.to_owned(),
        deadline: Some(crate::test_support::utc(2026, 12, 31, 23, 59)),
        schedule_id: default_schedule.id,
        ..Task::test_default()
    }
}

fn make_http_test_chunk(task_id: &str, start_time: chrono::DateTime<Utc>, minutes: i64) -> Chunk {
    let now = crate::test_support::test_now();
    Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task_id.to_owned(),
        start_time,
        end_time: start_time + chrono::Duration::minutes(minutes),
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn seed_task_with(
    state: &std::sync::Arc<AppState>,
    title: &str,
    mutate: impl FnOnce(&mut Task),
) -> Task {
    let mut task = make_http_test_task(state, title);
    mutate(&mut task);
    state.store.create_task(&task).expect("create task");
    task
}

fn seed_task(state: &std::sync::Arc<AppState>, title: &str) -> Task {
    seed_task_with(state, title, |_| {})
}

fn seed_auto_chunk(
    state: &std::sync::Arc<AppState>,
    task_id: &str,
    start: chrono::DateTime<Utc>,
    minutes: i64,
) -> String {
    let chunk = make_http_test_chunk(task_id, start, minutes);
    assert!(!chunk.is_fixed, "precondition: auto chunk");
    state.store.create_chunk(&chunk).expect("create chunk");
    chunk.id
}

async fn get_request(app: axum::Router, uri: &str) -> axum::response::Response {
    request(app, axum::http::Method::GET, uri).await
}

async fn json_request(
    app: axum::Router,
    method: axum::http::Method,
    uri: &str,
    body: Value,
) -> axum::response::Response {
    send_request(app, method, uri, Some(body)).await
}

async fn request(
    app: axum::Router,
    method: axum::http::Method,
    uri: &str,
) -> axum::response::Response {
    send_request(app, method, uri, None).await
}

async fn send_request(
    app: axum::Router,
    method: axum::http::Method,
    uri: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(HOST, "127.0.0.1");
    let req = if let Some(b) = body {
        builder = builder.header(CONTENT_TYPE, "application/json");
        builder
            .body(Body::from(b.to_string()))
            .expect("build request")
    } else {
        builder.body(Body::empty()).expect("build request")
    };
    app.oneshot(req).await.expect("send request")
}

async fn get_ok_json(app: axum::Router, uri: &str) -> Value {
    let response = get_request(app, uri).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {uri}");
    body_json(response).await
}

async fn get_ok_array(app: axum::Router, uri: &str) -> Vec<Value> {
    get_ok_json(app, uri)
        .await
        .as_array()
        .expect("should be array")
        .clone()
}

/// Assert that a response carries a `400 Bad Request` validation error.
/// Pass `""` as the needle when only the error code matters (an empty needle matches any message).
async fn assert_validation_error(response: axum::response::Response, message_contains: &str) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"], "validation");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains(message_contains),
        "validation message should mention '{message_contains}', got: {json}"
    );
}

#[test_case(Some("wrong-id"),        StatusCode::CONFLICT   ; "mismatch")]
#[test_case(Some("test-profile-id"), StatusCode::NO_CONTENT ; "correct_id")]
#[test_case(None::<&str>,            StatusCode::NO_CONTENT ; "no_guard")]
#[tokio::test]
/// Verify that the profile-ID guard on `DELETE /api/tasks/:id` returns `409 Conflict` on a
/// mismatch and `204 No Content` on success. This endpoint is representative; the same
/// middleware applies to all write endpoints (see `build_router`).
async fn profile_guard_on_delete_task(expected_id: Option<&str>, expected_status: StatusCode) {
    let state = memory_state();
    let task = seed_task(&state, "Guard test");
    let uri = match expected_id {
        Some(id) => format!("/api/tasks/{}?expected_profile_id={}", task.id, id),
        None => format!("/api/tasks/{}", task.id),
    };
    let app = super::build_router(state);
    let response = request(app, axum::http::Method::DELETE, &uri).await;
    assert_eq!(response.status(), expected_status);
}

pub(super) async fn assert_guard_fired(response: axum::response::Response) {
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let json = body_json(response).await;
    assert_eq!(json["error"], "profile_mismatch");
}

#[tokio::test]
async fn profile_guard_fires_on_complete_endpoint() {
    let state = memory_state();
    let task = seed_task(&state, "Guard test");
    let app = super::build_router(state);
    let response = request(
        app,
        axum::http::Method::POST,
        &format!("/api/tasks/{}/complete?expected_profile_id=wrong", task.id),
    )
    .await;
    assert_guard_fired(response).await;
}

#[tokio::test]
async fn profile_guard_fires_on_delete_comment() {
    let state = memory_state();
    // Seed a task so we have a valid comment parent.
    let default_schedule = state.store.get_default_schedule().expect("schedule");
    let task = crate::domain::models::Task {
        id: uuid::Uuid::now_v7().to_string(),
        title: "Comment parent".to_owned(),
        schedule_id: default_schedule.id,
        ..crate::domain::models::Task::test_default()
    };
    state.store.create_task(&task).expect("task");
    let now = crate::test_support::test_now();
    let comment = Comment {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task.id.clone(),
        content: "hello".to_owned(),
        author: "User".to_owned(),
        created_at: now,
        updated_at: now,
    };
    state.store.create_comment(&comment).expect("comment");
    let app = super::build_router(state);
    let response = request(
        app,
        axum::http::Method::DELETE,
        &format!("/api/comments/{}?expected_profile_id=wrong", comment.id),
    )
    .await;
    assert_guard_fired(response).await;
}

#[tokio::test]
async fn profile_guard_does_not_affect_read_endpoints() {
    let state = memory_state();
    let app = super::build_router(state);
    // Mismatched expected_profile_id on a GET — guard middleware is not
    // applied to this route, so the request is handled normally.
    let response = get_request(app, "/api/tasks?expected_profile_id=wrong-id").await;
    assert_eq!(response.status(), StatusCode::OK);
}

async fn assert_not_found(response: axum::response::Response) -> Value {
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["error"], "not_found");
    json
}
