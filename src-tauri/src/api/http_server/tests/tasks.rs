// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Task endpoint tests: create / complete / list (+ filters) / get / update /
//! delete, the labels endpoint, plus the `query_to_filter` unit tests.

use axum::http::StatusCode;
use test_case::test_case;

use super::{
    assert_guard_fired, assert_not_found, assert_validation_error, body_json, get_ok_array,
    get_ok_json, get_request, json_request, memory_state, request, seed_auto_chunk, seed_task,
    seed_task_with,
};
use crate::api::http_server::{build_router, query_to_filter, AppError, TaskListQuery};
use crate::domain::enums::{Priority, TaskStatus};

async fn assert_single_task_filter(
    state: std::sync::Arc<crate::state::AppState>,
    query: &str,
    expected_title: &str,
) {
    let app = build_router(state);
    let arr = get_ok_array(app, &format!("/api/tasks?{query}")).await;
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], expected_title);
}

async fn patch_task(
    app: axum::Router,
    task_id: &str,
    body: serde_json::Value,
) -> axum::response::Response {
    json_request(
        app,
        axum::http::Method::PATCH,
        &format!("/api/tasks/{task_id}"),
        body,
    )
    .await
}

async fn delete_task_expect_204(app: axum::Router, task_id: &str) -> axum::Router {
    let response = request(
        app.clone(),
        axum::http::Method::DELETE,
        &format!("/api/tasks/{task_id}"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    app
}

/// The guard middleware applies to: PATCH /api/tasks/{id}, POST /api/tasks/{id}/comments,
/// PATCH /api/comments/{id}, and POST /api/chunks/{id}/move.
#[test_case(Some("wrong-id"),        StatusCode::CONFLICT ; "mismatch")]
#[test_case(Some("test-profile-id"), StatusCode::CREATED  ; "correct_id")]
#[test_case(None::<&str>,            StatusCode::CREATED  ; "no_guard")]
#[tokio::test]
async fn profile_guard_on_create_task(expected_id: Option<&str>, expected_status: StatusCode) {
    let state = memory_state();
    let uri = match expected_id {
        Some(id) => format!("/api/tasks?expected_profile_id={id}"),
        None => "/api/tasks".to_owned(),
    };
    let app = build_router(state);
    let response = json_request(
        app,
        axum::http::Method::POST,
        &uri,
        serde_json::json!({
            "title": "Guard test",
            "duration_minutes": 60,
            "deadline": "2026-12-31T23:59:59Z"
        }),
    )
    .await;
    assert_eq!(response.status(), expected_status);
}

#[tokio::test]
async fn profile_guard_fires_on_update_task() {
    let state = memory_state();
    let task = seed_task(&state, "Guard test");
    let uri = format!("/api/tasks/{}?expected_profile_id=wrong", task.id);
    let app = build_router(state);
    let response = json_request(
        app,
        axum::http::Method::PATCH,
        &uri,
        serde_json::json!({ "title": "New title" }),
    )
    .await;
    assert_guard_fired(response).await;
}

#[tokio::test]
async fn create_task_creates_task_with_defaults() {
    let state = memory_state();
    let default_schedule = state
        .store
        .get_default_schedule()
        .expect("default schedule");

    let app = build_router(state);
    let response = json_request(
        app,
        axum::http::Method::POST,
        "/api/tasks",
        serde_json::json!({
            "title": "Imported task",
            "duration_minutes": 60,
            "deadline": "2026-12-31T23:59:59Z"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response).await;
    assert_eq!(json["title"], "Imported task");
    assert_eq!(json["priority"], "Medium");
    assert_eq!(json["status"], "pending");
    assert_eq!(json["schedule_id"], default_schedule.id);
}

#[tokio::test]
async fn create_task_accepts_labels_and_backlog_status() {
    let state = memory_state();
    let app = build_router(state);
    let response = json_request(
        app,
        axum::http::Method::POST,
        "/api/tasks",
        serde_json::json!({
            "title": "Backlog import task",
            "description": "Seeded from progress import",
            "duration_minutes": 30,
            "deadline": "2026-12-31T23:59:59Z",
            "labels": ["agent", "phase-3a"],
            "status": "backlog"
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response).await;
    assert_eq!(json["status"], "backlog");
    assert_eq!(json["labels"], serde_json::json!(["agent", "phase-3a"]));
    assert_eq!(json["description"], "Seeded from progress import");
}

#[tokio::test]
async fn create_task_invalid_input_returns_400() {
    let state = memory_state();
    let app = build_router(state);
    let response = json_request(
        app,
        axum::http::Method::POST,
        "/api/tasks",
        serde_json::json!({
            "title": "Bad task",
            "duration_minutes": 0,
            "deadline": "2026-12-31T23:59:59Z"
        }),
    )
    .await;
    assert_validation_error(response, "duration_minutes").await;
}

#[tokio::test]
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
async fn complete_task_endpoint_collapses_scheduled_chunks() {
    let state = memory_state();
    let task = seed_task_with(&state, "Complete me", |task| {
        task.status = TaskStatus::Scheduled;
        task.duration_minutes = 90;
        task.time_logged_minutes = 10;
    });

    let start1 = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 3, 28, 9, 0, 0).unwrap();
    let start2 = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 3, 28, 10, 0, 0).unwrap();
    let chunk1 = crate::domain::models::Chunk {
        id: "chunk-http-1".into(),
        task_id: task.id.clone(),
        start_time: start1,
        end_time: start1 + chrono::Duration::minutes(30),
        status: crate::domain::enums::ChunkStatus::Scheduled,
        is_fixed: true,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: start1,
        updated_at: start1,
    };
    let chunk2 = crate::domain::models::Chunk {
        id: "chunk-http-2".into(),
        task_id: task.id.clone(),
        start_time: start2,
        end_time: start2 + chrono::Duration::minutes(45),
        status: crate::domain::enums::ChunkStatus::Scheduled,
        is_fixed: true,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: start2,
        updated_at: start2,
    };
    state.store.create_chunk(&chunk1).expect("create chunk1");
    state.store.create_chunk(&chunk2).expect("create chunk2");

    let app = build_router(state.clone());
    let response = request(
        app,
        axum::http::Method::POST,
        &format!("/api/tasks/{}/complete", task.id),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["status"], "completed");
    assert_eq!(json["time_logged_minutes"], 90);

    let survivor = state
        .store
        .get_chunk("chunk-http-1")
        .expect("load chunk1")
        .expect("chunk1 exists");
    assert_eq!(
        survivor.status,
        crate::domain::enums::ChunkStatus::Completed
    );
    assert_eq!(survivor.logged_minutes, Some(80));
    assert!(
        state
            .store
            .get_chunk("chunk-http-2")
            .expect("load chunk2")
            .is_none(),
        "later scheduled chunks should be deleted on collapse"
    );
}

#[tokio::test]
async fn list_tasks_returns_empty_array_when_no_tasks() {
    let state = memory_state();
    let app = build_router(state);
    let json = get_ok_json(app, "/api/tasks").await;
    assert_eq!(json, serde_json::json!([]));
}

#[tokio::test]
async fn list_tasks_returns_all_tasks_when_no_filter() {
    let state = memory_state();
    seed_task(&state, "Task Alpha");
    seed_task(&state, "Task Beta");

    let app = build_router(state);
    let arr = get_ok_array(app, "/api/tasks").await;
    assert_eq!(arr.len(), 2);
}

#[test_case("pending", TaskStatus::Pending  ; "filter_by_pending")]
#[test_case("backlog", TaskStatus::Backlog   ; "filter_by_backlog")]
#[test_case("completed", TaskStatus::Completed ; "filter_by_completed")]
#[tokio::test]
async fn list_tasks_filters_by_single_status(status_str: &str, expected_status: TaskStatus) {
    let state = memory_state();
    seed_task_with(&state, "Matching task", |task| {
        task.status = expected_status;
    });
    seed_task_with(&state, "Other task", |task| {
        task.status = TaskStatus::Cancelled;
    });

    let app = build_router(state);
    let arr = get_ok_array(app, &format!("/api/tasks?status={status_str}")).await;
    assert_eq!(arr.len(), 1, "expected exactly one {status_str} task");
    assert_eq!(arr[0]["status"], status_str);
}

#[tokio::test]
async fn list_tasks_filters_by_comma_separated_statuses() {
    let state = memory_state();
    seed_task_with(&state, "Pending task", |task| {
        task.status = TaskStatus::Pending;
    });
    seed_task_with(&state, "Scheduled task", |task| {
        task.status = TaskStatus::Scheduled;
    });
    seed_task_with(&state, "Completed task", |task| {
        task.status = TaskStatus::Completed;
    });

    let app = build_router(state);
    let arr = get_ok_array(app, "/api/tasks?status=pending,scheduled").await;
    assert_eq!(arr.len(), 2, "should return 2 tasks (pending + scheduled)");
    let statuses: Vec<&str> = arr
        .iter()
        .map(|t| t["status"].as_str().expect("status string"))
        .collect();
    assert!(statuses.contains(&"pending"));
    assert!(statuses.contains(&"scheduled"));
}

#[tokio::test]
async fn list_tasks_filters_by_statuses_alias() {
    let state = memory_state();
    seed_task_with(&state, "Scheduled task", |task| {
        task.status = TaskStatus::Scheduled;
    });
    seed_task(&state, "Pending task");

    let app = build_router(state);
    let arr = get_ok_array(app, "/api/tasks?statuses=scheduled").await;
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], "Scheduled task");
    assert_eq!(arr[0]["status"], "scheduled");
}

#[tokio::test]
async fn list_tasks_filters_by_priority() {
    let state = memory_state();
    seed_task_with(&state, "High priority task", |task| {
        task.priority = Priority::High;
    });
    seed_task_with(&state, "Low priority task", |task| {
        task.priority = Priority::Low;
    });

    let app = build_router(state);
    let arr = get_ok_array(app, "/api/tasks?priority=High").await;
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], "High priority task");
    assert_eq!(arr[0]["priority"], "High");
}

#[tokio::test]
async fn list_tasks_filters_by_multiple_priorities_via_alias() {
    let state = memory_state();
    seed_task_with(&state, "High priority task", |task| {
        task.priority = Priority::High;
    });
    seed_task_with(&state, "Critical priority task", |task| {
        task.priority = Priority::Critical;
    });
    seed_task_with(&state, "Low priority task", |task| {
        task.priority = Priority::Low;
    });

    let app = build_router(state);
    let arr = get_ok_array(app, "/api/tasks?priorities=High,Critical").await;
    assert_eq!(arr.len(), 2);
    assert!(arr.iter().all(|t| t["priority"] != "Low"));
}

#[tokio::test]
async fn list_tasks_filters_by_search_text_across_title_and_description() {
    let state = memory_state();
    seed_task(&state, "Write meeting notes");
    seed_task_with(&state, "Write docs", |task| {
        task.description = Some("follow-up from design meeting".to_owned());
    });
    seed_task(&state, "Do laundry");

    let app = build_router(state);
    let arr = get_ok_array(app, "/api/tasks?search_text=MEETING").await;
    assert_eq!(arr.len(), 2);
    let titles: Vec<&str> = arr
        .iter()
        .map(|task| task["title"].as_str().expect("title string"))
        .collect();
    assert!(titles.contains(&"Write meeting notes"));
    assert!(titles.contains(&"Write docs"));
}

#[test_case("labels=agent", "Agent task" ; "included_label_keeps_carrier")]
#[test_case("excluded_labels=agent", "No label task" ; "excluded_label_removes_carrier")]
#[test_case("unlabeled=true", "No label task" ; "unlabeled_keeps_only_unlabeled")]
#[tokio::test]
async fn list_tasks_single_label_filter(query: &str, expected_title: &str) {
    let state = memory_state();
    seed_task_with(&state, "Agent task", |task| {
        task.labels = vec!["agent".to_owned()];
    });
    seed_task(&state, "No label task");
    assert_single_task_filter(state, query, expected_title).await;
}

#[test_case("labels=agent,work", "Agent work task" ; "match_all_requires_both_labels")]
#[test_case("labels=agent&excluded_labels=work", "Agent task" ; "included_minus_excluded")]
#[tokio::test]
async fn list_tasks_label_combination(query: &str, expected_title: &str) {
    let state = memory_state();
    seed_task_with(&state, "Agent work task", |task| {
        task.labels = vec!["agent".to_owned(), "work".to_owned()];
    });
    seed_task_with(&state, "Agent task", |task| {
        task.labels = vec!["agent".to_owned()];
    });
    seed_task_with(&state, "Work task", |task| {
        task.labels = vec!["work".to_owned()];
    });
    assert_single_task_filter(state, query, expected_title).await;
}

#[test_case("/api/tasks?unlabeled=yes"      ; "invalid_unlabeled")]
#[test_case("/api/tasks?status=bogus_status" ; "invalid_status")]
#[test_case("/api/tasks?priority=SuperHigh"  ; "invalid_priority")]
#[tokio::test]
async fn list_tasks_invalid_query_returns_400(uri: &str) {
    let state = memory_state();
    let app = build_router(state);
    let response = get_request(app, uri).await;
    assert_validation_error(response, "").await;
}

#[tokio::test]
async fn get_task_returns_200_with_task_when_found() {
    let state = memory_state();
    let task = seed_task(&state, "Find me");
    let app = build_router(state);
    let json = get_ok_json(app, &format!("/api/tasks/{}", task.id)).await;
    assert_eq!(json["id"], task.id);
    assert_eq!(json["title"], "Find me");
}

#[tokio::test]
async fn get_task_returns_404_when_not_found() {
    let state = memory_state();
    let app = build_router(state);
    let response = get_request(app, "/api/tasks/nonexistent-id").await;
    let json = assert_not_found(response).await;
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("nonexistent-id"));
}

#[test_case("title",       "Updated title"       ; "updates_title")]
#[test_case("description", "Updated description" ; "updates_description")]
#[tokio::test]
async fn update_task_updates_field(field: &str, expected_value: &str) {
    let state = memory_state();
    let task = if field == "description" {
        seed_task_with(&state, "Task with description", |t| {
            t.description = Some("Old description".to_owned());
        })
    } else {
        seed_task(&state, "Original title")
    };
    let app = build_router(state);
    let mut patch_map = serde_json::Map::new();
    patch_map.insert(
        field.to_owned(),
        serde_json::Value::String(expected_value.to_owned()),
    );
    let response = patch_task(app, &task.id, serde_json::Value::Object(patch_map)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["id"], task.id);
    assert_eq!(json[field], expected_value);
}

#[tokio::test]
async fn update_task_backlog_transition_removes_auto_chunk() {
    let state = memory_state();
    let task = seed_task_with(&state, "Scheduled task", |task| {
        task.status = TaskStatus::Scheduled;
    });
    let start = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 3, 28, 9, 0, 0).unwrap();
    let chunk_id = seed_auto_chunk(&state, &task.id, start, 30);

    let app = build_router(state.clone());
    let response = patch_task(app, &task.id, serde_json::json!({ "status": "backlog" })).await;

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["status"], "backlog");

    assert!(
        state
            .store
            .get_chunk(&chunk_id)
            .expect("get chunk")
            .is_none(),
        "auto chunk should be deleted by the backlog transition"
    );
}

#[tokio::test]
async fn update_task_returns_404_when_not_found() {
    let state = memory_state();
    let app = build_router(state);
    let response = patch_task(
        app,
        "nonexistent-id",
        serde_json::json!({ "title": "Updated title" }),
    )
    .await;
    assert_not_found(response).await;
}

#[tokio::test]
async fn update_task_invalid_input_returns_400() {
    let state = memory_state();
    let task = seed_task(&state, "Task to validate");
    let app = build_router(state);
    let response = patch_task(app, &task.id, serde_json::json!({ "duration_minutes": 0 })).await;
    assert_validation_error(response, "duration_minutes").await;
}

#[tokio::test]
async fn delete_task_returns_204_and_removes_task() {
    let state = memory_state();
    let task = seed_task(&state, "Delete me");
    let app = delete_task_expect_204(build_router(state), &task.id).await;
    let response = get_request(app, &format!("/api/tasks/{}", task.id)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_task_returns_404_when_not_found() {
    let state = memory_state();
    let app = build_router(state);
    let response = request(app, axum::http::Method::DELETE, "/api/tasks/nonexistent-id").await;
    assert_not_found(response).await;
}

/// Deleting a recurring instance cancels it instead of removing the row, so
/// its cadence slot stays occupied (same policy as the Tauri command).
#[tokio::test]
async fn delete_recurring_instance_cancels_instead_of_deleting() {
    let state = memory_state();
    let default_schedule = state
        .store
        .get_default_schedule()
        .expect("default schedule");
    let template = crate::domain::models::RecurringTemplate {
        id: "tmpl-http-1".to_owned(),
        schedule_id: default_schedule.id,
        ..crate::domain::models::RecurringTemplate::test_default()
    };
    state
        .store
        .create_template(&template)
        .expect("create template");

    let task = seed_task_with(&state, "Recurring instance", |task| {
        task.recurring_template_id = Some(template.id);
    });
    let app = delete_task_expect_204(build_router(state), &task.id).await;
    let json = get_ok_json(app, &format!("/api/tasks/{}", task.id)).await;
    assert_eq!(json["status"], "cancelled");
}

#[tokio::test]
async fn list_labels_returns_empty_array_when_no_labels() {
    let state = memory_state();
    let app = build_router(state);
    let json = get_ok_json(app, "/api/labels").await;
    assert_eq!(json, serde_json::json!([]));
}

#[tokio::test]
async fn list_labels_returns_distinct_labels_with_task_counts() {
    let state = memory_state();
    seed_task_with(&state, "Task A", |task| {
        task.labels = vec!["deep".to_owned(), "agent".to_owned()];
    });
    seed_task_with(&state, "Task B", |task| {
        task.labels = vec!["agent".to_owned()];
    });

    let app = build_router(state);
    let json = get_ok_json(app, "/api/labels").await;
    assert_eq!(
        json,
        serde_json::json!([
            { "label": "agent", "task_count": 2 },
            { "label": "deep", "task_count": 1 }
        ])
    );
}

fn assert_query_validation_error(q: TaskListQuery) {
    let err = query_to_filter(q).expect_err("should fail");
    assert!(matches!(err, AppError::Validation(_)));
}

#[test]
fn query_to_filter_empty_query_gives_default_filter() {
    let q = TaskListQuery::default();
    let filter = query_to_filter(q).expect("should succeed");
    assert!(filter.statuses.is_none());
    assert!(filter.priorities.is_none());
    assert!(filter.labels.is_none());
    assert!(filter.excluded_labels.is_none());
    assert!(filter.unlabeled.is_none());
    assert!(filter.search_text.is_none());
}

#[test_case("pending",   TaskStatus::Pending   ; "parse_pending")]
#[test_case("backlog",   TaskStatus::Backlog   ; "parse_backlog")]
#[test_case("scheduled", TaskStatus::Scheduled ; "parse_scheduled")]
#[test_case("completed", TaskStatus::Completed ; "parse_completed")]
#[test_case("cancelled", TaskStatus::Cancelled ; "parse_cancelled")]
fn query_to_filter_parses_single_status(raw: &str, expected: TaskStatus) {
    let q = TaskListQuery {
        status: Some(raw.to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should parse");
    let statuses = filter.statuses.expect("statuses should be Some");
    assert_eq!(statuses, vec![expected]);
}

#[test_case("Low",      Priority::Low      ; "parse_low")]
#[test_case("Medium",   Priority::Medium   ; "parse_medium")]
#[test_case("High",     Priority::High     ; "parse_high")]
#[test_case("Critical", Priority::Critical ; "parse_critical")]
fn query_to_filter_parses_priority(raw: &str, expected: Priority) {
    let q = TaskListQuery {
        priority: Some(raw.to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should parse");
    assert_eq!(filter.priorities, Some(vec![expected]));
}

#[test]
fn query_to_filter_parses_multiple_priorities() {
    let q = TaskListQuery {
        priority: Some("High,Critical".to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should parse");
    let priorities = filter.priorities.expect("priorities should be Some");
    assert_eq!(priorities.len(), 2);
    assert!(priorities.contains(&Priority::High));
    assert!(priorities.contains(&Priority::Critical));
}

#[test]
fn query_to_filter_parses_multiple_statuses() {
    let q = TaskListQuery {
        status: Some("pending,scheduled".to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should parse");
    let statuses = filter.statuses.expect("statuses should be Some");
    assert_eq!(statuses.len(), 2);
    assert!(statuses.contains(&TaskStatus::Pending));
    assert!(statuses.contains(&TaskStatus::Scheduled));
}

#[test_case(false, "agent,work"  ; "parses_multiple_labels")]
#[test_case(true,  "agent, work" ; "parses_multiple_excluded_labels")]
fn query_to_filter_parses_label_or_excluded_label(is_excluded: bool, raw: &str) {
    let q = if is_excluded {
        TaskListQuery {
            excluded_labels: Some(raw.to_owned()),
            ..TaskListQuery::default()
        }
    } else {
        TaskListQuery {
            labels: Some(raw.to_owned()),
            ..TaskListQuery::default()
        }
    };
    let filter = query_to_filter(q).expect("should parse");
    let parsed = if is_excluded {
        filter.excluded_labels
    } else {
        filter.labels
    };
    assert_eq!(parsed, Some(vec!["agent".to_owned(), "work".to_owned()]));
}

#[test_case(TaskListQuery { status: Some("garbage".to_owned()),   ..TaskListQuery::default() } ; "invalid_status")]
#[test_case(TaskListQuery { priority: Some("UltraHigh".to_owned()), ..TaskListQuery::default() } ; "invalid_priority")]
fn query_to_filter_invalid_enum_returns_validation_error(q: TaskListQuery) {
    assert_query_validation_error(q);
}

#[test]
fn query_to_filter_empty_labels_string_gives_none() {
    let q = TaskListQuery {
        labels: Some(String::new()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should succeed");
    assert!(filter.labels.is_none());
}

#[test]
fn query_to_filter_empty_excluded_labels_string_gives_none() {
    let q = TaskListQuery {
        excluded_labels: Some(",,".to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should succeed");
    assert!(filter.excluded_labels.is_none());
}

#[test_case("true", true; "parses true")]
#[test_case("false", false; "parses false")]
#[test_case(" true ", true; "trims whitespace")]
fn query_to_filter_parses_unlabeled(raw: &str, expected: bool) {
    let q = TaskListQuery {
        unlabeled: Some(raw.to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should parse");
    assert_eq!(filter.unlabeled, Some(expected));
}

#[test]
fn query_to_filter_invalid_unlabeled_returns_validation_error() {
    assert_query_validation_error(TaskListQuery {
        unlabeled: Some("maybe".to_owned()),
        ..TaskListQuery::default()
    });
}

#[test]
fn query_to_filter_passes_search_text_through() {
    let q = TaskListQuery {
        search_text: Some("meeting".to_owned()),
        ..TaskListQuery::default()
    };
    let filter = query_to_filter(q).expect("should succeed");
    assert_eq!(filter.search_text, Some("meeting".to_owned()));
}
