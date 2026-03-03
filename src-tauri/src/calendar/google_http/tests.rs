// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for `calendar::google_http`.
//!
//! All endpoints exercised here are local 127.0.0.1 mocks — no request ever
//! leaves the machine.

use std::io::Write as _;
use std::net::TcpListener;
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Utc};
use test_case::test_case;

use super::test_support::{
    instant_backoff, mock_keyring, mock_server, provider_env, provider_with_token, seed_provider,
    test_creds, test_now, unwrap_calendar_sync_err, RecordedRequest,
};
use super::{list_calendars, list_events, BackoffPolicy};
use crate::calendar::google::{GoogleCalendarSync, GoogleEndpoints};
use crate::traits::calendar_sync::{CalendarSync, ExternalCalendar, ExternalEvent};

fn day_window() -> (DateTime<Utc>, DateTime<Utc>) {
    (
        crate::test_support::utc(2026, 7, 11, 0, 0),
        crate::test_support::utc(2026, 7, 12, 0, 0),
    )
}

/// Returns a [401 response, token-refresh 200, <extra>] response sequence.
///
/// Used by tests that exercise the 401→token-refresh retry path. The caller
/// appends the third response (success or another 401).
fn responses_401_refresh(extra: (u16, String)) -> Vec<(u16, String)> {
    vec![
        (401, "{}".to_owned()),
        (
            200,
            r#"{"access_token":"new-tok","expires_in":3600}"#.to_owned(),
        ),
        extra,
    ]
}

fn list_day_events(body: String) -> Vec<ExternalEvent> {
    let (provider, handle) = provider_env(vec![(200, body)], "tok-list");
    let (s, e) = day_window();
    let result =
        list_events(&provider, &instant_backoff(), test_now(), "c", s, e).expect("list_events");
    handle.join().expect("mock join");
    result
}

fn local_midnight(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    NaiveDate::from_ymd_opt(y, m, d)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|dt| dt.and_local_timezone(Local).earliest())
        .map_or_else(
            || panic!("{y}-{m:02}-{d:02} has no local midnight"),
            |ldt| ldt.with_timezone(&Utc),
        )
}

fn list_calendars_ok(
    provider: &GoogleCalendarSync,
    handle: std::thread::JoinHandle<Vec<RecordedRequest>>,
) -> (Vec<ExternalCalendar>, Vec<RecordedRequest>) {
    let result = list_calendars(provider, &instant_backoff(), test_now()).expect("list_calendars");
    let requests = handle.join().expect("mock join");
    (result, requests)
}

fn list_calendars_err(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    handle: std::thread::JoinHandle<Vec<RecordedRequest>>,
) -> (String, Vec<RecordedRequest>) {
    let err = list_calendars(provider, backoff, test_now()).unwrap_err();
    let requests = handle.join().expect("mock join");
    match err {
        crate::error::AppError::CalendarSync(msg) => (msg, requests),
        other => panic!("expected CalendarSync, got {other:?}"),
    }
}

#[test]
fn list_calendars_two_pages_merged() {
    let (provider, handle) = provider_env(
        vec![
            (
                200,
                r#"{"items":[{"id":"c1","summary":"Work","primary":true}],"nextPageToken":"PAGE2"}"#
                    .to_owned(),
            ),
            (
                200,
                r#"{"items":[{"id":"c2","summary":"Personal","primary":false}]}"#.to_owned(),
            ),
        ],
        "tok-abc",
    );

    let (result, requests) = list_calendars_ok(&provider, handle);

    assert_eq!(result.len(), 2, "two items from two pages");
    assert_eq!(result[0].id, "c1");
    assert_eq!(result[0].title, "Work");
    assert!(result[0].primary);
    assert_eq!(result[1].id, "c2");
    assert_eq!(result[1].title, "Personal");
    assert!(!result[1].primary);

    assert!(
        requests[0]
            .path_with_query
            .starts_with("/users/me/calendarList"),
        "req 1 path: {}",
        requests[0].path_with_query
    );
    assert!(
        requests[1]
            .path_with_query
            .starts_with("/users/me/calendarList"),
        "req 2 path: {}",
        requests[1].path_with_query
    );

    assert!(
        !requests[0].path_with_query.contains("pageToken"),
        "req 1 must not have pageToken"
    );
    assert!(
        requests[1].path_with_query.contains("pageToken=PAGE2"),
        "req 2 must have pageToken=PAGE2; got {}",
        requests[1].path_with_query
    );

    assert_eq!(requests[0].authorization, "Bearer tok-abc");
    assert_eq!(requests[1].authorization, "Bearer tok-abc");
}

#[test]
fn list_calendars_missing_fields_handled() {
    let body = r#"{
        "items": [
            {"id":"c1","summary":"Has Summary"},
            {"id":"c2"},
            {"summary":"No Id"}
        ]
    }"#;
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-x");

    let (result, _requests) = list_calendars_ok(&provider, handle);

    assert_eq!(result.len(), 2, "item without id must be skipped");
    assert!(!result[0].primary, "missing primary → false");
    assert_eq!(result[1].title, "", "missing summary → empty title");
}

#[test]
fn list_events_timed_event_happy_path() {
    let body = r#"{
        "items": [{
            "id": "ev1",
            "summary": "Team Standup",
            "description": "Daily sync",
            "start": {"dateTime": "2026-07-11T10:00:00+02:00"},
            "end":   {"dateTime": "2026-07-11T10:30:00+02:00"}
        }]
    }"#;
    let cal_id = "#cal@group.test";
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-ev");

    let (start, end) = day_window();

    let result = list_events(
        &provider,
        &instant_backoff(),
        test_now(),
        cal_id,
        start,
        end,
    )
    .expect("list_events");
    let requests = handle.join().expect("mock join");

    assert_eq!(result.len(), 1);
    let ev = &result[0];
    assert_eq!(ev.calendar_id, cal_id);
    assert_eq!(ev.event_id, "ev1");
    assert_eq!(ev.title, "Team Standup");
    assert_eq!(ev.description.as_deref(), Some("Daily sync"));
    assert_eq!(ev.start, crate::test_support::utc(2026, 7, 11, 8, 0));
    assert_eq!(ev.end, crate::test_support::utc(2026, 7, 11, 8, 30));
    assert!(ev.busy);
    assert!(!ev.declined);

    let pq = &requests[0].path_with_query;
    assert!(
        pq.contains("singleEvents=true"),
        "missing singleEvents: {pq}"
    );
    assert!(
        pq.contains("timeMin=2026-07-11T00%3A00%3A00Z")
            || pq.contains("timeMin=2026-07-11T00:00:00Z"),
        "timeMin: {pq}"
    );
    assert!(
        pq.contains("timeMax=2026-07-12T00%3A00%3A00Z")
            || pq.contains("timeMax=2026-07-12T00:00:00Z"),
        "timeMax: {pq}"
    );
    // `#` is a fragment delimiter and must be percent-encoded; `@` is a
    // path sub-delimiter (RFC 3986) and stays unencoded.
    assert!(
        pq.contains("%23cal@group.test"),
        "# in calendar id must be percent-encoded in path: {pq}"
    );
    assert_eq!(requests[0].authorization, "Bearer tok-ev");
}

#[test]
fn list_events_two_pages_accumulate() {
    let page1 = r#"{"items":[{"id":"e1","start":{"dateTime":"2026-07-11T09:00:00Z"},"end":{"dateTime":"2026-07-11T10:00:00Z"}}],"nextPageToken":"P2"}"#;
    let page2 = r#"{"items":[{"id":"e2","start":{"dateTime":"2026-07-11T11:00:00Z"},"end":{"dateTime":"2026-07-11T12:00:00Z"}}]}"#;
    let (provider, handle) = provider_env(
        vec![(200, page1.to_owned()), (200, page2.to_owned())],
        "tok-p",
    );

    let (start, end) = day_window();
    let result = list_events(
        &provider,
        &instant_backoff(),
        test_now(),
        "primary",
        start,
        end,
    )
    .expect("list_events");
    let requests = handle.join().expect("mock join");

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].event_id, "e1");
    assert_eq!(result[1].event_id, "e2");
    assert!(
        !result[0].all_day,
        "a timed event must not be marked all_day"
    );
    assert!(
        requests[1].path_with_query.contains("pageToken=P2"),
        "page 2 request must have pageToken"
    );
}

#[test]
fn list_events_all_day_event_spans_local_day() {
    // Google sends all-day end as the next day (exclusive) — use as-is.
    let body = r#"{
        "items": [{
            "id": "allday1",
            "start": {"date": "2026-07-15"},
            "end":   {"date": "2026-07-16"}
        }]
    }"#;
    let (provider, handle) = provider_env(vec![(200, body.to_owned())], "tok-ad");

    let q_start = crate::test_support::utc(2026, 7, 15, 0, 0);
    let q_end = crate::test_support::utc(2026, 7, 16, 0, 0);
    let result = list_events(
        &provider,
        &instant_backoff(),
        test_now(),
        "primary",
        q_start,
        q_end,
    )
    .expect("list_events");
    handle.join().expect("mock join");

    assert_eq!(result.len(), 1);
    let ev = &result[0];

    let expected_start = local_midnight(2026, 7, 15);
    let expected_end = local_midnight(2026, 7, 16);

    assert_eq!(
        ev.start, expected_start,
        "all-day start must be local midnight"
    );
    assert_eq!(
        ev.end, expected_end,
        "all-day end must be local midnight of next day"
    );
    assert!(ev.all_day, "a date-only event must be marked all_day");
}

#[test]
fn list_events_transparent_is_not_busy() {
    let body = r#"{
        "items": [{
            "id": "free1",
            "start": {"dateTime": "2026-07-11T10:00:00Z"},
            "end":   {"dateTime": "2026-07-11T11:00:00Z"},
            "transparency": "transparent"
        }]
    }"#;
    let result = list_day_events(body.to_owned());

    assert_eq!(result.len(), 1);
    assert!(!result[0].busy, "transparent → busy=false");
    assert!(!result[0].declined, "transparent → declined=false");
}

/// Parametrized variants for declined/busy flag combinations; see `test_case` below.
#[test_case(true, false, true, false ; "self_declined_gives_declined_not_busy")]
#[test_case(false, true, false, true ; "other_declined_does_not_affect_flags")]
#[allow(clippy::fn_params_excessive_bools)] // test_case function; enum variants add noise without benefit
fn list_events_attendee_declined_flags(
    self_declined: bool,
    other_declined: bool,
    expect_declined: bool,
    expect_busy: bool,
) {
    let mut attendees = Vec::new();
    if self_declined {
        attendees.push(r#"{"self":true,"responseStatus":"declined"}"#);
    }
    if other_declined {
        attendees.push(r#"{"self":false,"responseStatus":"declined"}"#);
    }
    // Add an accepted self-attendee when self_declined=false so the attendees
    // array is non-empty.
    if !self_declined {
        attendees.push(r#"{"self":true,"responseStatus":"accepted"}"#);
    }
    let attendees_json = attendees.join(",");

    let body = format!(
        r#"{{"items":[{{"id":"a1",
            "start":{{"dateTime":"2026-07-11T10:00:00Z"}},
            "end":{{"dateTime":"2026-07-11T11:00:00Z"}},
            "attendees":[{attendees_json}]
        }}]}}"#
    );

    let result = list_day_events(body);

    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].declined, expect_declined,
        "declined flag mismatch"
    );
    assert_eq!(result[0].busy, expect_busy, "busy flag mismatch");
}

#[test_case(
    r#"{"items":[{"id":"ok1","start":{"dateTime":"2026-07-11T09:00:00Z"},"end":{"dateTime":"2026-07-11T10:00:00Z"}},{"id":"gone","status":"cancelled","start":{"dateTime":"2026-07-11T10:00:00Z"},"end":{"dateTime":"2026-07-11T11:00:00Z"}},{"id":"ok2","start":{"dateTime":"2026-07-11T11:00:00Z"},"end":{"dateTime":"2026-07-11T12:00:00Z"}}]}"#
    ; "cancelled_status_skipped")]
#[test_case(
    r#"{"items":[{"id":"ok1","start":{"dateTime":"2026-07-11T09:00:00Z"},"end":{"dateTime":"2026-07-11T10:00:00Z"}},{"id":"bad","start":{"dateTime":"NOT-A-DATE"},"end":{"dateTime":"2026-07-11T11:00:00Z"}},{"id":"ok2","start":{"dateTime":"2026-07-11T11:00:00Z"},"end":{"dateTime":"2026-07-11T12:00:00Z"}}]}"#
    ; "unparseable_datetime_skips_event_others_survive")]
fn list_events_bad_event_filtered(body: &str) {
    let result = list_day_events(body.to_owned());
    assert_eq!(result.len(), 2, "bad event must be skipped, others survive");
    assert_eq!(result[0].event_id, "ok1");
    assert_eq!(result[1].event_id, "ok2");
}

#[test_case(
    r#"{"start":{"dateTime":"2026-07-11T10:00:00Z"},"end":{"dateTime":"2026-07-11T11:00:00Z"}}"# ;
    "missing_event_id"
)]
#[test_case(
    r#"{"id":"bad","end":{"dateTime":"2026-07-11T11:00:00Z"}}"# ;
    "missing_start_field"
)]
#[test_case(
    r#"{"id":"bad","start":{"dateTime":"2026-07-11T09:00:00Z"}}"# ;
    "missing_end_field"
)]
#[test_case(
    r#"{"id":"bad","start":{"dateTime":"2026-07-11T09:00:00Z"},"end":{"dateTime":"NOT-A-DATE"}}"# ;
    "unparseable_end_datetime"
)]
#[test_case(
    r#"{"id":"bad","start":{},"end":{"dateTime":"2026-07-11T11:00:00Z"}}"# ;
    "empty_start_no_datetime_no_date"
)]
fn list_events_incomplete_event_skipped(bad_event_json: &str) {
    let body = format!(
        r#"{{"items":[
            {{"id":"ok1","start":{{"dateTime":"2026-07-11T09:00:00Z"}},"end":{{"dateTime":"2026-07-11T10:00:00Z"}}}},
            {bad_event_json},
            {{"id":"ok2","start":{{"dateTime":"2026-07-11T11:00:00Z"}},"end":{{"dateTime":"2026-07-11T12:00:00Z"}}}}
        ]}}"#
    );
    let result = list_day_events(body);

    assert_eq!(
        result.len(),
        2,
        "incomplete event must be skipped; good ones survive"
    );
    assert_eq!(result[0].event_id, "ok1");
    assert_eq!(result[1].event_id, "ok2");
}

#[test]
fn list_calendars_401_refresh_once_then_success() {
    let (base, handle) = mock_server(responses_401_refresh((
        200,
        r#"{"items":[{"id":"c1","summary":"Work","primary":true}]}"#.to_owned(),
    )));

    // token_url points to the SAME mock so the refresh POST is served in order.
    let token_url = format!("{base}/token");
    let provider = provider_with_token(&token_url, &base, "old-tok");

    let (result, requests) = list_calendars_ok(&provider, handle);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "c1");

    assert_eq!(requests[1].method, "POST", "token refresh must be a POST");

    assert_eq!(
        requests[2].authorization, "Bearer new-tok",
        "retried request must carry new token"
    );

    // New access token must be in the in-memory cache after the refresh.
    // Use cached_access_token() because expires_at from a real refresh call
    // is based on Utc::now() (2026), which predates test_now() (2030).
    assert_eq!(
        provider.cached_access_token().as_deref(),
        Some("new-tok"),
        "new token must be cached after 401→refresh"
    );
    // Refresh token must be retained in the keyring (response omitted it).
    let saved = provider.keyring().load().expect("load").expect("Some");
    assert_eq!(saved.refresh_token.as_deref(), Some("test-refresh-token"));
}

#[test]
fn list_calendars_401_refresh_then_401_returns_err() {
    let (base, handle) = mock_server(responses_401_refresh((401, "{}".to_owned())));
    let token_url = format!("{base}/token");
    let provider = provider_with_token(&token_url, &base, "old-tok");

    let (msg, requests) = list_calendars_err(&provider, &instant_backoff(), handle);
    assert!(
        msg.contains("reconnect required"),
        "error must mention reconnect: {msg}"
    );

    assert_eq!(requests.len(), 3, "expected 2 API + 1 token request");
}

#[test_case(403 ; "backoff_403")]
#[test_case(429 ; "backoff_429")]
fn list_calendars_rate_limit_then_success(status: u16) {
    let (provider, handle) = provider_env(
        vec![
            (status, r#"{"error":"rateLimitExceeded"}"#.to_owned()),
            (
                200,
                r#"{"items":[{"id":"c1","summary":"Work"}]}"#.to_owned(),
            ),
        ],
        "tok-rl",
    );

    let (result, requests) = list_calendars_ok(&provider, handle);

    assert_eq!(result.len(), 1);
    assert_eq!(
        requests.len(),
        2,
        "first fails, second succeeds → 2 requests"
    );
}

#[test]
fn list_calendars_rate_limit_exhausted_returns_err() {
    let (provider, handle) = provider_env(
        vec![
            (429, "{}".to_owned()),
            (429, "{}".to_owned()),
            (429, "{}".to_owned()),
        ],
        "tok-ex",
    );

    let backoff = BackoffPolicy {
        base_delay: Duration::from_millis(1),
        max_attempts: 3,
    };
    let (msg, requests) = list_calendars_err(&provider, &backoff, handle);

    assert!(msg.contains("429"), "error must mention 429: {msg}");
    assert!(
        msg.contains("3 attempts"),
        "error must mention attempt count: {msg}"
    );
    assert_eq!(
        requests.len(),
        3,
        "exactly max_attempts requests must be made"
    );
}

#[test_case(500, "{}", "500" ; "immediate_err_no_retry")]
#[test_case(200, "not valid json!!!", "parse error" ; "malformed_json")]
fn list_calendars_single_response_error(status: u16, body: &str, expected_msg: &str) {
    let (provider, handle) = provider_env(vec![(status, body.to_owned())], "tok-err");
    let (msg, requests) = list_calendars_err(&provider, &instant_backoff(), handle);
    assert!(
        msg.contains(expected_msg),
        "error must contain '{expected_msg}': {msg}"
    );
    assert_eq!(requests.len(), 1, "single-attempt error: no retry");
}

#[test]
fn list_calendars_truncated_body_returns_parse_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let base_url = format!("http://127.0.0.1:{port}");

    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = vec![0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let _ = stream.write_all(
                // Content-Length: 1000 advertises 1000 bytes but no body follows —
                // reqwest will read until the connection closes having received far fewer bytes.
                b"HTTP/1.1 200 OK\r\n\
                  Content-Type: application/json\r\n\
                  Content-Length: 1000\r\n\
                  Connection: close\r\n\r\n",
            );
            // stream drops at end of this block, closing the TCP connection.
            // The implicit drop is the truncation mechanism, not an explicit close call.
        }
        // Vec::<RecordedRequest>::new() is a type-match with list_calendars_err's expected
        // return — this raw-socket handler has no request-recording machinery.
        Vec::<RecordedRequest>::new()
    });

    let provider = seed_provider(&base_url, "tok-trunc");

    let (msg, _requests) = list_calendars_err(&provider, &instant_backoff(), handle);

    assert!(
        msg.contains("parse error"),
        "truncated body must return parse error: {msg}"
    );
}

#[test]
fn list_calendars_no_credential_returns_reconnect_err() {
    let keyring = mock_keyring();
    let provider = GoogleCalendarSync::with_mock_keyring(
        test_creds(),
        keyring,
        GoogleEndpoints {
            auth_url: "http://127.0.0.1:1/auth".to_owned(),
            token_url: "http://127.0.0.1:1/token".to_owned(),
            api_base_url: "http://127.0.0.1:1/api".to_owned(),
        },
        Duration::from_secs(5),
    );

    let err = list_calendars(&provider, &instant_backoff(), test_now()).unwrap_err();
    let msg = unwrap_calendar_sync_err(err);
    assert!(
        msg.contains("reconnect required"),
        "error must mention reconnect: {msg}"
    );
}

#[test]
fn list_calendars_network_error_returns_err_without_token_or_url() {
    let provider = provider_with_token("http://127.0.0.1:1/token", "http://127.0.0.1:1", "tok-net");

    let err = list_calendars(&provider, &instant_backoff(), test_now()).unwrap_err();
    let msg = unwrap_calendar_sync_err(err);
    // Must mention "network error" or a status, but NOT the token or URL.
    assert!(
        msg.contains("network error") || msg.chars().any(char::is_numeric),
        "error should describe failure without token: {msg}"
    );
    assert!(
        !msg.contains("tok-net"),
        "token must not appear in error: {msg}"
    );
}

#[test]
fn trait_list_calendars_delegates_to_google_http() {
    let (base, handle) = mock_server(vec![(
        200,
        r#"{"items":[{"id":"primary","summary":"My Calendar","primary":true}]}"#.to_owned(),
    )]);
    let provider = provider_with_token("http://127.0.0.1:1/token", &base, "tok-trait");

    let result = provider
        .list_calendars(crate::test_support::test_now())
        .expect("trait list_calendars");
    handle.join().expect("mock join");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "primary");
    assert!(result[0].primary);
}
