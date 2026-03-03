// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `calendar::google_http` write endpoints
//! (`create_calendar`, `list_app_calendar_events`, `delete_event`, and the
//! user-event writes). Chunk push goes through the batch encoder, covered in
//! `batch_tests`.
//!
//! All endpoints exercised here are local 127.0.0.1 mocks — no request ever
//! leaves the machine.

use chrono::{TimeZone as _, Utc};
use test_case::test_case;

use super::test_support::{
    assert_cud_batch_single_post, chunk_payload, cud_batch_env, cud_ops, instant_backoff,
    provider_env, test_now, week_window,
};
use super::{
    create_calendar, create_user_event, delete_event, list_app_calendar_events, update_user_event,
};
use crate::calendar::google::GoogleCalendarSync;
use crate::error::AppError;
use crate::traits::calendar_sync::{
    CalendarSync, ChunkEventPayload, RemoteChunkEvent, UserEventPayload,
};

fn payload() -> ChunkEventPayload {
    chunk_payload("chunk-42", "Part 1 of 2 — Après Work\n\nLeg day")
}

fn user_payload() -> UserEventPayload {
    UserEventPayload {
        title: "Dentist".to_owned(),
        description: Some("Cleaning".to_owned()),
        start: Utc
            .with_ymd_and_hms(2026, 7, 20, 9, 0, 0)
            .single()
            .expect("start"),
        end: Utc
            .with_ymd_and_hms(2026, 7, 20, 10, 0, 0)
            .single()
            .expect("end"),
        all_day: false,
    }
}

/// List the marked events over the shared [`week_window`] against `provider`,
/// expecting success. The caller joins the mock handle for request assertions.
fn list_week_events(provider: &GoogleCalendarSync) -> Vec<RemoteChunkEvent> {
    let (start, end) = week_window();
    list_app_calendar_events(
        provider,
        &instant_backoff(),
        test_now(),
        "cal-1",
        start,
        end,
    )
    .expect("list_app_calendar_events")
}

/// Join the mock server `handle`, then assert `err` is `AppError::CalendarSync`
/// with a message containing `expected_substring`. Shared by the
/// `delete_event`/`create_user_event`/`update_user_event` negative-path tests.
fn assert_calendar_sync_err<T>(
    handle: std::thread::JoinHandle<T>,
    err: &AppError,
    expected_substring: &str,
) {
    handle.join().expect("mock join");
    match err {
        AppError::CalendarSync(msg) => {
            assert!(msg.contains(expected_substring), "msg: {msg}");
        }
        other => panic!("expected CalendarSync, got {other:?}"),
    }
}

#[test]
fn create_calendar_posts_summary_and_returns_id() {
    let (provider, handle) = provider_env(vec![(200, r#"{"id":"cal-123"}"#.to_owned())], "tok-cc");

    let id = create_calendar(&provider, &instant_backoff(), test_now()).expect("create_calendar");
    let recorded = handle.join().expect("mock join");

    assert_eq!(id, "cal-123");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path_with_query, "/calendars");
    assert_eq!(recorded[0].authorization, "Bearer tok-cc");
    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).expect("body json");
    assert_eq!(body["summary"], "Après Work");
}

#[test_case(200, r#"{"notid":"x"}"#, "parse error" ; "missing_id_is_parse_error")]
#[test_case(500, "{}", "500" ; "http_500_is_error")]
fn create_calendar_error_cases(status: u16, body: &str, expected_msg: &str) {
    let (provider, handle) = provider_env(vec![(status, body.to_owned())], "tok-cc-err");

    let err = create_calendar(&provider, &instant_backoff(), test_now()).unwrap_err();
    handle.join().expect("mock join");

    match &err {
        AppError::CalendarSync(msg) => {
            assert!(
                msg.contains(expected_msg),
                "expected {expected_msg:?} in: {msg}"
            );
        }
        other => panic!("expected CalendarSync, got {other:?}"),
    }
}

#[test]
fn list_app_events_filters_to_marker_and_parses_fields() {
    let body = r#"{
        "items": [
            {"id": "evt-app", "etag": "\"e1\"", "summary": "Gym",
             "description": "Part 1 of 2 — Après Work",
             "start": {"dateTime": "2026-07-15T18:00:00Z"},
             "end":   {"dateTime": "2026-07-15T19:00:00Z"},
             "extendedProperties": {"private": {"apreswork_chunk_id": "chunk-1"}}},
            {"id": "evt-foreign", "summary": "No props",
             "start": {"dateTime": "2026-07-15T10:00:00Z"},
             "end":   {"dateTime": "2026-07-15T11:00:00Z"}},
            {"id": "evt-other-props", "summary": "Wrong key",
             "start": {"dateTime": "2026-07-15T12:00:00Z"},
             "end":   {"dateTime": "2026-07-15T13:00:00Z"},
             "extendedProperties": {"private": {"other_key": "x"}}}
        ]
    }"#;
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-list");
    let events = list_week_events(&provider);
    let recorded = handle.join().expect("mock join");

    assert_eq!(events.len(), 1, "only the marked event is app-owned");
    assert_eq!(events[0].event_id, "evt-app");
    assert_eq!(events[0].etag.as_deref(), Some("\"e1\""));
    assert_eq!(events[0].title, "Gym");
    assert_eq!(
        events[0].description.as_deref(),
        Some("Part 1 of 2 — Après Work")
    );
    assert_eq!(
        events[0].start,
        Utc.with_ymd_and_hms(2026, 7, 15, 18, 0, 0)
            .single()
            .expect("dt")
    );
    assert_eq!(
        events[0].end,
        Utc.with_ymd_and_hms(2026, 7, 15, 19, 0, 0)
            .single()
            .expect("dt")
    );

    let path = &recorded[0].path_with_query;
    assert!(path.starts_with("/calendars/cal-1/events?"), "path: {path}");
    assert!(path.contains("singleEvents=true"), "path: {path}");
    assert!(path.contains("showDeleted=false"), "path: {path}");
    assert!(
        path.contains("timeMin=2026-07-15T00%3A00%3A00Z"),
        "path: {path}"
    );
    assert!(
        path.contains("timeMax=2026-07-22T00%3A00%3A00Z"),
        "path: {path}"
    );
}

#[test]
fn list_app_events_follows_pagination() {
    let page1 = r#"{
        "items": [{"id": "evt-1",
                   "start": {"dateTime": "2026-07-15T18:00:00Z"},
                   "end":   {"dateTime": "2026-07-15T19:00:00Z"},
                   "extendedProperties": {"private": {"apreswork_chunk_id": "c1"}}}],
        "nextPageToken": "p2"
    }"#;
    let page2 = r#"{
        "items": [{"id": "evt-2",
                   "start": {"dateTime": "2026-07-16T18:00:00Z"},
                   "end":   {"dateTime": "2026-07-16T19:00:00Z"},
                   "extendedProperties": {"private": {"apreswork_chunk_id": "c2"}}}]
    }"#;
    let (provider, handle) = provider_env(
        vec![(200, page1.to_owned()), (200, page2.to_owned())],
        "tok-page",
    );
    let events = list_week_events(&provider);
    let recorded = handle.join().expect("mock join");

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_id, "evt-1");
    assert_eq!(events[1].event_id, "evt-2");
    assert!(!recorded[0].path_with_query.contains("pageToken"));
    assert!(recorded[1].path_with_query.contains("pageToken=p2"));
}

#[test_case(
    r#"{"etag": "\"e\"", "start": {"dateTime": "2026-07-15T18:00:00Z"},
        "end": {"dateTime": "2026-07-15T19:00:00Z"},
        "extendedProperties": {"private": {"apreswork_chunk_id": "c"}}}"#
    ; "missing_id_skipped")]
#[test_case(
    r#"{"id": "evt-bad-start", "start": {"dateTime": "not-a-date"},
        "end": {"dateTime": "2026-07-15T19:00:00Z"},
        "extendedProperties": {"private": {"apreswork_chunk_id": "c"}}}"#
    ; "unparseable_start_skipped")]
#[test_case(
    r#"{"id": "evt-no-end", "start": {"dateTime": "2026-07-15T18:00:00Z"},
        "extendedProperties": {"private": {"apreswork_chunk_id": "c"}}}"#
    ; "missing_end_skipped")]
fn list_app_events_skips_malformed_marked_items(item_json: &str) {
    let body = format!(r#"{{"items": [{item_json}]}}"#);
    let (provider, handle) = provider_env(vec![(200, body)], "tok-skip");
    let events = list_week_events(&provider);
    handle.join().expect("mock join");

    assert!(
        events.is_empty(),
        "malformed marked item must be skipped, got {events:?}"
    );
}

#[test]
fn list_app_events_missing_summary_defaults_to_empty_title() {
    let body = r#"{
        "items": [{"id": "evt-untitled",
                   "start": {"dateTime": "2026-07-15T18:00:00Z"},
                   "end":   {"dateTime": "2026-07-15T19:00:00Z"},
                   "extendedProperties": {"private": {"apreswork_chunk_id": "c"}}}]
    }"#;
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-untitled");
    let events = list_week_events(&provider);
    handle.join().expect("mock join");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "");
    assert_eq!(events[0].etag, None);
    assert_eq!(events[0].description, None);
}

#[test_case(204 ; "deleted_204")]
#[test_case(404 ; "already_gone_404")]
#[test_case(200 ; "other_success_200")]
fn delete_event_success_statuses(status: u16) {
    let (provider, handle) = provider_env(vec![(status, String::new())], "tok-del");

    delete_event(&provider, &instant_backoff(), test_now(), "cal-1", "evt-7")
        .expect("delete_event");
    let recorded = handle.join().expect("mock join");

    assert_eq!(recorded[0].method, "DELETE");
    assert_eq!(recorded[0].path_with_query, "/calendars/cal-1/events/evt-7");
    assert_eq!(recorded[0].authorization, "Bearer tok-del");
}

#[test]
fn delete_event_http_error_is_err() {
    let (provider, handle) = provider_env(vec![(500, "{}".to_owned())], "tok-del-err");

    let err =
        delete_event(&provider, &instant_backoff(), test_now(), "cal-1", "evt-7").unwrap_err();
    assert_calendar_sync_err(handle, &err, "500");
}

/// A full event resource as Google echoes it from `events.insert` / `events.patch`.
fn user_event_response(id: &str, summary: &str, start: &str, end: &str) -> String {
    format!(
        r#"{{"id":"{id}","status":"confirmed","summary":"{summary}",
            "start":{{"dateTime":"{start}","timeZone":"UTC"}},
            "end":{{"dateTime":"{end}","timeZone":"UTC"}}}}"#
    )
}

#[test]
fn create_user_event_posts_without_marker_and_returns_event() {
    let response = user_event_response(
        "evt-user",
        "Dentist",
        "2026-07-20T09:00:00Z",
        "2026-07-20T10:00:00Z",
    );
    let (provider, handle) = provider_env(vec![(200, response)], "tok-cu");

    let event = create_user_event(
        &provider,
        &instant_backoff(),
        test_now(),
        "cal-1",
        &user_payload(),
    )
    .expect("create_user_event");
    let recorded = handle.join().expect("mock join");

    assert_eq!(event.calendar_id, "cal-1");
    assert_eq!(event.event_id, "evt-user");
    assert_eq!(event.title, "Dentist");
    assert!(event.busy, "a fresh user event is busy");
    assert!(!event.declined, "own event is never declined");
    assert_eq!(
        event.start,
        Utc.with_ymd_and_hms(2026, 7, 20, 9, 0, 0)
            .single()
            .expect("dt")
    );

    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path_with_query, "/calendars/cal-1/events");
    assert_eq!(recorded[0].authorization, "Bearer tok-cu");

    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).expect("body json");
    assert_eq!(body["summary"], "Dentist");
    assert_eq!(body["description"], "Cleaning");
    assert_eq!(body["start"]["dateTime"], "2026-07-20T09:00:00+00:00");
    assert_eq!(body["start"]["timeZone"], "UTC");
    assert_eq!(body["end"]["dateTime"], "2026-07-20T10:00:00+00:00");
    assert!(
        body.get("extendedProperties").is_none(),
        "user events carry no chunk marker: {body}"
    );
    assert!(
        body.get("reminders").is_none(),
        "user events use calendar-default reminders: {body}"
    );
}

#[test]
fn create_user_event_all_day_writes_date_fields_with_exclusive_end() {
    // Provider echoes an all-day event (date-only start/end).
    let response = r#"{"id":"evt-ad","status":"confirmed","summary":"Vacation",
        "start":{"date":"2026-07-15"},"end":{"date":"2026-07-16"}}"#;
    let (provider, handle) = provider_env(vec![(200, response.to_owned())], "tok-ad");

    // Build Local-midnight instants the way the pull path does, so the
    // write-back formats them back to the same calendar dates on any machine.
    let local_midnight = |y, m, d| {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|nd| nd.and_hms_opt(0, 0, 0))
            .and_then(|dt| dt.and_local_timezone(chrono::Local).earliest())
            .map(|ldt| ldt.with_timezone(&Utc))
            .expect("local midnight")
    };
    let payload = UserEventPayload {
        title: "Vacation".to_owned(),
        description: None,
        start: local_midnight(2026, 7, 15),
        end: local_midnight(2026, 7, 16),
        all_day: true,
    };

    let event = create_user_event(&provider, &instant_backoff(), test_now(), "cal-1", &payload)
        .expect("create_user_event all-day");
    let recorded = handle.join().expect("mock join");

    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).expect("body json");
    assert_eq!(body["start"]["date"], "2026-07-15");
    assert_eq!(body["end"]["date"], "2026-07-16");
    assert!(
        body["start"].get("dateTime").is_none(),
        "all-day start must not carry dateTime: {body}"
    );
    assert!(
        body["start"].get("timeZone").is_none(),
        "all-day start must not carry timeZone: {body}"
    );

    assert!(
        event.all_day,
        "a provider all-day response maps to all_day = true"
    );
}

#[test]
fn create_user_event_serializes_null_description_when_none() {
    let response = user_event_response(
        "evt-x",
        "Quick",
        "2026-07-20T09:00:00Z",
        "2026-07-20T10:00:00Z",
    );
    let (provider, handle) = provider_env(vec![(200, response)], "tok-cu-null");

    let payload = UserEventPayload {
        description: None,
        ..user_payload()
    };
    let event = create_user_event(&provider, &instant_backoff(), test_now(), "cal-1", &payload)
        .expect("create");
    let recorded = handle.join().expect("mock join");

    assert_eq!(event.description, None);
    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).expect("body json");
    assert!(
        body["description"].is_null(),
        "None description serializes to null: {body}"
    );
}

#[test]
fn create_user_event_http_error_is_err() {
    let (provider, handle) = provider_env(vec![(500, "{}".to_owned())], "tok-cu-err");

    let err = create_user_event(
        &provider,
        &instant_backoff(),
        test_now(),
        "cal-1",
        &user_payload(),
    )
    .unwrap_err();
    assert_calendar_sync_err(handle, &err, "500");
}

#[test]
fn create_user_event_unmappable_response_is_err() {
    // 200 OK but the body has no `id` — map_event returns None → error.
    let body = r#"{"summary":"NoId",
        "start":{"dateTime":"2026-07-20T09:00:00Z"},
        "end":{"dateTime":"2026-07-20T10:00:00Z"}}"#;
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-cu-noid");

    let err = create_user_event(
        &provider,
        &instant_backoff(),
        test_now(),
        "cal-1",
        &user_payload(),
    )
    .unwrap_err();
    assert_calendar_sync_err(handle, &err, "missing required event fields");
}

#[test]
fn update_user_event_patches_without_marker_and_returns_event() {
    let response = user_event_response(
        "evt-9",
        "Dentist moved",
        "2026-07-20T11:00:00Z",
        "2026-07-20T12:00:00Z",
    );
    let (provider, handle) = provider_env(vec![(200, response)], "tok-uu");

    let event = update_user_event(
        &provider,
        &instant_backoff(),
        test_now(),
        "cal-1",
        "evt-9",
        &user_payload(),
    )
    .expect("update_user_event");
    let recorded = handle.join().expect("mock join");

    assert_eq!(event.event_id, "evt-9");
    assert_eq!(event.title, "Dentist moved");
    assert_eq!(
        event.start,
        Utc.with_ymd_and_hms(2026, 7, 20, 11, 0, 0)
            .single()
            .expect("dt")
    );

    assert_eq!(recorded[0].method, "PATCH");
    assert_eq!(recorded[0].path_with_query, "/calendars/cal-1/events/evt-9");

    let body: serde_json::Value = serde_json::from_str(&recorded[0].body).expect("body json");
    // Request body reflects the payload, not the response.
    assert_eq!(body["summary"], "Dentist");
    assert!(
        body.get("extendedProperties").is_none(),
        "PATCH must not stamp a chunk marker: {body}"
    );
    assert!(
        body.get("reminders").is_none(),
        "PATCH must not touch reminders: {body}"
    );
}

#[test]
fn update_user_event_http_error_is_err() {
    let (provider, handle) = provider_env(vec![(500, "{}".to_owned())], "tok-uu-err");

    let err = update_user_event(
        &provider,
        &instant_backoff(),
        test_now(),
        "cal-1",
        "evt-9",
        &user_payload(),
    )
    .unwrap_err();
    assert_calendar_sync_err(handle, &err, "500");
}

#[test]
fn trait_ensure_app_calendar_delegates_to_google_http() {
    let (provider, handle) = provider_env(
        vec![(200, r#"{"id":"cal-new"}"#.to_owned())],
        "tok-trait-cal",
    );

    let id = provider
        .ensure_app_calendar(test_now())
        .expect("ensure_app_calendar");
    let recorded = handle.join().expect("mock join");

    assert_eq!(id, "cal-new");
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path_with_query, "/calendars");
}

#[test]
fn trait_list_app_calendar_events_delegates_to_google_http() {
    let body = r#"{
        "items": [{"id": "evt-1",
                   "start": {"dateTime": "2026-07-15T18:00:00Z"},
                   "end":   {"dateTime": "2026-07-15T19:00:00Z"},
                   "extendedProperties": {"private": {"apreswork_chunk_id": "c1"}}}]
    }"#;
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-trait-list");
    let (start, end) = week_window();

    let events = provider
        .list_app_calendar_events(test_now(), "cal-1", start, end)
        .expect("trait list_app_calendar_events");
    handle.join().expect("mock join");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, "evt-1");
}

#[test]
fn trait_execute_sync_ops_delegates_to_batch() {
    // Verifies the trait method funnels every op through ONE batch POST. See batch_tests for codec/retry/chunking edge cases.
    let (provider, handle) = cud_batch_env("rb-trait", "tok-trait-ops");

    let ops = cud_ops(&payload());
    let results = provider
        .execute_sync_ops(test_now(), "cal-1", &ops)
        .expect("execute_sync_ops");
    let recorded = handle.join().expect("mock join");

    assert_cud_batch_single_post(&results, &recorded);
}

#[test]
fn trait_create_user_event_delegates_to_google_http() {
    let response = user_event_response(
        "evt-td",
        "Dentist",
        "2026-07-20T09:00:00Z",
        "2026-07-20T10:00:00Z",
    );
    let (provider, handle) = provider_env(vec![(200, response)], "tok-trait-cu");

    let event = provider
        .create_user_event(test_now(), "cal-1", &user_payload())
        .expect("trait create_user_event");
    let recorded = handle.join().expect("mock join");

    assert_eq!(event.event_id, "evt-td");
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path_with_query, "/calendars/cal-1/events");
}

#[test]
fn trait_update_user_event_delegates_to_google_http() {
    let response = user_event_response(
        "evt-9",
        "Dentist",
        "2026-07-20T09:00:00Z",
        "2026-07-20T10:00:00Z",
    );
    let (provider, handle) = provider_env(vec![(200, response)], "tok-trait-uu");

    let event = provider
        .update_user_event(test_now(), "cal-1", "evt-9", &user_payload())
        .expect("trait update_user_event");
    let recorded = handle.join().expect("mock join");

    assert_eq!(event.event_id, "evt-9");
    assert_eq!(recorded[0].method, "PATCH");
    assert_eq!(recorded[0].path_with_query, "/calendars/cal-1/events/evt-9");
}

#[test]
fn trait_delete_user_event_delegates_to_google_http() {
    let (provider, handle) = provider_env(vec![(204, String::new())], "tok-trait-du");

    provider
        .delete_user_event(test_now(), "cal-1", "evt-7")
        .expect("trait delete_user_event");
    let recorded = handle.join().expect("mock join");

    assert_eq!(recorded[0].method, "DELETE");
    assert_eq!(recorded[0].path_with_query, "/calendars/cal-1/events/evt-7");
}
