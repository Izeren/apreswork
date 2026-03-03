// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use chrono::{DateTime, TimeZone as _, Utc};

use test_case::test_case;
use url::Url;

use super::{GoogleCalendarSync, GoogleCredentials, GoogleEndpoints};
use crate::calendar::google_http::test_support::mock_keyring;
use crate::calendar::google_token::test_support::fail_entry;
use crate::calendar::google_token::{KeyringStore, PersistedCredential};
use crate::traits::calendar_sync::{AuthStatus, CalendarSync};

const AUTH_TIMEOUT_DEFAULT: Duration = Duration::from_secs(5);
const AUTH_TIMEOUT_SHORT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn test_creds() -> GoogleCredentials {
    GoogleCredentials {
        client_id: "test-client-id".to_owned(),
        client_secret: "test-secret".to_owned(),
    }
}

fn test_provider(keyring: KeyringStore, token_url: &str) -> GoogleCalendarSync {
    test_provider_with_timeout(keyring, token_url, AUTH_TIMEOUT_DEFAULT)
}

fn test_provider_with_timeout(
    keyring: KeyringStore,
    token_url: &str,
    auth_timeout: Duration,
) -> GoogleCalendarSync {
    GoogleCalendarSync::with_mock_keyring(
        test_creds(),
        keyring,
        GoogleEndpoints {
            auth_url: "http://127.0.0.1:1/auth".to_owned(),
            token_url: token_url.to_owned(),
            api_base_url: "http://127.0.0.1:1/api".to_owned(),
        },
        auth_timeout,
    )
}

/// Returns a keyring and provider backed by a mock token endpoint that returns `{}`.
fn test_provider_with_empty_ok_mock() -> (KeyringStore, GoogleCalendarSync) {
    let keyring = mock_keyring();
    let (token_url, _handle) = mock_token_endpoint("{}", 200);
    let provider = test_provider(keyring.clone(), &token_url);
    (keyring, provider)
}

/// Returns a keyring and provider pointing at an unreachable endpoint (port 1).
///
/// Use when the test never triggers a network call — disconnect semantics, cache
/// hits, auth-status reads, or calls that are expected to return a `CalendarSync`
/// error without any HTTP round-trip.
fn dead_provider() -> (KeyringStore, GoogleCalendarSync) {
    let keyring = mock_keyring();
    let provider = test_provider(keyring.clone(), "http://127.0.0.1:1/token");
    (keyring, provider)
}

fn seed_expired_token(keyring: &KeyringStore) {
    keyring
        .save(&PersistedCredential {
            refresh_token: Some("rt".to_owned()),
            expires_at: crate::test_support::test_now() - chrono::Duration::hours(1),
        })
        .expect("seed expired token");
}

fn future_expiry() -> DateTime<Utc> {
    crate::test_support::test_now() + chrono::Duration::hours(2)
}

fn far_future() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2999, 1, 1, 0, 0, 0)
        .single()
        .expect("far future")
}

/// Build a provider + mock token endpoint that returns the standard refresh response JSON.
///
/// The response body has a configurable `access_token` value so each test can assert
/// a distinct token, while the common fields (`refresh_token`, `expires_in`, `token_type`)
/// avoid repeated inline JSON literals.
fn provider_with_refresh_mock(
    keyring: KeyringStore,
    access_token: &str,
) -> (GoogleCalendarSync, std::thread::JoinHandle<Option<String>>) {
    let body = format!(
        r#"{{"access_token":"{access_token}","refresh_token":"rt","expires_in":3600,"token_type":"Bearer"}}"#
    );
    let (token_url, mock_handle) = mock_token_endpoint(&body, 200);
    let provider = test_provider(keyring, &token_url);
    (provider, mock_handle)
}

fn assert_calendar_sync_error(provider: &GoogleCalendarSync, needle: &str) {
    let err = provider
        .access_token(crate::test_support::test_now(), false)
        .unwrap_err();
    match &err {
        crate::error::AppError::CalendarSync(msg) => {
            assert!(msg.contains(needle), "error must mention {needle:?}: {msg}");
        }
        other => panic!("expected CalendarSync error, got: {other:?}"),
    }
}

fn assert_refresh_error_contains(
    keyring: &KeyringStore,
    provider: &GoogleCalendarSync,
    needle: &str,
) {
    seed_expired_token(keyring);
    assert_calendar_sync_error(provider, needle);
}

fn mock_token_endpoint(
    body: &str,
    status: u16,
) -> (String, std::thread::JoinHandle<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock listener");
    let port = listener.local_addr().expect("mock addr").port();
    let url = format!("http://127.0.0.1:{port}/token");
    let body = body.to_owned();

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().ok()?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).ok()?;
        let request = String::from_utf8_lossy(&buf[..n]).to_string();

        let request_body = request
            .find("\r\n\r\n")
            .map(|i| request[i + 4..].trim_end_matches('\0').to_owned());

        let status_line = if status == 200 {
            "200 OK"
        } else {
            "400 Bad Request"
        };
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).ok();
        request_body
    });

    (url, handle)
}

fn extract_state_and_port(consent_url: &str) -> (String, u16) {
    let parsed = Url::parse(consent_url).expect("parse consent URL");
    let mut state = String::new();
    let mut port = 0u16;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "state" => state = v.into_owned(),
            "redirect_uri" => {
                let redirect = Url::parse(v.as_ref()).expect("parse redirect_uri");
                port = redirect.port().expect("redirect port");
            }
            _ => {}
        }
        if !state.is_empty() && port > 0 {
            break;
        }
    }
    assert!(!state.is_empty(), "state missing from consent URL");
    assert!(port > 0, "redirect port missing from consent URL");
    (state, port)
}

fn send_redirect(port: u16, query: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to loopback listener");
    let request = format!("GET /?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .expect("write redirect");
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).expect("read redirect response");
    String::from_utf8_lossy(&buf[..n]).to_string()
}

fn wait_for_status_change(provider: &GoogleCalendarSync, max_wait: Duration) -> AuthStatus {
    provider.wait_status_change(max_wait)
}

fn run_exchange_assert_not_connected(provider: &GoogleCalendarSync, msg: &str) {
    let consent_url = provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    let (state, port) = extract_state_and_port(&consent_url);
    let redirect_query = format!("state={state}&code=4%2Fcode");
    send_redirect(port, &redirect_query);
    let final_status = wait_for_status_change(provider, AUTH_TIMEOUT_DEFAULT);
    assert_eq!(final_status, AuthStatus::NotConnected, "{msg}");
}

#[test]
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
fn begin_auth_returns_url_with_required_params() {
    let (_keyring, provider) = test_provider_with_empty_ok_mock();

    let consent_url = provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    let parsed = Url::parse(&consent_url).expect("parse consent URL");

    assert!(
        consent_url.starts_with("http://127.0.0.1:1/auth"),
        "unexpected base: {consent_url}"
    );

    let params: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
    assert!(params.contains_key("client_id"), "missing client_id");
    assert!(
        params.contains_key("code_challenge"),
        "missing code_challenge"
    );
    assert_eq!(
        params
            .get("code_challenge_method")
            .map(std::convert::AsRef::as_ref),
        Some("S256"),
        "code_challenge_method must be S256"
    );
    assert!(params.contains_key("state"), "missing state");
    assert_eq!(
        params.get("access_type").map(std::convert::AsRef::as_ref),
        Some("offline")
    );
    assert_eq!(
        params.get("prompt").map(std::convert::AsRef::as_ref),
        Some("consent")
    );

    let scope_str = params
        .get("scope")
        .map(|s| s.as_ref().to_owned())
        .unwrap_or_default();
    assert!(
        scope_str.contains("calendar.app.created"),
        "missing app.created scope"
    );
    assert!(
        scope_str.contains("calendar.events"),
        "missing events scope"
    );
    assert!(
        !scope_str.contains("calendar.events.readonly"),
        "events scope must be read/write (calendar.events), not readonly"
    );
    assert!(
        scope_str.contains("calendar.calendarlist.readonly"),
        "missing calendarlist scope"
    );

    let redirect_uri = params
        .get("redirect_uri")
        .expect("redirect_uri missing")
        .as_ref();
    let redirect_parsed = Url::parse(redirect_uri).expect("parse redirect_uri");
    assert_eq!(
        redirect_parsed.host_str(),
        Some("127.0.0.1"),
        "redirect must be loopback"
    );
    assert!(
        redirect_parsed.port().unwrap_or(0) > 0,
        "redirect port must be set"
    );

    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::Pending
    );
}

#[test]
fn happy_path_connects_and_saves_token() {
    let keyring = mock_keyring();
    let mock_body =
        r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}"#;
    let (token_url, mock_handle) = mock_token_endpoint(mock_body, 200);
    let provider = test_provider(keyring.clone(), &token_url);

    let consent_url = provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    let (state, port) = extract_state_and_port(&consent_url);

    let redirect_query = format!("state={state}&code=4%2Fcode");
    let response = send_redirect(port, &redirect_query);
    assert!(
        response.contains("200"),
        "redirect response must be 200: {response}"
    );
    assert!(
        response.contains("close this tab"),
        "redirect response must have close-tab message: {response}"
    );
    assert!(
        response.contains("charset=utf-8"),
        "redirect response must declare UTF-8 (em dash renders as mojibake without it): {response}"
    );

    let final_status = wait_for_status_change(&provider, AUTH_TIMEOUT_DEFAULT);
    assert_eq!(
        final_status,
        AuthStatus::Connected { email: None },
        "expected Connected after happy path"
    );

    // Refresh token must be persisted in the keyring (no token file).
    let stored = keyring.load().expect("load").expect("Some");
    assert_eq!(stored.refresh_token.as_deref(), Some("rt-1"));

    // Access token must be in the in-memory cache (not in the keyring).
    // cached_access_token() bypasses the expiry check: the exchange worker
    // uses Utc::now() for expires_at, which predates test_now() (2030).
    assert_eq!(
        provider.cached_access_token().as_deref(),
        Some("at-1"),
        "access token must be cached after exchange"
    );

    let req_body = mock_handle.join().expect("mock join").unwrap_or_default();
    assert!(
        req_body.contains("code_verifier"),
        "exchange POST must include code_verifier: {req_body}"
    );
}

#[test_case("state=WRONG_STATE&code=4%2Fcode" ; "wrong_state")]
#[test_case("error=access_denied" ; "error_param")]
#[test_case("state={state}&noop=1" ; "missing_code")]
fn failure_redirect_leaves_not_connected(redirect_suffix: &str) {
    let keyring = mock_keyring();
    let (token_url, mock_handle) = mock_token_endpoint("{}", 200);
    let provider = test_provider_with_timeout(keyring.clone(), &token_url, AUTH_TIMEOUT_SHORT);

    let consent_url = provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    let (state, port) = extract_state_and_port(&consent_url);

    let query = redirect_suffix.replace("{state}", &state);
    send_redirect(port, &query);

    let final_status = wait_for_status_change(&provider, Duration::from_secs(3));
    assert_eq!(
        final_status,
        AuthStatus::NotConnected,
        "failure redirect must leave NotConnected"
    );
    assert!(
        keyring.load().expect("load").is_none(),
        "no keyring credential should be saved on failure"
    );

    let contacted = mock_handle.is_finished();
    if redirect_suffix.contains("WRONG_STATE") || redirect_suffix.contains("error=") {
        assert!(
            !contacted,
            "token endpoint must NOT be contacted on CSRF/error failure"
        );
    }
}

#[test]
fn timeout_leaves_not_connected() {
    let keyring = mock_keyring();
    let provider = test_provider_with_timeout(keyring, "http://127.0.0.1:1/token", POLL_INTERVAL);

    provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    let final_status = wait_for_status_change(&provider, AUTH_TIMEOUT_SHORT);
    assert_eq!(
        final_status,
        AuthStatus::NotConnected,
        "timed-out flow must leave NotConnected"
    );
}

#[test]
fn superseded_flow_does_not_connect() {
    let keyring = mock_keyring();

    let mock_body = r#"{"access_token":"at-super","refresh_token":"rt-super","expires_in":3600,"token_type":"Bearer"}"#;
    let (token_url1, mock1) = mock_token_endpoint(mock_body, 200);
    let (_, _mock2) = mock_token_endpoint(mock_body, 200);

    let provider = test_provider(keyring.clone(), &token_url1);

    let url1 = provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth #1");
    let (state1, port1) = extract_state_and_port(&url1);

    provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth #2");

    send_redirect(port1, &format!("state={state1}&code=4%2Fcode"));

    let _ = mock1.join();
    std::thread::sleep(Duration::from_millis(200));

    assert!(
        keyring.load().expect("load").is_none(),
        "stale flow must not produce a keyring credential"
    );
}

#[test]
fn disconnect_during_pending_clears_state() {
    let (keyring, provider) = dead_provider();

    provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::Pending
    );
    provider.disconnect().expect("disconnect during pending");
    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::NotConnected
    );
    assert!(
        keyring.load().expect("load").is_none(),
        "keyring must be empty after disconnect"
    );
}

#[test]
fn disconnect_when_already_disconnected_is_ok() {
    let (_, provider) = dead_provider();
    provider.disconnect().expect("disconnect (no-op)");
    provider
        .disconnect()
        .expect("second disconnect (idempotent)");
    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::NotConnected
    );
}

#[test]
fn disconnect_clears_cache_and_keyring() {
    let (keyring, provider) = dead_provider();
    keyring
        .save(&PersistedCredential {
            refresh_token: Some("rt".to_owned()),
            expires_at: far_future(),
        })
        .expect("seed");
    provider.seed_access_token("at".to_owned(), far_future());

    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::Connected { email: None }
    );
    provider.disconnect().expect("disconnect");
    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::NotConnected
    );
    assert!(
        keyring.load().expect("load after disconnect").is_none(),
        "keyring must be empty after disconnect"
    );
}

#[test]
fn access_token_nearly_expired_cache_triggers_refresh() {
    let keyring = mock_keyring();
    let (provider, mock_handle) = provider_with_refresh_mock(keyring.clone(), "fresh-at");

    // Seed a cache token that expires exactly at test_now() — within the 60s margin.
    provider.seed_access_token("stale-at".to_owned(), crate::test_support::test_now());
    keyring
        .save(&PersistedCredential {
            refresh_token: Some("rt".to_owned()),
            expires_at: crate::test_support::test_now() + chrono::Duration::hours(1),
        })
        .expect("seed keyring");

    // Cache is present but expires_at <= now + 60s → must refresh.
    let at = provider
        .access_token(crate::test_support::test_now(), false)
        .expect("access_token (cache expired)");
    assert_eq!(at, "fresh-at");

    mock_handle.join().expect("mock join");
}

#[test]
fn access_token_refresh_preserves_old_refresh_token() {
    let keyring = mock_keyring();
    let mock_body = r#"{"access_token":"refreshed-at","expires_in":3600,"token_type":"Bearer"}"#;
    // Response deliberately omits `refresh_token` — old one must be kept.
    let (token_url, mock_handle) = mock_token_endpoint(mock_body, 200);
    let provider = test_provider(keyring.clone(), &token_url);

    keyring
        .save(&PersistedCredential {
            refresh_token: Some("keep-me".to_owned()),
            expires_at: crate::test_support::test_now() - chrono::Duration::hours(1),
        })
        .expect("seed expired credential");

    let new_at = provider
        .access_token(crate::test_support::test_now(), false)
        .expect("access_token refresh");
    assert_eq!(new_at, "refreshed-at");

    let saved = keyring.load().expect("load").expect("Some");
    assert_eq!(saved.refresh_token.as_deref(), Some("keep-me"));

    mock_handle.join().expect("mock join");
}

#[test]
fn access_token_refresh_updates_cache() {
    let keyring = mock_keyring();
    let mock_body =
        r#"{"access_token":"new-at","refresh_token":"rt","expires_in":3600,"token_type":"Bearer"}"#;
    let (token_url, mock_handle) = mock_token_endpoint(mock_body, 200);
    let provider = test_provider(keyring.clone(), &token_url);

    seed_expired_token(&keyring);

    let at = provider
        .access_token(crate::test_support::test_now(), false)
        .expect("access_token");
    assert_eq!(at, "new-at");

    // The provider that refreshed must have its own cache populated.
    assert_eq!(
        provider.cached_access_token().as_deref(),
        Some("new-at"),
        "cache must be updated on the provider that performed the refresh"
    );

    mock_handle.join().expect("mock join");
}

#[test]
fn access_token_fresh_cache_returns_without_http_call() {
    let (_, provider) = dead_provider();

    // Seed the in-memory cache directly — no HTTP needed.
    provider.seed_access_token("cached-at".to_owned(), future_expiry());

    let at = provider
        .access_token(crate::test_support::test_now(), false)
        .expect("access_token (cache hit)");
    assert_eq!(at, "cached-at");
}

#[test]
fn access_token_force_refresh_bypasses_cache() {
    let keyring = mock_keyring();
    let (provider, mock_handle) = provider_with_refresh_mock(keyring.clone(), "forced-at");

    // Seed a valid cache entry AND a keyring credential.
    provider.seed_access_token("stale-at".to_owned(), future_expiry());
    keyring
        .save(&PersistedCredential {
            refresh_token: Some("rt".to_owned()),
            expires_at: future_expiry(),
        })
        .expect("seed");

    let at = provider
        .access_token(crate::test_support::test_now(), true)
        .expect("forced refresh");
    assert_eq!(at, "forced-at");

    mock_handle.join().expect("mock join");
}

#[test]
fn access_token_refresh_missing_refresh_token_returns_err() {
    let (keyring, provider) = test_provider_with_empty_ok_mock();

    keyring
        .save(&PersistedCredential {
            refresh_token: None, // no refresh token
            expires_at: crate::test_support::test_now() - chrono::Duration::hours(1),
        })
        .expect("seed credential without refresh token");

    assert_calendar_sync_error(&provider, "reconnect");
}

#[test]
fn access_token_no_credential_returns_err() {
    let (_, provider) = dead_provider();
    let err = provider
        .access_token(crate::test_support::test_now(), false)
        .unwrap_err();
    assert!(matches!(err, crate::error::AppError::CalendarSync(_)));
}

#[test_case(false ; "without_credential")]
#[test_case(true ; "with_credential")]
fn is_available(should_have_cred: bool) {
    let (keyring, provider) = dead_provider();
    if should_have_cred {
        keyring
            .save(&PersistedCredential {
                refresh_token: Some("rt".to_owned()),
                expires_at: future_expiry(),
            })
            .expect("seed credential");
    }
    assert_eq!(
        provider.is_available(),
        should_have_cred,
        "is_available must be {should_have_cred}"
    );
}

#[test]
fn is_available_true_when_cache_populated() {
    // No keyring credential — but cache is pre-seeded.
    let (_, provider) = dead_provider();
    provider.seed_access_token("at".to_owned(), future_expiry());
    assert!(provider.is_available(), "cache-only → still available");
}

#[test]
fn auth_status_keyring_error_returns_not_connected() {
    let keyring = KeyringStore::with_mock_entry(fail_entry(true, false, false));
    let provider = test_provider(keyring, "http://127.0.0.1:1/token");
    // Keyring failure must be silently ignored and treated as not connected.
    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::NotConnected
    );
}

#[test]
fn auth_status_connected_when_cache_populated() {
    let (_, provider) = dead_provider();
    provider.seed_access_token("at".to_owned(), future_expiry());
    assert_eq!(
        provider.auth_status(crate::test_support::test_instant_now()),
        AuthStatus::Connected { email: None },
        "in-memory cache → Connected without touching keyring"
    );
}

#[test]
fn access_token_refresh_http_error_returns_err() {
    let keyring = mock_keyring();
    let (token_url, _mock) = mock_token_endpoint("{\"error\":\"invalid_grant\"}", 400);
    let provider = test_provider(keyring.clone(), &token_url);
    assert_refresh_error_contains(&keyring, &provider, "400");
}

#[test]
fn exchange_code_failure_leaves_not_connected() {
    let keyring = mock_keyring();
    let (token_url, _mock) = mock_token_endpoint("{\"error\":\"invalid_code\"}", 400);
    let provider = test_provider(keyring.clone(), &token_url);

    run_exchange_assert_not_connected(&provider, "exchange failure must leave NotConnected");
    assert!(
        keyring.load().expect("load").is_none(),
        "no keyring credential on exchange failure"
    );
}

#[test]
fn exchange_success_but_save_fails_leaves_not_connected() {
    // Token exchange succeeds but the keyring save fails, exercising
    // the log::warn branch in finish_flow.
    let keyring = KeyringStore::with_mock_entry(fail_entry(false, true, false));

    let mock_body =
        r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}"#;
    let (token_url, _mock) = mock_token_endpoint(mock_body, 200);
    let provider = test_provider(keyring, &token_url);

    run_exchange_assert_not_connected(
        &provider,
        "exchange succeeded but save failed → must be NotConnected",
    );
}

#[test]
fn list_calendars_returns_calendar_sync_error() {
    let (_, provider) = dead_provider();
    let err = provider
        .list_calendars(crate::test_support::test_now())
        .unwrap_err();
    assert!(matches!(err, crate::error::AppError::CalendarSync(_)));
}

#[test]
fn list_events_returns_calendar_sync_error() {
    let (_, provider) = dead_provider();
    let now = crate::test_support::utc(2026, 7, 11, 0, 0);
    let later = crate::test_support::utc(2026, 7, 12, 0, 0);
    let err = provider.list_events(now, "any", now, later).unwrap_err();
    assert!(matches!(err, crate::error::AppError::CalendarSync(_)));
}

#[test]
fn compiled_without_env_vars_returns_none() {
    assert!(
        GoogleCredentials::compiled().is_none(),
        "compiled() must return None when env vars are absent at compile time"
    );
}

#[test]
fn access_token_refresh_network_error_returns_err() {
    let (keyring, provider) = dead_provider();
    assert_refresh_error_contains(&keyring, &provider, "request failed");
}

#[test]
fn access_token_refresh_json_error_returns_err() {
    let keyring = mock_keyring();
    let (token_url, _mock) = mock_token_endpoint("{\"not_a_token\": true}", 200);
    let provider = test_provider(keyring.clone(), &token_url);
    assert_refresh_error_contains(&keyring, &provider, "parse error");
}

#[test]
fn wait_for_status_change_returns_pending_on_timeout() {
    let keyring = mock_keyring();
    let provider = test_provider_with_timeout(
        keyring,
        "http://127.0.0.1:1/token",
        Duration::from_millis(100),
    );
    provider
        .begin_auth(
            crate::test_support::test_now(),
            crate::test_support::test_instant_now(),
        )
        .expect("begin_auth");
    let status = wait_for_status_change(&provider, Duration::ZERO);
    assert_eq!(status, AuthStatus::Pending);
}
