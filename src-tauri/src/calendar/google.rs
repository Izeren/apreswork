// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Google `OAuth2` provider — loopback-redirect PKCE flow.
//!
//! All Google-specific details (`OAuth2` consent URL, token endpoint, credential
//! types, PKCE verifier, pending-flow state) are encapsulated here. Only
//! neutral trait types cross the boundary into the rest of the app.
//!
//! # Security
//!
//! Token contents are NEVER logged or included in error messages. Refresh tokens
//! are stored in the OS keyring; access tokens are memory-only (never written to
//! disk).

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse as _, TokenUrl,
};

/// `BasicClient` configured with both auth and token endpoints.
type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;
use url::Url;

use crate::calendar::google_http;
use crate::calendar::google_token::{KeyringStore, PersistedCredential, StoredToken};
use crate::error::AppError;
use crate::traits::calendar_sync::{
    AuthStatus, CalendarSync, ExternalCalendar, ExternalEvent, RemoteChunkEvent, SyncOp,
    SyncOpResult, UserEventPayload,
};

/// Client credentials compiled into the binary at build time.
///
/// For Google "Desktop" app type the `client_secret` is not truly secret —
/// it is embedded in every distributed copy of the binary, which is
/// industry-standard for desktop OAuth apps (DESIGN.md §4.3).
#[derive(Clone)]
pub struct GoogleCredentials {
    /// `OAuth2` client ID.
    pub client_id: String,
    /// `OAuth2` client secret (not confidential for desktop apps).
    pub client_secret: String,
}

impl GoogleCredentials {
    /// Read credentials from compile-time environment variables.
    ///
    /// Returns `None` when either variable is absent or empty (provider
    /// unavailable).
    #[must_use]
    pub fn compiled() -> Option<Self> {
        let id = option_env!("APRESWORK_GOOGLE_CLIENT_ID")
            .unwrap_or("")
            .trim();
        let secret = option_env!("APRESWORK_GOOGLE_CLIENT_SECRET")
            .unwrap_or("")
            .trim();
        if id.is_empty() || secret.is_empty() {
            None
        } else {
            Some(Self {
                client_id: id.to_owned(),
                client_secret: secret.to_owned(),
            })
        }
    }
}

/// `OAuth2` and Calendar API endpoint URLs; injectable so tests never touch Google.
#[derive(Clone)]
pub struct GoogleEndpoints {
    /// Authorization endpoint (consent page URL base).
    pub auth_url: String,
    /// Token endpoint (code → token exchange and refresh).
    pub token_url: String,
    /// Google Calendar REST API v3 base URL (without trailing slash).
    pub api_base_url: String,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            token_url: "https://oauth2.googleapis.com/token".to_owned(),
            api_base_url: "https://www.googleapis.com/calendar/v3".to_owned(),
        }
    }
}

// ── Pending auth state ────────────────────────────────────────────────────────

/// Shared slot for the currently-active (if any) loopback flow.
struct PendingSlot {
    current: Option<PendingAuth>,
    /// Monotonically incrementing generation counter. A new `begin_auth` call
    /// bumps this, superseding any prior in-flight flow.
    generation: u64,
}

/// Metadata for an in-flight flow (no tokens or PKCE verifiers here — those
/// move into the worker thread at spawn time).
struct PendingAuth {
    generation: u64,
    deadline: Instant,
}

/// In-memory access-token cache. Never written to disk.
struct CachedToken {
    access_token: String,
    expires_at: DateTime<Utc>,
}

/// JSON shape returned by the Google token refresh endpoint.
#[derive(serde::Deserialize)]
struct RefreshResponse {
    access_token: String,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
}

/// Google Calendar sync provider.
///
/// Thread-safe: `pending` and `cached_token` are guarded by `Mutex`; all other
/// fields are either immutable after construction or `Clone + Send`.
//
// CalendarSync suffix repeats the module name; allowed per the trait's own
// module_name_repetitions allow on the trait definition.
#[allow(clippy::module_name_repetitions)]
pub struct GoogleCalendarSync {
    creds: GoogleCredentials,
    keyring: KeyringStore,
    endpoints: GoogleEndpoints,
    auth_timeout: Duration,
    http: reqwest::blocking::Client,
    pending: Arc<Mutex<PendingSlot>>,
    pending_changed: Arc<Condvar>,
    cached_token: Arc<Mutex<Option<CachedToken>>>,
}

impl GoogleCalendarSync {
    /// Construct with production defaults (real Google endpoints, 5-min timeout).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::CalendarSync`] if the platform keyring rejects the
    /// derived service or username values (e.g., exceeds length limits).
    pub fn new(creds: GoogleCredentials, token_path: &Path) -> Result<Self, AppError> {
        let keyring = KeyringStore::for_token_path(token_path)?;
        Ok(Self::build(
            creds,
            keyring,
            GoogleEndpoints::default(),
            Duration::from_secs(300),
        ))
    }

    /// Internal constructor shared by the production and test paths.
    ///
    /// # Panics
    ///
    /// Panics if `reqwest::blocking::Client` cannot be built (TLS init failure).
    fn build(
        creds: GoogleCredentials,
        keyring: KeyringStore,
        endpoints: GoogleEndpoints,
        auth_timeout: Duration,
    ) -> Self {
        let http = reqwest::blocking::Client::builder()
            // SSRF prevention: do not follow redirects automatically
            // (oauth2 crate doc requirement).
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build blocking HTTP client");
        Self {
            creds,
            keyring,
            endpoints,
            auth_timeout,
            http,
            pending: Arc::new(Mutex::new(PendingSlot {
                current: None,
                generation: 0,
            })),
            pending_changed: Arc::new(Condvar::new()),
            cached_token: Arc::new(Mutex::new(None)),
        }
    }

    /// Construct with injected keyring and endpoints (test only, no migration).
    #[cfg(test)]
    pub(crate) fn with_mock_keyring(
        creds: GoogleCredentials,
        keyring: KeyringStore,
        endpoints: GoogleEndpoints,
        auth_timeout: Duration,
    ) -> Self {
        Self::build(creds, keyring, endpoints, auth_timeout)
    }

    /// Construct with production defaults and an injected keyring (test only).
    #[cfg(test)]
    pub(crate) fn new_with_mock_keyring(creds: GoogleCredentials, keyring: KeyringStore) -> Self {
        Self::build(
            creds,
            keyring,
            GoogleEndpoints::default(),
            Duration::from_secs(300),
        )
    }

    /// Pre-populate the in-memory access-token cache (test only).
    ///
    /// Used by test helpers that need a "connected" provider without going
    /// through an HTTP refresh grant.
    #[cfg(test)]
    pub(crate) fn seed_access_token(&self, access_token: String, expires_at: DateTime<Utc>) {
        let mut cache = self.cached_token.lock().expect("cache lock in seed");
        *cache = Some(CachedToken {
            access_token,
            expires_at,
        });
    }

    #[cfg(test)]
    pub(crate) fn keyring(&self) -> &crate::calendar::google_token::KeyringStore {
        &self.keyring
    }

    /// Return the cached access token without any expiry check (test-only).
    #[cfg(test)]
    pub(crate) fn cached_access_token(&self) -> Option<String> {
        self.cached_token
            .lock()
            .expect("cache lock in cached_access_token")
            .as_ref()
            .map(|ct| ct.access_token.clone())
    }

    /// Return the current access token, refreshing if needed.
    ///
    /// Serves from the in-memory cache when the cached token has more than 60 s
    /// remaining. On a cache miss, loads the refresh token from the keyring and
    /// performs an HTTP refresh grant. Pass `force_refresh: true` on a 401 retry
    /// to bypass the cache and always fetch a new token.
    ///
    /// # Errors
    ///
    /// `CalendarSync("reconnect required …")` when there is no keyring credential
    /// or the credential has no refresh token.
    pub(crate) fn access_token(
        &self,
        now: DateTime<Utc>,
        force_refresh: bool,
    ) -> Result<String, AppError> {
        if !force_refresh {
            let cache = self
                .cached_token
                .lock()
                .expect("cache lock in access_token");
            if let Some(ref ct) = *cache {
                let margin = chrono::Duration::seconds(60);
                if ct.expires_at > now + margin {
                    return Ok(ct.access_token.clone());
                }
            }
        }

        let cred = self
            .keyring
            .load()?
            .ok_or_else(|| AppError::CalendarSync("reconnect required: no stored token".into()))?;

        self.refresh(cred, now)
    }

    /// Perform a token refresh grant against `endpoints.token_url`.
    ///
    /// Preserves the old refresh token when the response omits it (Google's
    /// behaviour on some re-consent scenarios). Persists the new credential to
    /// the keyring and updates the in-memory cache.
    fn refresh(&self, cred: PersistedCredential, now: DateTime<Utc>) -> Result<String, AppError> {
        let started = std::time::Instant::now();
        let refresh_token = cred.refresh_token.ok_or_else(|| {
            AppError::CalendarSync(
                "reconnect required: refresh token missing — disconnect and reconnect".into(),
            )
        })?;

        let params = [
            ("client_id", self.creds.client_id.as_str()),
            ("client_secret", self.creds.client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .http
            .post(&self.endpoints.token_url)
            .form(&params)
            .send()
            .map_err(|e| {
                AppError::CalendarSync(format!(
                    "token refresh request failed: {}",
                    e.status()
                        .map_or_else(|| "network error".to_owned(), |s| s.to_string())
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(AppError::CalendarSync(format!(
                "token refresh failed with HTTP {status}"
            )));
        }

        let resp: RefreshResponse = response.json().map_err(|e| {
            AppError::CalendarSync(format!("token refresh response parse error: {e}"))
        })?;

        let expires_at = now + chrono::Duration::seconds(resp.expires_in.unwrap_or(3600));
        let new_refresh_token = resp.refresh_token.or(Some(refresh_token));

        self.keyring.save(&PersistedCredential {
            refresh_token: new_refresh_token,
            expires_at,
        })?;

        {
            let mut cache = self.cached_token.lock().expect("cache lock in refresh");
            *cache = Some(CachedToken {
                access_token: resp.access_token.clone(),
                expires_at,
            });
        }

        log::info!(
            "google: token refresh took {}ms",
            started.elapsed().as_millis()
        );
        Ok(resp.access_token)
    }

    /// Borrow the underlying HTTP client (used by the REST client module).
    pub(crate) fn http(&self) -> &reqwest::blocking::Client {
        &self.http
    }

    /// Borrow the configured endpoints (used by the REST client module).
    pub(crate) fn endpoints(&self) -> &GoogleEndpoints {
        &self.endpoints
    }

    fn has_cached_token(&self) -> bool {
        self.cached_token.lock().expect("cache lock").is_some()
    }

    /// Block until `auth_status` is no longer `Pending`, or `max_wait` elapses.
    ///
    /// Uses a condvar paired with the pending-slot mutex — no busy-poll, no
    /// `Instant::now()` calls in the caller.
    #[cfg(test)]
    pub(crate) fn wait_status_change(&self, max_wait: Duration) -> AuthStatus {
        {
            let guard = self
                .pending
                .lock()
                .expect("pending slot lock in wait_status_change");
            let (_guard, _timed_out) = self
                .pending_changed
                .wait_timeout_while(guard, max_wait, |slot| slot.current.is_some())
                .expect("condvar wait in wait_status_change");
        }
        self.auth_status(crate::test_support::test_instant_now())
    }
}

impl CalendarSync for GoogleCalendarSync {
    // TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
    #[allow(clippy::too_many_lines)]
    fn begin_auth(&self, now: DateTime<Utc>, now_instant: Instant) -> Result<String, AppError> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
            AppError::CalendarSync(format!("cannot bind loopback auth listener: {e}"))
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::CalendarSync(format!("cannot read loopback port: {e}")))?
            .port();

        let redirect_uri = format!("http://127.0.0.1:{port}");

        let oauth_client = build_oauth_client(&self.creds, &self.endpoints, &redirect_uri)
            .map_err(|e| AppError::CalendarSync(format!("cannot build auth URL: {e}")))?;

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let (consent_url, csrf_token) = oauth_client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.app.created".to_owned(),
            ))
            // Read/write events on the user's own calendars: reads mirror busy
            // blocks; writes back in-app create/edit/delete of user-owned events
            // (G11 write-through). Broader than `calendar.app.created`, which only
            // covers the dedicated app calendar.
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.events".to_owned(),
            ))
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar.calendarlist.readonly".to_owned(),
            ))
            // Backup file access: only files this app created (non-sensitive tier).
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/drive.file".to_owned(),
            ))
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("access_type", "offline")
            // `prompt=consent` ensures Google returns a refresh token (required
            // for the "Testing" publishing status where tokens expire after 7 days).
            .add_extra_param("prompt", "consent")
            .url();

        let deadline = now_instant + self.auth_timeout;
        let my_generation = {
            let mut slot = self
                .pending
                .lock()
                .expect("pending slot lock in begin_auth");
            slot.generation = slot.generation.wrapping_add(1);
            let gen = slot.generation;
            slot.current = Some(PendingAuth {
                generation: gen,
                deadline,
            });
            gen
        };

        // Clone everything the worker thread needs (no borrows across threads).
        let worker_creds = self.creds.clone();
        let worker_endpoints = self.endpoints.clone();
        let worker_http = self.http.clone();
        let worker_keyring = self.keyring.clone();
        let worker_pending = Arc::clone(&self.pending);
        let worker_pending_changed = Arc::clone(&self.pending_changed);
        let worker_cached_token = Arc::clone(&self.cached_token);

        let auth_timeout = self.auth_timeout;
        std::thread::spawn(move || {
            run_auth_worker(
                &listener,
                pkce_verifier,
                &csrf_token,
                my_generation,
                &redirect_uri,
                &worker_creds,
                &worker_endpoints,
                &worker_http,
                &worker_keyring,
                &worker_cached_token,
                &worker_pending,
                &worker_pending_changed,
                auth_timeout,
                now,
            );
        });

        Ok(consent_url.to_string())
    }

    fn auth_status(&self, now_instant: Instant) -> AuthStatus {
        {
            let mut slot = self
                .pending
                .lock()
                .expect("pending slot lock in auth_status");
            if let Some(ref pending_auth) = slot.current {
                if now_instant < pending_auth.deadline {
                    return AuthStatus::Pending;
                }
                // Deadline expired — clean up and fall through.
                slot.current = None;
            }
        }

        if self.has_cached_token() {
            return AuthStatus::Connected { email: None };
        }
        match self.keyring.load() {
            Ok(Some(_)) => AuthStatus::Connected { email: None },
            Ok(None) => AuthStatus::NotConnected,
            Err(e) => {
                log::warn!("auth_status: keyring read failed: {e}");
                AuthStatus::NotConnected
            }
        }
    }

    fn disconnect(&self) -> Result<(), AppError> {
        {
            let mut slot = self
                .pending
                .lock()
                .expect("pending slot lock in disconnect");
            slot.generation = slot.generation.wrapping_add(1);
            slot.current = None;
        }
        {
            let mut cache = self.cached_token.lock().expect("cache lock in disconnect");
            *cache = None;
        }
        self.keyring.delete()
    }

    fn is_available(&self) -> bool {
        self.has_cached_token() || matches!(self.keyring.load(), Ok(Some(_)))
    }

    fn list_calendars(&self, now: DateTime<Utc>) -> Result<Vec<ExternalCalendar>, AppError> {
        google_http::list_calendars(self, &default_backoff(), now)
    }

    fn list_events(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEvent>, AppError> {
        google_http::list_events(self, &default_backoff(), now, calendar_id, start, end)
    }

    fn ensure_app_calendar(&self, now: DateTime<Utc>) -> Result<String, AppError> {
        google_http::create_calendar(self, &default_backoff(), now)
    }

    fn list_app_calendar_events(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<RemoteChunkEvent>, AppError> {
        google_http::list_app_calendar_events(
            self,
            &default_backoff(),
            now,
            calendar_id,
            start,
            end,
        )
    }

    fn execute_sync_ops(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        ops: &[SyncOp],
    ) -> Result<Vec<SyncOpResult>, AppError> {
        google_http::batch_sync_ops(
            self,
            &default_backoff(),
            now,
            google_http::BATCH_MAX_OPS,
            calendar_id,
            ops,
        )
    }

    fn create_user_event(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError> {
        google_http::create_user_event(self, &default_backoff(), now, calendar_id, payload)
    }

    fn update_user_event(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        event_id: &str,
        payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError> {
        google_http::update_user_event(
            self,
            &default_backoff(),
            now,
            calendar_id,
            event_id,
            payload,
        )
    }

    fn delete_user_event(
        &self,
        now: DateTime<Utc>,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<(), AppError> {
        // A user-event delete is identical to a chunk-event delete — both are
        // idempotent deletes by event id, with no marker involved.
        google_http::delete_event(self, &default_backoff(), now, calendar_id, event_id)
    }
}

fn default_backoff() -> google_http::BackoffPolicy {
    google_http::BackoffPolicy::default()
}

/// Build the oauth2 `BasicClient` configured for Google.
fn build_oauth_client(
    creds: &GoogleCredentials,
    endpoints: &GoogleEndpoints,
    redirect_uri: &str,
) -> Result<ConfiguredClient, String> {
    let auth_url =
        AuthUrl::new(endpoints.auth_url.clone()).map_err(|e| format!("invalid auth URL: {e}"))?;
    let token_url = TokenUrl::new(endpoints.token_url.clone())
        .map_err(|e| format!("invalid token URL: {e}"))?;
    let redirect = RedirectUrl::new(redirect_uri.to_owned())
        .map_err(|e| format!("invalid redirect URI: {e}"))?;

    Ok(BasicClient::new(ClientId::new(creds.client_id.clone()))
        .set_client_secret(ClientSecret::new(creds.client_secret.clone()))
        .set_auth_uri(auth_url)
        .set_token_uri(token_url)
        .set_redirect_uri(redirect))
}

/// Clones the listener, spawns a thread that blocks on `accept`, and waits up
/// to `timeout` for a connection. Returns `None` on clone failure, accept error,
/// or timeout; logs the reason.
fn accept_with_timeout(listener: &TcpListener, timeout: Duration) -> Option<TcpStream> {
    let listener_clone = match listener.try_clone() {
        Ok(l) => l,
        Err(e) => {
            log::warn!("auth worker: cannot clone listener: {e}");
            return None;
        }
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(listener_clone.accept());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok((s, _))) => Some(s),
        Ok(Err(e)) => {
            log::warn!("auth worker: accept error: {}", e.kind());
            None
        }
        Err(_) => {
            log::warn!("auth worker: flow timed out waiting for redirect");
            None
        }
    }
}

/// Worker thread: waits for the browser redirect, exchanges the code.
///
/// On success, writes the token file (unless the flow was superseded by a
/// newer `begin_auth`). Always clears `current` from the pending slot when done.
#[allow(clippy::too_many_arguments)]
fn run_auth_worker(
    listener: &TcpListener,
    pkce_verifier: oauth2::PkceCodeVerifier,
    expected_state: &CsrfToken,
    my_generation: u64,
    redirect_uri: &str,
    creds: &GoogleCredentials,
    endpoints: &GoogleEndpoints,
    http: &reqwest::blocking::Client,
    keyring: &KeyringStore,
    cached_token: &Arc<Mutex<Option<CachedToken>>>,
    pending: &Arc<Mutex<PendingSlot>>,
    pending_changed: &Arc<Condvar>,
    auth_timeout: Duration,
    now: DateTime<Utc>,
) {
    let Some(stream) = accept_with_timeout(listener, auth_timeout) else {
        return finish_flow(
            pending,
            pending_changed,
            my_generation,
            keyring,
            cached_token,
            None,
        );
    };

    let result = parse_and_exchange(
        stream,
        pkce_verifier,
        expected_state,
        redirect_uri,
        creds,
        endpoints,
        http,
        now,
    );

    finish_flow(
        pending,
        pending_changed,
        my_generation,
        keyring,
        cached_token,
        result.ok().flatten(),
    );
}

/// Self-contained HTML page shown in the browser tab after the OAuth redirect.
///
/// Must be served with an explicit UTF-8 charset (header + meta) — without it
/// browsers fall back to Latin-1 and mangle the non-ASCII text. The phrase
/// "close this tab" is asserted by the loopback tests.
fn redirect_page(success: bool) -> String {
    let (mark, mark_color, heading, detail) = if success {
        (
            "✓",
            "#10b981",
            "Connected to Google Calendar",
            "Authorization received — you can close this tab and return to Après Work.",
        )
    } else {
        (
            "✕",
            "#ef4444",
            "Sign-in didn't complete",
            "Authorization failed — you can close this tab and retry from Après Work's settings.",
        )
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Après Work</title>
</head>
<body style="margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;font-family:system-ui,sans-serif;background:#f5f5f4;color:#1c1917">
<main style="text-align:center;padding:2.5rem 3rem;background:#fff;border:1px solid #e7e5e4;border-radius:12px;box-shadow:0 1px 3px rgba(0,0,0,0.08)">
<div style="font-size:2rem;color:{mark_color}" aria-hidden="true">{mark}</div>
<h1 style="font-size:1.25rem;margin:0.5rem 0">{heading}</h1>
<p style="margin:0;color:#57534e">{detail}</p>
</main>
</body>
</html>"#
    )
}

/// Read the HTTP request from `stream`, parse query params, respond, exchange.
///
/// Takes `pkce_verifier` by value (consumed by the exchange if it reaches
/// that point — `set_pkce_verifier` requires ownership).
///
/// Returns `Ok(Some(token))` on success, `Ok(None)` on CSRF failure / error
/// param, `Err` on unrecoverable IO.
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn parse_and_exchange(
    mut stream: TcpStream,
    pkce_verifier: oauth2::PkceCodeVerifier,
    expected_state: &CsrfToken,
    redirect_uri: &str,
    creds: &GoogleCredentials,
    endpoints: &GoogleEndpoints,
    http: &reqwest::blocking::Client,
    now: DateTime<Utc>,
) -> Result<Option<StoredToken>, ()> {
    // Read at most 4 KiB (the request line + headers — no body on a GET).
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).map_err(|e| {
        log::warn!("auth worker: read error: {}", e.kind());
    })?;
    let request_text = String::from_utf8_lossy(&buf[..n]);
    let first_line = request_text.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");

    // Parse query params via the `url` crate for correct percent-decoding.
    let parsed_url = Url::parse(&format!("http://127.0.0.1{path}")).map_err(|e| {
        log::warn!("auth worker: cannot parse redirect URL: {e}");
    })?;

    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    let mut error: Option<String> = None;
    for (key, value) in parsed_url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }

    let success = error.is_none()
        && state.as_deref() == Some(expected_state.secret().as_str())
        && code.is_some();
    let body = redirect_page(success);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    // Best-effort write — the browser tab only needs to show the message.
    let _ = stream.write_all(response.as_bytes());

    if let Some(err_param) = error {
        log::warn!("auth worker: provider returned error param");
        let _ = err_param; // never log its value — could contain sensitive info
        return Ok(None);
    }
    if state.as_deref() != Some(expected_state.secret().as_str()) {
        log::warn!("auth worker: state mismatch — possible CSRF, ignoring");
        return Ok(None);
    }
    let Some(auth_code) = code else {
        log::warn!("auth worker: redirect had no code param");
        return Ok(None);
    };

    let oauth_client = build_oauth_client(creds, endpoints, redirect_uri).map_err(|e| {
        log::warn!("auth worker: cannot build oauth client for exchange: {e}");
    })?;

    let token_response = oauth_client
        .exchange_code(AuthorizationCode::new(auth_code))
        .set_pkce_verifier(pkce_verifier)
        .request(http)
        .map_err(|e| {
            log::warn!("auth worker: token exchange failed");
            let _ = e; // never log — could include response body with token
        })?;

    let expires_at = now
        + chrono::Duration::from_std(
            token_response
                .expires_in()
                .unwrap_or(Duration::from_secs(3600)),
        )
        .unwrap_or(chrono::Duration::hours(1));

    Ok(Some(StoredToken {
        access_token: token_response.access_token().secret().clone(),
        refresh_token: token_response
            .refresh_token()
            .map(|t: &RefreshToken| t.secret().clone()),
        expires_at,
    }))
}

/// Commit the result of a finished auth flow into shared state.
///
/// If `my_generation` no longer matches the slot's `current` generation, the
/// flow was superseded — discard any token silently (never persist anything).
fn finish_flow(
    pending: &Arc<Mutex<PendingSlot>>,
    pending_changed: &Arc<Condvar>,
    my_generation: u64,
    keyring: &KeyringStore,
    cached_token: &Arc<Mutex<Option<CachedToken>>>,
    stored_token: Option<StoredToken>,
) {
    // The `pending` lock is held for the entire save + cache update below.
    // This is intentional: holding it across the keyring IPC prevents a
    // disconnect() that fires between save and cache-write from leaving a
    // dangling credential in the keyring while clearing only the in-memory
    // cache. The trade-off is that `auth_status()` (which also takes this
    // lock) blocks for the duration of the keyring round-trip; do NOT move
    // the save outside this guard to "fix" the latency — that reintroduces
    // the save-after-disconnect race.
    let mut slot = pending.lock().expect("pending slot lock in finish_flow");
    if slot
        .current
        .as_ref()
        .is_none_or(|p| p.generation != my_generation)
    {
        // Superseded or already cleared by disconnect() — discard everything.
        drop(slot);
        pending_changed.notify_all();
        return;
    }
    if let Some(token) = stored_token {
        let cred = PersistedCredential {
            refresh_token: token.refresh_token,
            expires_at: token.expires_at,
        };
        match keyring.save(&cred) {
            Ok(()) => {
                let mut cache = cached_token.lock().expect("cache lock in finish_flow");
                *cache = Some(CachedToken {
                    access_token: token.access_token,
                    expires_at: token.expires_at,
                });
            }
            Err(e) => {
                log::warn!("auth worker: cannot save credential to keyring: {e}");
            }
        }
    }
    slot.current = None;
    drop(slot);
    pending_changed.notify_all();
}

#[cfg(test)]
mod tests;
