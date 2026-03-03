// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `calendar::google_http` multipart batch push path
//! (`batch_sync_ops` and its codec helpers).
//!
//! Pure codec tests need no network; the end-to-end tests drive `batch_sync_ops`
//! against a local 127.0.0.1 `multipart/mixed` mock — no request ever leaves the
//! machine.

use std::time::Duration;

use test_case::test_case;

use super::super::test_support::{
    assert_cud_batch_single_post, batch_body, chunk_payload, cud_batch_env, cud_ops, cud_results,
    instant_backoff, mock_server_ct, ok_event_part, seed_provider, test_now, RecordedRequest,
};
use super::{
    batch_sync_ops, batch_url, boundary_from_content_type, chunk_create_body, chunk_update_body,
    classify_part, encode_batch, event_path, events_path, inner_is_retryable, jitter, max_hint,
    parse_batch_response, parse_retry_after, retry_delay, BackoffPolicy, BatchInnerResponse,
    BATCH_MAX_OPS,
};
use crate::calendar::google::GoogleCalendarSync;
use crate::traits::calendar_sync::{ChunkEventPayload, SyncOp, SyncOpResult};

fn payload() -> ChunkEventPayload {
    chunk_payload("chunk-42", "Leg day")
}

/// Seed a provider against an already-started mock, push `ops` through
/// `batch_sync_ops` (default batch-max) expecting a `CalendarSync` failure, then
/// assert the message contains `needle` (when `Some`) and that exactly `attempts`
/// requests were recorded.
fn assert_batch_err(
    base: &str,
    token: &str,
    handle: std::thread::JoinHandle<Vec<RecordedRequest>>,
    ops: &[SyncOp],
    needle: Option<&str>,
    attempts: usize,
) {
    let provider = seed_provider(base, token);
    let err = batch_sync_ops(
        &provider,
        &instant_backoff(),
        test_now(),
        BATCH_MAX_OPS,
        "cal-1",
        ops,
    )
    .unwrap_err();
    let recorded = handle.join().expect("mock join");
    match &err {
        crate::error::AppError::CalendarSync(msg) => {
            if let Some(needle) = needle {
                assert!(msg.contains(needle), "expected {needle:?} in: {msg}");
            }
        }
        other => panic!("expected CalendarSync, got {other:?}"),
    }
    assert_eq!(recorded.len(), attempts, "recorded attempts");
}

#[test]
fn batch_url_is_sibling_of_api_base() {
    assert_eq!(
        batch_url("https://www.googleapis.com/calendar/v3").expect("prod"),
        "https://www.googleapis.com/batch/calendar/v3"
    );
    assert_eq!(
        batch_url("http://127.0.0.1:8080").expect("mock"),
        "http://127.0.0.1:8080/batch/calendar/v3"
    );
}

#[test]
fn events_and_event_paths_are_host_relative_and_encoded() {
    assert_eq!(
        events_path("http://api.test", "cal-1").expect("events"),
        "/calendars/cal-1/events"
    );
    assert_eq!(
        event_path("http://api.test", "cal-1", "evt-9").expect("event"),
        "/calendars/cal-1/events/evt-9"
    );
    assert_eq!(
        events_path("https://www.googleapis.com/calendar/v3", "cal-1").expect("prod events"),
        "/calendar/v3/calendars/cal-1/events"
    );
    // Ids with reserved characters are percent-encoded.
    assert_eq!(
        event_path("http://api.test", "team@x.com", "a/b").expect("encoded"),
        "/calendars/team@x.com/events/a%2Fb"
    );
}

#[test]
fn chunk_create_body_stamps_marker_and_reminders() {
    let body = chunk_create_body(&payload());
    assert_eq!(body["summary"], "Gym");
    assert_eq!(body["description"], "Leg day");
    assert_eq!(
        body["extendedProperties"]["private"]["apreswork_chunk_id"],
        "chunk-42"
    );
    assert_eq!(body["reminders"]["useDefault"], false);
    assert_eq!(body["reminders"]["overrides"], serde_json::json!([]));
    assert_eq!(body["start"]["dateTime"], "2026-07-15T18:00:00+00:00");
    assert_eq!(body["start"]["timeZone"], "UTC");
    assert_eq!(body["end"]["dateTime"], "2026-07-15T19:00:00+00:00");
    assert_eq!(body["end"]["timeZone"], "UTC");
}

#[test]
fn chunk_update_body_omits_marker_and_reminders() {
    let body = chunk_update_body(&payload());
    assert_eq!(body["summary"], "Gym");
    assert!(
        body.get("extendedProperties").is_none(),
        "PATCH must not re-stamp the chunk marker: {body}"
    );
    assert!(
        body.get("reminders").is_none(),
        "PATCH must not touch reminders: {body}"
    );
    assert_eq!(body["start"]["dateTime"], "2026-07-15T18:00:00+00:00");
    assert_eq!(body["end"]["dateTime"], "2026-07-15T19:00:00+00:00");
}

#[test]
fn encode_batch_frames_each_op_with_indexed_content_id() {
    let create = SyncOp::Create(payload());
    let update = SyncOp::Update {
        event_id: "evt-u".to_owned(),
        payload: payload(),
    };
    let delete = SyncOp::Delete {
        event_id: "evt-d".to_owned(),
    };
    let ops: Vec<(usize, &SyncOp)> = vec![(0, &create), (1, &update), (2, &delete)];

    let (boundary, body) = encode_batch("http://api.test", "cal-1", &ops).expect("encode");

    assert!(boundary.starts_with("batch_"), "boundary: {boundary}");
    assert!(body.contains(&format!("--{boundary}\r\n")));
    assert!(
        body.contains(&format!("--{boundary}--")),
        "closing delimiter"
    );
    assert!(body.contains("Content-ID: <item-0>"));
    assert!(body.contains("Content-ID: <item-1>"));
    assert!(body.contains("Content-ID: <item-2>"));
    assert!(body.contains("POST /calendars/cal-1/events HTTP/1.1"));
    assert!(body.contains("PATCH /calendars/cal-1/events/evt-u HTTP/1.1"));
    assert!(body.contains("DELETE /calendars/cal-1/events/evt-d HTTP/1.1"));
    assert!(body.contains("apreswork_chunk_id"));

    assert_eq!(
        body.matches("Content-Type: application/json").count(),
        2,
        "only create+update carry a JSON inner body"
    );
}

#[test]
fn parse_batch_response_decodes_status_body_and_retry_after() {
    let boundary = "resp_bnd";
    let body = batch_body(
        boundary,
        &[
            ok_event_part(r#"{"id":"evt-c","etag":"\"e1\""}"#),
            ("HTTP/1.1 204 No Content", "", ""),
            (
                "HTTP/1.1 429 Too Many Requests",
                "Retry-After: 7\r\n",
                r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#,
            ),
        ],
    );
    let ct = format!("multipart/mixed; boundary={boundary}");

    let parts = parse_batch_response(&ct, &body).expect("parse");

    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].status, 200);
    assert!(parts[0].body.contains("evt-c"), "body: {}", parts[0].body);
    assert_eq!(parts[0].retry_after, None);
    assert_eq!(parts[1].status, 204);
    assert_eq!(parts[1].body, "");
    assert_eq!(parts[2].status, 429);
    assert_eq!(parts[2].retry_after, Some(Duration::from_secs(7)));
}

#[test]
fn parse_batch_response_quoted_boundary_is_accepted() {
    let body = batch_body("qb", &[("HTTP/1.1 204 No Content", "", "")]);
    let parts = parse_batch_response("multipart/mixed; boundary=\"qb\"", &body).expect("parse");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].status, 204);
}

#[test]
fn parse_batch_response_missing_boundary_errs() {
    let err = parse_batch_response("application/json", "not multipart").unwrap_err();
    assert!(
        matches!(&err, crate::error::AppError::CalendarSync(m) if m.contains("boundary")),
        "err: {err:?}"
    );
}

#[test]
fn parse_batch_response_malformed_part_errs() {
    let body = "--b\r\nno-blank-line-here\r\n--b--\r\n";
    let err = parse_batch_response("multipart/mixed; boundary=b", body).unwrap_err();
    assert!(
        matches!(&err, crate::error::AppError::CalendarSync(_)),
        "err: {err:?}"
    );
}

#[test]
fn parse_batch_response_non_http_status_line_errs() {
    let body = batch_body("b", &[("GARBAGE LINE", "", "")]);
    let err = parse_batch_response("multipart/mixed; boundary=b", &body).unwrap_err();
    assert!(
        matches!(&err, crate::error::AppError::CalendarSync(m) if m.contains("status")),
        "err: {err:?}"
    );
}

#[test_case("multipart/mixed; boundary=", None ; "empty_value")]
#[test_case("multipart/mixed; boundary=\"\"", None ; "empty_quoted")]
#[test_case("multipart/mixed; boundary=abc", Some("abc") ; "real_boundary")]
fn boundary_from_content_type_cases(ct: &str, expected: Option<&str>) {
    assert_eq!(boundary_from_content_type(ct).as_deref(), expected);
}

#[test]
fn parse_batch_response_skips_empty_segments() {
    // Two adjacent delimiters yield an empty segment the decoder must skip before
    // the real part that follows (not count it, not error on it).
    let body = "--b\r\n\r\n--b\r\nContent-Type: application/http\r\n\r\n\
                HTTP/1.1 204 No Content\r\n\r\n\r\n--b--\r\n";
    let parts = parse_batch_response("multipart/mixed; boundary=b", body).expect("parse");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].status, 204);
}

#[test_case("5", Some(Duration::from_secs(5)))]
#[test_case(" 10 ", Some(Duration::from_secs(10)))]
#[test_case("0", Some(Duration::from_secs(0)))]
#[test_case("abc", None)]
#[test_case("", None)]
// HTTP-date form is not supported (Calendar sends seconds when it sends any).
#[test_case("Wed, 21 Oct 2026 07:28:00 GMT", None)]
fn parse_retry_after_cases(input: &str, expected: Option<Duration>) {
    assert_eq!(parse_retry_after(input), expected);
}

#[test_case(429, "", true)]
#[test_case(500, "", true)]
#[test_case(502, "", true)]
#[test_case(503, "", true)]
#[test_case(504, "", true)]
#[test_case(403, r#"{"error":{"errors":[{"domain":"usageLimits"}]}}"#, true)]
#[test_case(403, "rateLimitExceeded", true)]
#[test_case(403, "userRateLimitExceeded", true)]
#[test_case(403, r#"{"error":"permission denied"}"#, false)]
#[test_case(200, "", false)]
#[test_case(400, "", false)]
#[test_case(404, "", false)]
#[test_case(409, "", false)]
#[test_case(412, "", false)]
fn inner_is_retryable_cases(status: u16, body: &str, expected: bool) {
    assert_eq!(inner_is_retryable(status, body), expected);
}

#[test]
fn jitter_is_bounded_by_base_and_zero_for_zero_base() {
    let now = test_now();
    for _ in 0..64 {
        assert!(jitter(Duration::from_millis(100), now) < Duration::from_millis(100));
    }
    assert_eq!(jitter(Duration::ZERO, now), Duration::ZERO);
}

#[test]
fn retry_delay_grows_exponentially_honors_hint_and_caps() {
    let bp = BackoffPolicy {
        base_delay: Duration::from_millis(100),
        max_attempts: 8,
    };
    let now = test_now();
    let d1 = retry_delay(&bp, 1, None, now);
    assert!(
        (Duration::from_millis(100)..Duration::from_millis(200)).contains(&d1),
        "d1: {d1:?}"
    );
    let d3 = retry_delay(&bp, 3, None, now);
    assert!(
        (Duration::from_millis(400)..Duration::from_millis(500)).contains(&d3),
        "d3: {d3:?}"
    );
    assert_eq!(
        retry_delay(&bp, 1, Some(Duration::from_secs(5)), now),
        Duration::from_secs(5)
    );
    assert_eq!(
        retry_delay(&bp, 1, Some(Duration::from_secs(120)), now),
        Duration::from_secs(64)
    );
    // A huge attempt saturates at the ceiling, not a panic/overflow.
    assert!(retry_delay(&bp, 40, None, now) <= Duration::from_secs(64));
}

#[test]
fn classify_part_delete_permanent_error_errs() {
    // A Delete failing with a non-404, non-retryable status is a permanent error
    // (not idempotent-gone) → fails the whole cycle.
    let op = SyncOp::Delete {
        event_id: "evt-x".to_owned(),
    };
    let part = BatchInnerResponse {
        status: 400,
        retry_after: None,
        body: r#"{"error":"bad delete"}"#.to_owned(),
    };
    let err = classify_part(&op, &part).unwrap_err();
    assert!(
        matches!(&err, crate::error::AppError::CalendarSync(m) if m.contains("400")),
        "err: {err:?}"
    );
}

#[test_case(
    Some(Duration::from_secs(3)),
    Some(Duration::from_secs(8)),
    Some(Duration::from_secs(8))
)]
#[test_case(
    Some(Duration::from_secs(8)),
    Some(Duration::from_secs(3)),
    Some(Duration::from_secs(8))
)]
#[test_case(None, Some(Duration::from_secs(3)), Some(Duration::from_secs(3)))]
#[test_case(Some(Duration::from_secs(3)), None, Some(Duration::from_secs(3)))]
#[test_case(None, None, None)]
fn max_hint_cases(a: Option<Duration>, b: Option<Duration>, expected: Option<Duration>) {
    assert_eq!(max_hint(a, b), expected);
}

/// Push the standard CUD op set through `batch_sync_ops` against `provider`
/// (default batch-max, `"cal-1"`), then join the mock server `handle`. Shared
/// by tests that only differ in how `provider`/`handle` were constructed.
fn run_cud_batch(
    provider: &GoogleCalendarSync,
    handle: std::thread::JoinHandle<Vec<RecordedRequest>>,
) -> (Vec<SyncOpResult>, Vec<RecordedRequest>) {
    let ops = cud_ops(&payload());
    let results = batch_sync_ops(
        provider,
        &instant_backoff(),
        test_now(),
        BATCH_MAX_OPS,
        "cal-1",
        &ops,
    )
    .expect("batch");
    let recorded = handle.join().expect("mock join");
    (results, recorded)
}

/// Build the second batch response (`boundary=b2`, single ok `ok_event_part`
/// keyed by `event_json`), pair it with the caller's first response `r1`
/// (`boundary=b1`) into a two-response mock server, and seed a provider
/// against it with `token`. Shared by tests exercising a retry/second-batch
/// flow.
fn two_batch_response_env(
    r1: String,
    event_json: &'static str,
    token: &str,
) -> (
    GoogleCalendarSync,
    std::thread::JoinHandle<Vec<RecordedRequest>>,
) {
    let r2 = batch_body("b2", &[ok_event_part(event_json)]);
    let (base, handle) = mock_server_ct(vec![
        (200, "multipart/mixed; boundary=b1".to_owned(), r1),
        (200, "multipart/mixed; boundary=b2".to_owned(), r2),
    ]);
    let provider = seed_provider(&base, token);
    (provider, handle)
}

#[test]
fn batch_sync_ops_maps_all_variants_in_one_request() {
    let (provider, handle) = cud_batch_env("rb1", "tok-batch");

    let (results, recorded) = run_cud_batch(&provider, handle);

    assert_cud_batch_single_post(&results, &recorded);
    assert_eq!(recorded[0].authorization, "Bearer tok-batch");
    assert!(recorded[0].body.starts_with("--batch_"));
    assert!(recorded[0]
        .body
        .contains("POST /calendars/cal-1/events HTTP/1.1"));
    assert!(recorded[0]
        .body
        .contains("PATCH /calendars/cal-1/events/evt-u HTTP/1.1"));
    assert!(recorded[0]
        .body
        .contains("DELETE /calendars/cal-1/events/evt-d HTTP/1.1"));
}

#[test]
fn batch_sync_ops_retries_only_throttled_ops_and_preserves_order() {
    let r1 = batch_body(
        "b1",
        &[
            ok_event_part(r#"{"id":"evt-c","etag":"\"e-c\""}"#),
            (
                "HTTP/1.1 429 Too Many Requests",
                "",
                r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#,
            ),
            ("HTTP/1.1 204 No Content", "", ""),
        ],
    );
    let (provider, handle) =
        two_batch_response_env(r1, r#"{"id":"evt-u","etag":"\"e-u\""}"#, "tok-retry");

    let (results, recorded) = run_cud_batch(&provider, handle);

    // Results stay in original op order despite the update being retried later.
    assert_eq!(results, cud_results());
    assert_eq!(recorded.len(), 2);
    // The retry re-sends ONLY the throttled update, tagged with its original index.
    assert!(recorded[1]
        .body
        .contains("PATCH /calendars/cal-1/events/evt-u HTTP/1.1"));
    assert!(recorded[1].body.contains("Content-ID: <item-1>"));
    assert!(
        !recorded[1]
            .body
            .contains("POST /calendars/cal-1/events HTTP/1.1"),
        "already-created op must not be resent"
    );
}

#[test]
fn batch_sync_ops_permanent_inner_failure_errs() {
    let resp = batch_body(
        "b",
        &[
            ("HTTP/1.1 400 Bad Request", "", r#"{"error":"bad request"}"#),
            ("HTTP/1.1 204 No Content", "", ""),
        ],
    );
    let (base, handle) =
        mock_server_ct(vec![(200, "multipart/mixed; boundary=b".to_owned(), resp)]);
    let ops = vec![
        SyncOp::Create(payload()),
        SyncOp::Delete {
            event_id: "evt-d".to_owned(),
        },
    ];
    assert_batch_err(&base, "tok-perm", handle, &ops, Some("400"), 1);
}

#[test]
fn batch_sync_ops_delete_404_is_idempotent_success() {
    let resp = batch_body(
        "b",
        &[("HTTP/1.1 404 Not Found", "", r#"{"error":"gone"}"#)],
    );
    let (base, handle) =
        mock_server_ct(vec![(200, "multipart/mixed; boundary=b".to_owned(), resp)]);
    let provider = seed_provider(&base, "tok-del404");

    let ops = vec![SyncOp::Delete {
        event_id: "evt-gone".to_owned(),
    }];
    let results = batch_sync_ops(
        &provider,
        &instant_backoff(),
        test_now(),
        BATCH_MAX_OPS,
        "cal-1",
        &ops,
    )
    .expect("batch");
    handle.join().expect("mock join");

    assert_eq!(results, vec![SyncOpResult::Deleted]);
}

#[test]
fn batch_sync_ops_chunks_at_batch_max_preserving_order() {
    let r1 = batch_body(
        "b1",
        &[
            ok_event_part(r#"{"id":"evt-a","etag":"\"ea\""}"#),
            ok_event_part(r#"{"id":"evt-b","etag":"\"eb\""}"#),
        ],
    );
    let (provider, handle) =
        two_batch_response_env(r1, r#"{"id":"evt-c","etag":"\"ec\""}"#, "tok-chunk");

    let ops = vec![
        SyncOp::Create(chunk_payload("c-a", "Leg day")),
        SyncOp::Create(chunk_payload("c-b", "Leg day")),
        SyncOp::Create(chunk_payload("c-c", "Leg day")),
    ];
    let results =
        batch_sync_ops(&provider, &instant_backoff(), test_now(), 2, "cal-1", &ops).expect("batch");
    let recorded = handle.join().expect("mock join");

    assert_eq!(
        results,
        vec![
            SyncOpResult::Created {
                chunk_id: "c-a".to_owned(),
                event_id: "evt-a".to_owned(),
                etag: Some("\"ea\"".to_owned()),
            },
            SyncOpResult::Created {
                chunk_id: "c-b".to_owned(),
                event_id: "evt-b".to_owned(),
                etag: Some("\"eb\"".to_owned()),
            },
            SyncOpResult::Created {
                chunk_id: "c-c".to_owned(),
                event_id: "evt-c".to_owned(),
                etag: Some("\"ec\"".to_owned()),
            },
        ]
    );
    assert_eq!(recorded.len(), 2, "two sub-batches");
    // First batch carries the first two ops (indices 0,1); second carries index 2.
    assert!(recorded[0].body.contains("Content-ID: <item-0>"));
    assert!(recorded[0].body.contains("Content-ID: <item-1>"));
    assert!(!recorded[0].body.contains("Content-ID: <item-2>"));
    assert!(recorded[1].body.contains("Content-ID: <item-2>"));
}

#[test]
fn batch_sync_ops_gives_up_after_max_attempts_when_throttled() {
    let throttled = || {
        batch_body(
            "b",
            &[(
                "HTTP/1.1 429 Too Many Requests",
                "",
                r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#,
            )],
        )
    };
    let (base, handle) = mock_server_ct(vec![
        (200, "multipart/mixed; boundary=b".to_owned(), throttled()),
        (200, "multipart/mixed; boundary=b".to_owned(), throttled()),
        (200, "multipart/mixed; boundary=b".to_owned(), throttled()),
    ]);
    let ops = vec![SyncOp::Create(payload())];
    assert_batch_err(&base, "tok-exhaust", handle, &ops, None, 3);
}

#[test]
fn batch_sync_ops_empty_ops_makes_no_request() {
    let provider = seed_provider("http://127.0.0.1:1", "tok-empty");
    let results = batch_sync_ops(
        &provider,
        &instant_backoff(),
        test_now(),
        BATCH_MAX_OPS,
        "cal-1",
        &[],
    )
    .expect("empty");
    assert!(results.is_empty());
}

#[test]
fn batch_sync_ops_whole_batch_http_error_errs() {
    let (base, handle) =
        mock_server_ct(vec![(400, "application/json".to_owned(), "{}".to_owned())]);
    let ops = vec![SyncOp::Create(payload())];
    assert_batch_err(&base, "tok-batch400", handle, &ops, Some("400"), 1);
}

#[test]
fn batch_sync_ops_part_count_mismatch_errs() {
    let resp = batch_body("b", &[("HTTP/1.1 204 No Content", "", "")]);
    let (base, handle) =
        mock_server_ct(vec![(200, "multipart/mixed; boundary=b".to_owned(), resp)]);
    let ops = vec![
        SyncOp::Delete {
            event_id: "evt-1".to_owned(),
        },
        SyncOp::Delete {
            event_id: "evt-2".to_owned(),
        },
    ];
    assert_batch_err(&base, "tok-mismatch", handle, &ops, Some("part count"), 1);
}
