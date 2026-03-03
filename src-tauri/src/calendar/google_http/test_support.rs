// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared mock-HTTP-server harness for `google_http` tests.
//!
//! Used by both `tests` (read endpoints) and `write_tests` (write endpoints).
//! All servers bind 127.0.0.1 — no request ever leaves the machine.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeZone as _, Utc};

use super::BackoffPolicy;
use crate::calendar::google::{GoogleCalendarSync, GoogleCredentials, GoogleEndpoints};
use crate::calendar::google_token::{KeyringStore, PersistedCredential};
use crate::traits::calendar_sync::{ChunkEventPayload, SyncOp, SyncOpResult};

#[derive(Debug)]
pub(crate) struct RecordedRequest {
    pub(crate) method: String,
    pub(crate) path_with_query: String,
    pub(crate) authorization: String,
    pub(crate) body: String,
}

/// Start a sequential mock HTTP server that serves N canned responses in order.
///
/// Each response is served with `Connection: close` so reqwest opens a fresh
/// connection for every request.  The returned `JoinHandle` yields all recorded
/// requests once all `responses` have been served.
///
/// Responses are `application/json`; use [`mock_server_ct`] when the test needs
/// a different `Content-Type` (e.g. `multipart/mixed` batch responses).
pub(crate) fn mock_server(
    responses: Vec<(u16, String)>,
) -> (String, std::thread::JoinHandle<Vec<RecordedRequest>>) {
    mock_server_ct(
        responses
            .into_iter()
            .map(|(status, body)| (status, "application/json".to_owned(), body))
            .collect(),
    )
}

/// Like [`mock_server`] but each response carries an explicit `Content-Type`.
///
/// Batch responses are `multipart/mixed; boundary=…`, and the decoder reads the
/// boundary from that header (Google picks its own, distinct from the request's),
/// so tests must be able to set it per response.
pub(crate) fn mock_server_ct(
    responses: Vec<(u16, String, String)>,
) -> (String, std::thread::JoinHandle<Vec<RecordedRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let port = listener.local_addr().expect("mock addr").port();
    let base_url = format!("http://127.0.0.1:{port}");

    let handle = std::thread::spawn(move || {
        let mut recorded = Vec::new();
        for (status, content_type, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = vec![0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).to_string();

            recorded.push(parse_http_request(&raw));

            let status_text = match status {
                204 => "204 No Content",
                400 => "400 Bad Request",
                401 => "401 Unauthorized",
                403 => "403 Forbidden",
                404 => "404 Not Found",
                429 => "429 Too Many Requests",
                500 => "500 Internal Server Error",
                _ => "200 OK",
            };
            let response = format!(
                "HTTP/1.1 {status_text}\r\n\
                 Content-Type: {content_type}\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
        recorded
    });

    (base_url, handle)
}

/// Build a `multipart/mixed` batch response body.
///
/// Each part is `(status_line, inner_header_block, inner_body)`; the header
/// block carries a trailing CRLF per header (or is empty for none). The framing
/// mirrors what Google returns: `Content-Type: application/http` part wrapper,
/// then an embedded HTTP response. Shared by `batch_tests` and `write_tests`.
pub(crate) fn batch_body(boundary: &str, parts: &[(&str, &str, &str)]) -> String {
    let mut s = String::new();
    for (i, (status_line, headers, body)) in parts.iter().enumerate() {
        s.push_str("--");
        s.push_str(boundary);
        s.push_str("\r\nContent-Type: application/http\r\nContent-ID: <response-item-");
        s.push_str(&i.to_string());
        s.push_str(">\r\n\r\n");
        s.push_str(status_line);
        s.push_str("\r\n");
        s.push_str(headers);
        s.push_str("\r\n");
        s.push_str(body);
        s.push_str("\r\n");
    }
    s.push_str("--");
    s.push_str(boundary);
    s.push_str("--\r\n");
    s
}

pub(crate) fn ok_event_part(json: &'static str) -> (&'static str, &'static str, &'static str) {
    (
        "HTTP/1.1 200 OK",
        "Content-Type: application/json\r\n",
        json,
    )
}

fn parse_http_request(raw: &str) -> RecordedRequest {
    let header_end = raw.find("\r\n\r\n");
    let headers_part = header_end.map_or(raw, |idx| &raw[..idx]);
    let body = header_end.map_or(String::new(), |idx| {
        raw[idx + 4..].trim_end_matches('\0').to_owned()
    });

    let mut lines = headers_part.split("\r\n");
    let first_line = lines.next().unwrap_or("");
    let mut parts = first_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("").to_owned();
    let path_with_query = parts.next().unwrap_or("").to_owned();

    let authorization = lines
        .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
        .map(|l| l["authorization:".len()..].trim().to_owned())
        .unwrap_or_default();

    RecordedRequest {
        method,
        path_with_query,
        authorization,
        body,
    }
}

pub(crate) fn test_creds() -> GoogleCredentials {
    GoogleCredentials {
        client_id: "test-client-id".to_owned(),
        client_secret: "test-secret".to_owned(),
    }
}

/// Build a mock keyring for tests.
pub(crate) fn mock_keyring() -> KeyringStore {
    KeyringStore::with_mock_entry(Arc::new(keyring::Entry::new_with_credential(Box::new(
        keyring::mock::MockCredential::default(),
    ))))
}

/// Build a provider with seeded access token and fully-specified endpoints
/// (all 127.0.0.1 — no stray calls to Google).
pub(crate) fn provider_with_token(
    token_url: &str,
    api_base: &str,
    access_token: &str,
) -> GoogleCalendarSync {
    let keyring = mock_keyring();
    let far_future = Utc
        .with_ymd_and_hms(2999, 1, 1, 0, 0, 0)
        .single()
        .expect("token expiry");
    keyring
        .save(&PersistedCredential {
            refresh_token: Some("test-refresh-token".to_owned()),
            expires_at: far_future,
        })
        .expect("seed credential");
    let provider = GoogleCalendarSync::with_mock_keyring(
        test_creds(),
        keyring,
        GoogleEndpoints {
            auth_url: "http://127.0.0.1:1/auth".to_owned(),
            token_url: token_url.to_owned(),
            api_base_url: api_base.to_owned(),
        },
        Duration::from_secs(5),
    );
    provider.seed_access_token(access_token.to_owned(), far_future);
    provider
}

pub(crate) fn instant_backoff() -> BackoffPolicy {
    BackoffPolicy {
        base_delay: Duration::from_millis(1),
        max_attempts: 3,
    }
}

pub(crate) use crate::test_support::test_now;

pub(crate) fn seed_provider(base: &str, access_token: &str) -> GoogleCalendarSync {
    provider_with_token("http://127.0.0.1:1/token", base, access_token)
}

/// JSON [`mock_server`] + [`seed_provider`], as one call.
pub(crate) fn provider_env(
    responses: Vec<(u16, String)>,
    access_token: &str,
) -> (
    GoogleCalendarSync,
    std::thread::JoinHandle<Vec<RecordedRequest>>,
) {
    let (base, handle) = mock_server(responses);
    let provider = seed_provider(&base, access_token);
    (provider, handle)
}

pub(crate) fn week_window() -> (DateTime<Utc>, DateTime<Utc>) {
    (
        Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("window start"),
        Utc.with_ymd_and_hms(2026, 7, 22, 0, 0, 0)
            .single()
            .expect("window end"),
    )
}

/// A chunk event payload — "Gym", 2026-07-15 18:00–19:00 UTC — with a
/// caller-supplied `chunk_id` and `description`. `batch_tests` and `write_tests`
/// each wrap it with their own default description.
pub(crate) fn chunk_payload(chunk_id: &str, description: &str) -> ChunkEventPayload {
    ChunkEventPayload {
        chunk_id: chunk_id.to_owned(),
        title: "Gym".to_owned(),
        description: description.to_owned(),
        start: Utc
            .with_ymd_and_hms(2026, 7, 15, 18, 0, 0)
            .single()
            .expect("start"),
        end: Utc
            .with_ymd_and_hms(2026, 7, 15, 19, 0, 0)
            .single()
            .expect("end"),
    }
}

/// The canonical mixed op list for the batch-push tests: create + update
/// `evt-u` (both carrying `payload`), then delete `evt-d`.
pub(crate) fn cud_ops(payload: &ChunkEventPayload) -> Vec<SyncOp> {
    vec![
        SyncOp::Create(payload.clone()),
        SyncOp::Update {
            event_id: "evt-u".to_owned(),
            payload: payload.clone(),
        },
        SyncOp::Delete {
            event_id: "evt-d".to_owned(),
        },
    ]
}

/// Expected results for [`cud_ops`] when the mock returns `evt-c`/`evt-u`
/// with etags `e-c`/`e-u`.
pub(crate) fn cud_results() -> Vec<SyncOpResult> {
    vec![
        SyncOpResult::Created {
            chunk_id: "chunk-42".to_owned(),
            event_id: "evt-c".to_owned(),
            etag: Some("\"e-c\"".to_owned()),
        },
        SyncOpResult::Updated {
            chunk_id: "chunk-42".to_owned(),
            event_id: "evt-u".to_owned(),
            etag: Some("\"e-u\"".to_owned()),
        },
        SyncOpResult::Deleted,
    ]
}

/// Single-response `multipart/mixed` mock for the canonical
/// create+update+delete batch ([`cud_ops`]): `evt-c`/`evt-u` created/updated then
/// a 204 delete, all framed under `boundary`. Returns the seeded provider plus the
/// recorded-request handle.
pub(crate) fn cud_batch_env(
    boundary: &str,
    access_token: &str,
) -> (
    GoogleCalendarSync,
    std::thread::JoinHandle<Vec<RecordedRequest>>,
) {
    let resp = batch_body(
        boundary,
        &[
            ok_event_part(r#"{"id":"evt-c","etag":"\"e-c\""}"#),
            ok_event_part(r#"{"id":"evt-u","etag":"\"e-u\""}"#),
            ("HTTP/1.1 204 No Content", "", ""),
        ],
    );
    let (base, handle) = mock_server_ct(vec![(
        200,
        format!("multipart/mixed; boundary={boundary}"),
        resp,
    )]);
    let provider = seed_provider(&base, access_token);
    (provider, handle)
}

/// Unwrap an [`AppError::CalendarSync`] and return its message string.
/// Used by inline error-path tests that have no mock-server handle to join.
pub(crate) fn unwrap_calendar_sync_err(err: crate::error::AppError) -> String {
    match err {
        crate::error::AppError::CalendarSync(msg) => msg,
        other => panic!("expected CalendarSync, got {other:?}"),
    }
}

pub(crate) fn assert_cud_batch_single_post(results: &[SyncOpResult], recorded: &[RecordedRequest]) {
    assert_eq!(results, cud_results().as_slice());
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, "POST");
    assert_eq!(recorded[0].path_with_query, "/batch/calendar/v3");
}
