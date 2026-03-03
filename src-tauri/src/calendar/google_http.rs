// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Retry/backoff policy lives here and ONLY here — all call sites import the
//! single [`BackoffPolicy`] definition. Tokens are NEVER logged or included
//! in error messages.

use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Utc};

use super::google::GoogleCalendarSync;
use crate::error::AppError;
use crate::traits::calendar_sync::{
    ExternalCalendar, ExternalEvent, RemoteChunkEvent, UserEventPayload,
};

fn parse_err(e: impl std::fmt::Display) -> AppError {
    AppError::CalendarSync(format!("Google API response parse error: {e}"))
}

fn http_status_err(status_u16: u16) -> AppError {
    AppError::CalendarSync(format!("Google API request failed with HTTP {status_u16}"))
}

/// Retry policy for Google API calls (403 / 429 rate-limit responses).
///
/// ONE definition used by every call site; never copy this struct.
pub(crate) struct BackoffPolicy {
    /// Initial delay (doubles each retry attempt).
    pub base_delay: Duration,
    /// Maximum number of total attempts (first try + retries).
    pub max_attempts: u32,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(500),
            max_attempts: 3,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarListResponse {
    #[serde(default)]
    items: Vec<CalendarListItem>,
    next_page_token: Option<String>,
}

#[derive(serde::Deserialize)]
struct CalendarListItem {
    id: Option<String>,
    summary: Option<String>,
    primary: Option<bool>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsListResponse {
    #[serde(default)]
    items: Vec<EventItem>,
    next_page_token: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventItem {
    id: Option<String>,
    status: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    start: Option<EventDateTime>,
    end: Option<EventDateTime>,
    transparency: Option<String>,
    #[serde(default)]
    attendees: Vec<Attendee>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventDateTime {
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(serde::Deserialize)]
struct Attendee {
    /// JSON key is `"self"` — a keyword in Rust.
    #[serde(rename = "self", default)]
    self_: bool,
    #[serde(rename = "responseStatus")]
    response_status: Option<String>,
}

#[derive(serde::Deserialize)]
struct CreateCalendarResponse {
    id: String,
}

/// Separate from `EventItem` (used by `list_events`) to include `extended_properties`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppEventItem {
    id: Option<String>,
    etag: Option<String>,
    start: Option<EventTime>,
    end: Option<EventTime>,
    summary: Option<String>,
    description: Option<String>,
    #[serde(rename = "extendedProperties")]
    extended_properties: Option<AppExtendedProperties>,
}

#[derive(serde::Deserialize)]
struct AppExtendedProperties {
    private: Option<std::collections::HashMap<String, String>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventTime {
    date_time: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppEventsListResponse {
    #[serde(default)]
    items: Vec<AppEventItem>,
    next_page_token: Option<String>,
}

/// Execute an authorized request with 401-refresh-once and 403/429 backoff.
///
/// Returns the raw `Response` for any status that is not retried:
/// - **401**: refreshes token once; second 401 → `Err`.
/// - **403 / 429**: exponential backoff up to `backoff.max_attempts`; exhausted → `Err`.
/// - **All other statuses** (2xx, 404, 5xx …): returned as-is so callers can
///   inspect the status and body as appropriate.
///
/// Token material is NEVER included in error messages.
pub(crate) fn execute_authorized_raw(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    build: &dyn Fn(&str) -> reqwest::blocking::RequestBuilder,
) -> Result<reqwest::blocking::Response, AppError> {
    let mut token = provider.access_token(now, false)?;
    let mut refreshed_once = false;
    let mut backoff_attempt: u32 = 0;

    // The loop terminates because:
    //   - 401 is handled at most twice (second 401 → Err).
    //   - 403/429 are capped by max_attempts (→ Err when exhausted).
    //   - Any other status → returned to caller.
    loop {
        let started = std::time::Instant::now();
        let response = build(&token).send().map_err(|e| {
            AppError::CalendarSync(format!(
                "Google API request failed: {}",
                e.status()
                    .map_or_else(|| "network error".to_owned(), |s| s.to_string())
            ))
        })?;

        let status_u16 = response.status().as_u16();
        log::info!(
            "google_http: {} -> {status_u16} in {}ms",
            response.url().path(),
            started.elapsed().as_millis()
        );

        if status_u16 == 401 {
            if refreshed_once {
                return Err(AppError::CalendarSync(
                    "reconnect required: Google rejected the credentials (HTTP 401)".into(),
                ));
            }
            log::info!("google_http: 401 — refreshing token");
            token = provider.access_token(now, true)?;
            refreshed_once = true;
            continue;
        }

        if status_u16 == 403 || status_u16 == 429 {
            backoff_attempt += 1;
            if backoff_attempt >= backoff.max_attempts {
                return Err(AppError::CalendarSync(format!(
                    "Google API request failed with HTTP {status_u16} after {} attempts",
                    backoff.max_attempts
                )));
            }
            // Exponential backoff: base_delay * 2^(attempt-1).
            // backoff_attempt is always >= 1 here.
            #[allow(clippy::arithmetic_side_effects)]
            // attempt >= 1, max is max_attempts-1; no overflow for realistic values
            let delay = backoff
                .base_delay
                .saturating_mul(1u32 << (backoff_attempt - 1));
            log::info!(
                "google_http: backoff {}ms after HTTP {status_u16}",
                delay.as_millis()
            );
            std::thread::sleep(delay);
            continue;
        }

        return Ok(response);
    }
}

/// Send an authorized request with 401-refresh-once and 403/429 backoff.
///
/// - **401**: refreshes the token once per logical call via
///   `provider.access_token(now, true)`; a second 401 returns
///   `Err("reconnect required: …")`.
/// - **403 / 429**: sleeps `base_delay * 2^attempt` and retries up to
///   `backoff.max_attempts` total attempts; exhausted → `Err`.
/// - **Other non-2xx**: immediate `Err` (no retry).
/// - **2xx**: parses the JSON body and returns the value.
///
/// Token material is NEVER included in error messages.
fn send_authorized(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    build: &dyn Fn(&str) -> reqwest::blocking::RequestBuilder,
) -> Result<serde_json::Value, AppError> {
    let response = execute_authorized_raw(provider, backoff, now, build)?;

    let status = response.status();
    let status_u16 = status.as_u16();

    if !status.is_success() {
        return Err(http_status_err(status_u16));
    }

    let text = response.text().map_err(|_| {
        AppError::CalendarSync("Google API response parse error: failed to read body".into())
    })?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(parse_err)?;
    Ok(json)
}

/// Implemented by every `*ListResponse` DTO so [`fetch_all_pages`] can drive
/// `nextPageToken` pagination generically.
trait PagedResponse: serde::de::DeserializeOwned {
    type Item;

    /// Split the page into its items and the `nextPageToken` (if any).
    fn into_parts(self) -> (Vec<Self::Item>, Option<String>);
}

impl PagedResponse for CalendarListResponse {
    type Item = CalendarListItem;
    fn into_parts(self) -> (Vec<Self::Item>, Option<String>) {
        (self.items, self.next_page_token)
    }
}

impl PagedResponse for EventsListResponse {
    type Item = EventItem;
    fn into_parts(self) -> (Vec<Self::Item>, Option<String>) {
        (self.items, self.next_page_token)
    }
}

impl PagedResponse for AppEventsListResponse {
    type Item = AppEventItem;
    fn into_parts(self) -> (Vec<Self::Item>, Option<String>) {
        (self.items, self.next_page_token)
    }
}

/// Follow `nextPageToken` pagination for a Google list endpoint, collecting
/// every item across all pages.
///
/// `base_query` is sent with every request; `pageToken` is appended
/// automatically for pages after the first. A deserialization failure surfaces
/// as [`AppError::CalendarSync`] with the shared parse-error message.
fn fetch_all_pages<R: PagedResponse>(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    url: &str,
    base_query: &[(&str, &str)],
) -> Result<Vec<R::Item>, AppError> {
    let mut results: Vec<R::Item> = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let pt = page_token.take();
        let json = send_authorized(provider, backoff, now, &|token| {
            let mut req = provider
                .http()
                .get(url)
                .bearer_auth(token)
                .query(base_query);
            if let Some(ref pt_val) = pt {
                req = req.query(&[("pageToken", pt_val.as_str())]);
            }
            req
        })?;

        let resp: R = serde_json::from_value(json).map_err(parse_err)?;
        let (items, next) = resp.into_parts();
        results.extend(items);

        page_token = next;
        if page_token.is_none() {
            break;
        }
    }

    Ok(results)
}

/// Follows `nextPageToken` pagination until exhausted. Items without an `id`
/// are skipped defensively.
pub(crate) fn list_calendars(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
) -> Result<Vec<ExternalCalendar>, AppError> {
    let api_base = api_base_of(provider);
    let cal_list_url = format!("{api_base}/users/me/calendarList");

    let items = fetch_all_pages::<CalendarListResponse>(
        provider,
        backoff,
        now,
        &cal_list_url,
        &[("maxResults", "250")],
    )?;

    Ok(items
        .into_iter()
        .filter_map(|item| {
            let id = item.id?;
            Some(ExternalCalendar {
                id,
                title: item.summary.unwrap_or_default(),
                primary: item.primary.unwrap_or(false),
            })
        })
        .collect())
}

/// Uses `singleEvents=true` so recurring events arrive pre-expanded. Follows
/// `nextPageToken` pagination. The calendar id is percent-encoded via
/// [`url::Url::path_segments_mut`] to handle ids containing `#` or `@`.
pub(crate) fn list_events(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<ExternalEvent>, AppError> {
    let (events_url_str, start_str, end_str) =
        events_url_and_range(provider, calendar_id, start, end)?;

    let items = fetch_all_pages::<EventsListResponse>(
        provider,
        backoff,
        now,
        &events_url_str,
        &[
            ("singleEvents", "true"),
            ("timeMin", start_str.as_str()),
            ("timeMax", end_str.as_str()),
            ("maxResults", "2500"),
        ],
    )?;

    Ok(items
        .iter()
        .filter_map(|item| map_event(calendar_id, item))
        .collect())
}

/// Returns `None` for cancelled events, items without an id, or items whose
/// start/end cannot be parsed (with a `log::warn!` in that case). All policy
/// for `busy` and `declined` lives here exclusively (Decision 5).
fn map_event(calendar_id: &str, item: &EventItem) -> Option<ExternalEvent> {
    if item.status.as_deref() == Some("cancelled") {
        return None;
    }

    let event_id = item.id.clone()?;
    let title = item.summary.as_deref().unwrap_or("").to_owned();
    let description = item.description.clone();

    let Some(start_dt_field) = item.start.as_ref() else {
        log::warn!("google_http: skipping event {event_id}: missing start");
        return None;
    };
    let Some(end_dt_field) = item.end.as_ref() else {
        log::warn!("google_http: skipping event {event_id}: missing end");
        return None;
    };

    let Some(start) = parse_event_datetime(start_dt_field) else {
        log::warn!("google_http: skipping event {event_id}: unparseable start");
        return None;
    };
    let Some(end) = parse_event_datetime(end_dt_field) else {
        log::warn!("google_http: skipping event {event_id}: unparseable end");
        return None;
    };

    let declined = item
        .attendees
        .iter()
        .any(|a| a.self_ && a.response_status.as_deref() == Some("declined"));

    // Decision 5: transparent events (provider "free" status) are not busy;
    // declined events are also not busy (scheduler may place chunks over them).
    let transparent = item.transparency.as_deref() == Some("transparent");
    let busy = !transparent && !declined;

    // A date-only `start` marks an all-day event. `start`/`end` above still carry
    // the full local-day UTC span (parse_event_datetime); this only records the
    // original date-only representation for rendering + all-day write-back.
    let all_day = start_dt_field.date.is_some();

    Some(ExternalEvent {
        calendar_id: calendar_id.to_owned(),
        event_id,
        title,
        description,
        start,
        end,
        busy,
        declined,
        all_day,
    })
}

fn parse_rfc3339_utc(s: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// - `dateTime` field → parse RFC 3339 (includes UTC offset) → convert to UTC.
/// - `date` field (all-day event) → midnight in the machine's local timezone
///   → UTC.  Returns `None` if the local midnight is ambiguous/nonexistent
///   (DST gap edge case).
fn parse_event_datetime(dt: &EventDateTime) -> Option<DateTime<Utc>> {
    if let Some(dt_str) = &dt.date_time {
        return parse_rfc3339_utc(dt_str);
    }

    if let Some(date_str) = &dt.date {
        let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        let naive_midnight = naive.and_hms_opt(0, 0, 0)?;
        // `.earliest()` resolves ambiguous local times (DST fallback); returns
        // None when local midnight is unrepresentable (DST gap).
        return naive_midnight
            .and_local_timezone(Local)
            .earliest()
            .map(|local_dt| local_dt.with_timezone(&Utc));
    }

    None
}

/// Create the dedicated "Après Work" calendar and return its provider calendar ID.
///
/// POST `{api_base}/calendars` with `{"summary": "Après Work"}`.
pub(crate) fn create_calendar(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
) -> Result<String, AppError> {
    let api_base = api_base_of(provider);
    let url = format!("{api_base}/calendars");
    let body = serde_json::json!({"summary": "Après Work"});

    let json = send_authorized(provider, backoff, now, &|token| {
        provider.http().post(&url).bearer_auth(token).json(&body)
    })?;

    let resp: CreateCalendarResponse = serde_json::from_value(json).map_err(parse_err)?;
    Ok(resp.id)
}

/// Fetch app-owned events on the dedicated calendar overlapping `[start, end)`.
///
/// Filters to events bearing `extendedProperties.private.apreswork_chunk_id`.
/// Items missing an id or with unparseable times are skipped with a warn log.
/// Follows `nextPageToken` pagination.
pub(crate) fn list_app_calendar_events(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<RemoteChunkEvent>, AppError> {
    let (events_url_str, start_str, end_str) =
        events_url_and_range(provider, calendar_id, start, end)?;

    let items = fetch_all_pages::<AppEventsListResponse>(
        provider,
        backoff,
        now,
        &events_url_str,
        &[
            ("singleEvents", "true"),
            ("showDeleted", "false"),
            ("timeMin", start_str.as_str()),
            ("timeMax", end_str.as_str()),
            ("maxResults", "2500"),
        ],
    )?;

    let mut results: Vec<RemoteChunkEvent> = Vec::new();
    for item in items {
        let has_marker = item
            .extended_properties
            .as_ref()
            .and_then(|ep| ep.private.as_ref())
            .and_then(|p| p.get("apreswork_chunk_id"))
            .is_some();
        if !has_marker {
            continue;
        }

        let Some(event_id) = item.id else {
            log::warn!("google_http: app event has no id, skipping");
            continue;
        };

        let Some(start_time) = parse_app_event_time(item.start.as_ref(), &event_id, "start") else {
            continue;
        };

        let Some(end_time) = parse_app_event_time(item.end.as_ref(), &event_id, "end") else {
            continue;
        };

        results.push(RemoteChunkEvent {
            event_id,
            etag: item.etag,
            start: start_time,
            end: end_time,
            title: item.summary.unwrap_or_default(),
            description: item.description,
        });
    }

    Ok(results)
}

/// App-owned events are always timed (never all-day), so only `dateTime` is consulted.
/// `which` (`"start"`/`"end"`) names the field in the log.
fn parse_app_event_time(
    field: Option<&EventTime>,
    event_id: &str,
    which: &str,
) -> Option<DateTime<Utc>> {
    let parsed = field
        .and_then(|t| t.date_time.as_deref())
        .and_then(parse_rfc3339_utc);
    if parsed.is_none() {
        log::warn!("google_http: app event {event_id}: unparseable {which}, skipping");
    }
    parsed
}

/// Delete an event from the dedicated app calendar.
///
/// 204 No Content (deleted) and 404 Not Found (already gone) are both treated
/// as success — this makes the operation idempotent.
///
/// Uses `execute_authorized_raw` directly (instead of `send_authorized`) because
/// a successful DELETE returns 204 with no body, which `send_authorized`'s JSON
/// parse would reject.
pub(crate) fn delete_event(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    event_id: &str,
) -> Result<(), AppError> {
    let api_base = api_base_of(provider);
    let url_str = build_events_url(&api_base, calendar_id, Some(event_id))?.to_string();

    let response = execute_authorized_raw(provider, backoff, now, &|token| {
        provider.http().delete(url_str.as_str()).bearer_auth(token)
    })?;

    let status_u16 = response.status().as_u16();

    if status_u16 == 204 || status_u16 == 404 {
        return Ok(());
    }

    if !response.status().is_success() {
        return Err(http_status_err(status_u16));
    }

    Ok(())
}

/// Map an `events.insert` / `events.patch` response body to [`ExternalEvent`].
///
/// Both write endpoints return the full event resource. Mapping through
/// [`map_event`] keeps `busy` / `declined` derivation in one place (Decision 5)
/// rather than letting the caller assume defaults.
fn map_write_response(
    calendar_id: &str,
    json: serde_json::Value,
) -> Result<ExternalEvent, AppError> {
    let item: EventItem = serde_json::from_value(json).map_err(parse_err)?;
    map_event(calendar_id, &item).ok_or_else(|| {
        AppError::CalendarSync("Google API response missing required event fields".into())
    })
}

/// Timed events serialize `start`/`end` as `dateTime` (UTC). All-day events
/// serialize them as `date` — Google's exclusive-end convention — converting the
/// UTC instants back through the machine's local zone so the dates round-trip
/// exactly what the all-day pull produced (`parse_event_datetime`). A `null`
/// description clears the field (merge-patch semantics on update).
fn user_event_body(payload: &UserEventPayload) -> serde_json::Value {
    let (start, end) = if payload.all_day {
        let start_date = payload
            .start
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string();
        let end_date = payload
            .end
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string();
        (
            serde_json::json!({ "date": start_date }),
            serde_json::json!({ "date": end_date }),
        )
    } else {
        (
            serde_json::json!({ "dateTime": payload.start.to_rfc3339(), "timeZone": "UTC" }),
            serde_json::json!({ "dateTime": payload.end.to_rfc3339(), "timeZone": "UTC" }),
        )
    };
    serde_json::json!({
        "summary": payload.title,
        "description": payload.description,
        "start": start,
        "end": end,
    })
}

fn send_user_event_request(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    event_id: Option<&str>,
    payload: &UserEventPayload,
) -> Result<ExternalEvent, AppError> {
    let api_base = api_base_of(provider);
    let url_str = build_events_url(&api_base, calendar_id, event_id)?.to_string();
    let body = user_event_body(payload);
    let json = send_authorized(provider, backoff, now, &|token| {
        let req = if event_id.is_some() {
            provider.http().patch(url_str.as_str())
        } else {
            provider.http().post(url_str.as_str())
        };
        req.bearer_auth(token).json(&body)
    })?;
    map_write_response(calendar_id, json)
}

/// Create a user-owned event on `calendar_id` and return the mirror event.
///
/// Unlike the chunk-create body, stamps NO `extendedProperties.private.apreswork_chunk_id`
/// marker and sets no `reminders` override (the calendar's default reminders
/// apply, which is what a user expects for their own event).
pub(crate) fn create_user_event(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    payload: &UserEventPayload,
) -> Result<ExternalEvent, AppError> {
    send_user_event_request(provider, backoff, now, calendar_id, None, payload)
}

/// Update a user-owned event's time/content and return the mirror event.
///
/// PATCH with `summary` / `description` / `start` / `end` only — no chunk
/// marker is ever written. A `null` description clears the field (merge-patch
/// semantics), matching a user who empties the description in the editor.
pub(crate) fn update_user_event(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    event_id: &str,
    payload: &UserEventPayload,
) -> Result<ExternalEvent, AppError> {
    send_user_event_request(provider, backoff, now, calendar_id, Some(event_id), payload)
}

/// One definition for every endpoint builder in this module.
fn api_base_of(provider: &GoogleCalendarSync) -> String {
    provider
        .endpoints()
        .api_base_url
        .trim_end_matches('/')
        .to_owned()
}

/// Callers take `.to_string()` (absolute) or `.path()` (relative).
fn build_events_url(
    api_base: &str,
    calendar_id: &str,
    event_id: Option<&str>,
) -> Result<url::Url, AppError> {
    let mut url = url::Url::parse(&format!("{api_base}/calendars"))
        .map_err(|e| AppError::CalendarSync(format!("internal: invalid API base URL: {e}")))?;
    {
        let mut segments = url.path_segments_mut().map_err(|()| {
            AppError::CalendarSync("internal: API base URL is cannot-be-base".into())
        })?;
        segments.push(calendar_id).push("events");
        if let Some(id) = event_id {
            segments.push(id);
        }
    }
    Ok(url)
}

/// Shared by [`list_events`] and [`list_app_calendar_events`].
fn events_url_and_range(
    provider: &GoogleCalendarSync,
    calendar_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(String, String, String), AppError> {
    let api_base = api_base_of(provider);
    let events_url_str = build_events_url(&api_base, calendar_id, None)?.to_string();
    let start_str = start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let end_str = end.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    Ok((events_url_str, start_str, end_str))
}

mod batch;
pub(crate) use batch::{batch_sync_ops, BATCH_MAX_OPS};
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_tests;
