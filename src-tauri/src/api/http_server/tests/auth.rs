// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for auth + calendar-sync REST endpoints:
//! POST /api/auth/google/begin, GET /api/auth/google/status,
//! POST /api/auth/google/disconnect, POST /api/calendar/pull,
//! POST /api/sync/now, GET /api/sync/status.

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{Method, StatusCode};
use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use crate::api::http_server::build_router;
use crate::domain::models::ExternalEventRecord;
use crate::error::AppError;
use crate::state::AppState;
use crate::test_support::calendar::{make_event, MockCalendarSync};
use crate::traits::calendar_sync::AuthStatus;
use crate::traits::storage::Store;

use super::{body_json, memory_state_with_sync, request};

/// Seeds one `external_events` row for `cal-a` spanning `now..now+1h`
/// (`id`/`event_id` derived from `key`, given `title`), via the shared ±window
/// replace. Shared by the disconnect tests.
fn seed_external_event(state: &Arc<AppState>, now: DateTime<Utc>, key: &str, title: &str) {
    let seeded = ExternalEventRecord {
        id: format!("ev-{key}"),
        calendar_id: "cal-a".to_owned(),
        event_id: format!("gcal-ev-{key}"),
        title: title.to_owned(),
        description: None,
        start_time: now,
        end_time: now + Duration::hours(1),
        busy: true,
        declined: false,
        all_day: false,
        updated_at: now,
    };
    state
        .store
        .replace_external_events_in_window(
            "cal-a",
            now - Duration::hours(1),
            now + Duration::hours(2),
            &[seeded],
        )
        .expect("seed external_events");
}

/// Builds state whose `MockCalendarSync` returns one busy `cal-a` event at
/// `now`, with `pull_calendar_ids=["cal-a"]` configured. Shared by the
/// pull/sync happy-path tests.
fn state_with_one_cal_event(now: DateTime<Utc>) -> Arc<AppState> {
    let mut events = HashMap::new();
    events.insert(
        "cal-a".to_owned(),
        vec![make_event("cal-a", "ev-a1", "Meeting", true, now)],
    );
    let state = memory_state_with_sync(Arc::new(MockCalendarSync::new(true, events)));
    state
        .store
        .set_config_value("pull_calendar_ids", r#"["cal-a"]"#)
        .expect("set cal ids");
    state
}

/// Asserts a sanitized `500 calendar_sync` response whose body does NOT contain
/// `forbidden` (a provider-internal detail that must never leak).
async fn assert_sanitized_calendar_sync_500(response: axum::response::Response, forbidden: &str) {
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let json = body_json(response).await;
    assert_eq!(json["error"], "calendar_sync");
    let body_str = json.to_string();
    assert!(
        !body_str.contains(forbidden),
        "response must not leak provider error detail; body: {body_str}"
    );
}

// ── POST /api/auth/google/begin ───────────────────────────────────────────

#[tokio::test]
async fn begin_with_configured_url_returns_200_and_url() {
    let mock =
        MockCalendarSync::new(false, HashMap::new()).with_begin_url("https://consent.example/x");
    let state = memory_state_with_sync(Arc::new(mock));
    let app = build_router(state);

    let response = request(app, Method::POST, "/api/auth/google/begin").await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["url"], "https://consent.example/x");
}

#[tokio::test]
async fn begin_with_noop_sync_returns_500_calendar_sync_sanitized() {
    let state = memory_state_with_sync(Arc::new(crate::calendar::noop::NoopCalendarSync));
    let app = build_router(state);

    let response = request(app, Method::POST, "/api/auth/google/begin").await;

    assert_sanitized_calendar_sync_500(response, "no calendar provider").await;
}

// ── GET /api/auth/google/status ───────────────────────────────────────────

#[test_case(
    AuthStatus::NotConnected,
    r#"{"type":"not_connected"}"#;
    "not_connected"
)]
#[test_case(
    AuthStatus::Pending,
    r#"{"type":"pending"}"#;
    "pending"
)]
#[test_case(
    AuthStatus::Connected { email: Some("user@example.com".into()) },
    r#"{"type":"connected","email":"user@example.com"}"#;
    "connected_with_email"
)]
#[tokio::test]
// test_case passes AuthStatus by value; no ref needed in the signature.
#[allow(clippy::needless_pass_by_value)]
async fn auth_status_returns_correct_variant(status: AuthStatus, expected_json: &str) {
    let mock = MockCalendarSync::new(false, HashMap::new()).with_status(status);
    let state = memory_state_with_sync(Arc::new(mock));
    let app = build_router(state);

    let response = request(app, Method::GET, "/api/auth/google/status").await;

    assert_eq!(response.status(), StatusCode::OK);
    let got = body_json(response).await;
    let expected: serde_json::Value =
        serde_json::from_str(expected_json).expect("parse expected json");
    assert_eq!(got, expected);
}

// ── POST /api/auth/google/disconnect ─────────────────────────────────────

#[test_case(false, StatusCode::NO_CONTENT, 0; "happy_path")]
#[test_case(true, StatusCode::INTERNAL_SERVER_ERROR, 1; "provider_error")]
#[tokio::test]
async fn disconnect_transitions_state(
    with_error: bool,
    expected_status: StatusCode,
    expected_row_count: usize,
) {
    let mock = if with_error {
        MockCalendarSync::new(false, HashMap::new()).with_disconnect_error()
    } else {
        MockCalendarSync::new(false, HashMap::new())
    };
    let state = memory_state_with_sync(Arc::new(mock));
    let store = state.store.clone();
    let now = crate::test_support::test_now();
    seed_external_event(&state, now, "test", "Test event");

    let app = build_router(state);
    let response = request(app, Method::POST, "/api/auth/google/disconnect").await;

    assert_eq!(response.status(), expected_status);
    if with_error {
        let json = body_json(response).await;
        assert_eq!(json["error"], "calendar_sync");
    }

    let remaining = store
        .get_external_events_in_range(now - Duration::hours(2), now + Duration::hours(3))
        .expect("get events");
    assert_eq!(remaining.len(), expected_row_count);
}

// ── POST /api/calendar/pull ───────────────────────────────────────────────

/// POST /api/calendar/pull against `state`, asserting `200 OK`. Returns the
/// store (cloned before the router consumes `state`) and the raw response
/// for the caller to inspect further.
async fn pull_ok(state: Arc<AppState>) -> (Arc<dyn Store + Send + Sync>, axum::response::Response) {
    let store = state.store.clone();
    let app = build_router(state);
    let response = request(app, Method::POST, "/api/calendar/pull").await;
    assert_eq!(response.status(), StatusCode::OK);
    (store, response)
}

#[tokio::test]
async fn pull_happy_path_returns_200_with_schedule_result_and_mirrors_events() {
    let now = crate::test_support::test_now();
    let state = state_with_one_cal_event(now);
    let (store, response) = pull_ok(state).await;

    let json = body_json(response).await;
    assert!(
        json.get("placed_chunks").is_some(),
        "response must have placed_chunks key; got: {json}"
    );
    assert!(
        json.get("warnings").is_some(),
        "response must have warnings key; got: {json}"
    );

    let mirrored = store
        .get_external_events_in_range(now - Duration::days(8), now + Duration::days(31))
        .expect("get events");
    assert_eq!(mirrored.len(), 1, "mirrored event must be in the store");
    assert_eq!(mirrored[0].event_id, "ev-a1");
}

#[tokio::test]
async fn pull_unavailable_mock_returns_200_no_external_events() {
    // Provider unavailable: pull no-ops, reschedule still runs.
    let mock = MockCalendarSync::new(false, HashMap::new());
    let state = memory_state_with_sync(Arc::new(mock));
    let (store, _response) = pull_ok(state).await;

    let now = crate::test_support::test_now();
    let events = store
        .get_external_events_in_range(now - Duration::days(8), now + Duration::days(31))
        .expect("get events");
    assert!(
        events.is_empty(),
        "no external events expected when provider unavailable"
    );
}

// ── POST /api/sync/now + GET /api/sync/status ─────────────────────────────

#[tokio::test]
async fn sync_now_happy_path_returns_200_and_records_bookkeeping() {
    let now = crate::test_support::test_now();
    let state = state_with_one_cal_event(now);

    let app = build_router(state);
    let response = request(app.clone(), Method::POST, "/api/sync/now").await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert!(
        json["schedule"].get("placed_chunks").is_some(),
        "response must have schedule.placed_chunks key; got: {json}"
    );
    assert!(
        json["schedule"].get("warnings").is_some(),
        "response must have schedule.warnings key; got: {json}"
    );
    assert!(
        json["pushed"].get("created").is_some()
            && json["pushed"].get("updated").is_some()
            && json["pushed"].get("deleted").is_some(),
        "response must have pushed created/updated/deleted counts; got: {json}"
    );

    let response = request(app, Method::GET, "/api/sync/status").await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert!(
        json["last_sync_at"].is_string(),
        "last_sync_at must be recorded; got: {json}"
    );
    assert!(
        json["last_sync_error"].is_null(),
        "last_sync_error must be null after success; got: {json}"
    );
}

#[tokio::test]
async fn sync_now_provider_error_returns_500_calendar_sync_sanitized() {
    let mock = MockCalendarSync::new(true, HashMap::new()).with_ensure_calendar_error();
    let state = memory_state_with_sync(Arc::new(mock));

    let app = build_router(state);
    let response = request(app.clone(), Method::POST, "/api/sync/now").await;

    assert_sanitized_calendar_sync_500(response, "ensure_app_calendar failed").await;

    let response = request(app, Method::GET, "/api/sync/status").await;
    let json = body_json(response).await;
    assert!(
        json["last_sync_at"].is_null(),
        "no success timestamp on failure; got: {json}"
    );
    assert!(
        json["last_sync_error"].is_string(),
        "last_sync_error must be recorded; got: {json}"
    );
}

#[tokio::test]
async fn sync_status_fresh_store_returns_nulls() {
    let mock = MockCalendarSync::new(false, HashMap::new());
    let state = memory_state_with_sync(Arc::new(mock));
    let app = build_router(state);

    let response = request(app, Method::GET, "/api/sync/status").await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(
        json,
        serde_json::json!({ "last_sync_at": null, "last_sync_error": null })
    );
}

#[tokio::test]
async fn pull_list_events_error_returns_500_calendar_sync_sanitized() {
    let mut events = HashMap::new();
    // cal-a is listed in pull ids but its list_events will error.
    events.insert("cal-a".to_owned(), vec![]);
    let mock = MockCalendarSync::new(true, events).with_calendar_error(
        "cal-a",
        AppError::CalendarSync("simulated network timeout".into()),
    );
    let state = memory_state_with_sync(Arc::new(mock));

    state
        .store
        .set_config_value("pull_calendar_ids", r#"["cal-a"]"#)
        .expect("set cal ids");

    let app = build_router(state);
    let response = request(app, Method::POST, "/api/calendar/pull").await;

    assert_sanitized_calendar_sync_500(response, "simulated network timeout").await;
}
