// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Google Calendar batch push path (`multipart/mixed`).
//!
//! Encodes chunk create/update/delete ops into one batch request, decodes the
//! multipart reply, and retries ONLY the rate-limited inner ops with truncated
//! exponential backoff. The retry/backoff policy is the shared [`BackoffPolicy`]
//! from the parent module; tokens are NEVER logged or included in error messages.

use std::time::Duration;

use chrono::{DateTime, Utc};

use super::{
    api_base_of, build_events_url, execute_authorized_raw, BackoffPolicy, GoogleCalendarSync,
};
use crate::error::AppError;
use crate::traits::calendar_sync::{ChunkEventPayload, SyncOp, SyncOpResult};

/// Maximum sync operations per batch request.
///
/// Google's hard cap is 1000 calls per batch, but a batch counts n-as-n against
/// the per-user quota (~600 requests/minute for recent projects), so the batch
/// size only decides how many round-trips a reschedule costs — not whether it is
/// throttled. 250 collapses a large reschedule (e.g. 87 chunks) into a single
/// round-trip while keeping each request modestly framed; the per-minute budget,
/// not this cap, is the real ceiling.
pub(crate) const BATCH_MAX_OPS: usize = 250;

/// Ceiling for any single retry backoff sleep (Google's suggested maximum for
/// truncated exponential backoff).
const MAX_BACKOFF: Duration = Duration::from_secs(64);

/// One decoded inner response from a `multipart/mixed` batch reply.
#[derive(Debug)]
struct BatchInnerResponse {
    status: u16,
    retry_after: Option<Duration>,
    body: String,
}

/// Per-op outcome after classifying its inner response.
#[derive(Debug)]
enum Slot {
    Done(SyncOpResult),
    Retry,
}

/// Google Calendar batch endpoint, derived from the injected API base.
///
/// The batch endpoint (`/batch/calendar/v3`) is a sibling of the versioned API
/// path, so we keep the scheme + authority of `api_base_url` (tests point it at a
/// local mock) and swap in the batch path. Deriving it — rather than storing a
/// second endpoint — keeps one injected base and rules out host drift.
fn batch_url(api_base: &str) -> Result<String, AppError> {
    let base = url::Url::parse(api_base)
        .map_err(|e| AppError::CalendarSync(format!("internal: invalid API base URL: {e}")))?;
    let batch = base
        .join("/batch/calendar/v3")
        .map_err(|e| AppError::CalendarSync(format!("internal: invalid batch URL: {e}")))?;
    Ok(batch.to_string())
}

/// Host-relative path for the events collection of `calendar_id`.
fn events_path(api_base: &str, calendar_id: &str) -> Result<String, AppError> {
    Ok(build_events_url(api_base, calendar_id, None)?
        .path()
        .to_owned())
}

/// Host-relative path for a single event resource under `calendar_id`.
fn event_path(api_base: &str, calendar_id: &str, event_id: &str) -> Result<String, AppError> {
    Ok(build_events_url(api_base, calendar_id, Some(event_id))?
        .path()
        .to_owned())
}

/// Shared `summary`/`description`/`start`/`end` fields for a chunk event body.
fn chunk_event_fields(payload: &ChunkEventPayload) -> serde_json::Value {
    serde_json::json!({
        "summary": payload.title,
        "description": payload.description,
        "start": {"dateTime": payload.start.to_rfc3339(), "timeZone": "UTC"},
        "end":   {"dateTime": payload.end.to_rfc3339(),   "timeZone": "UTC"},
    })
}

/// Build the `events.insert` body for a scheduled chunk.
///
/// Stamps `extendedProperties.private.apreswork_chunk_id` = `payload.chunk_id`
/// (so the pull path recognises app-owned events) and disables reminders. Used
/// by the batch encoder ([`encode_batch`]).
fn chunk_create_body(payload: &ChunkEventPayload) -> serde_json::Value {
    let mut body = chunk_event_fields(payload);
    body["reminders"] = serde_json::json!({"useDefault": false, "overrides": []});
    body["extendedProperties"] =
        serde_json::json!({"private": {"apreswork_chunk_id": payload.chunk_id}});
    body
}

/// Build the `events.patch` body for a scheduled chunk (time/content only).
///
/// Omits `extendedProperties` and `reminders`: the chunk marker set on create is
/// preserved by the PATCH merge, and reminders stay as first written.
fn chunk_update_body(payload: &ChunkEventPayload) -> serde_json::Value {
    chunk_event_fields(payload)
}

/// Encode a batch of ops as a `multipart/mixed` body; returns `(boundary, body)`.
///
/// Each part is an inner HTTP request tagged `Content-ID: <item-{index}>`, where
/// `index` is the op's ORIGINAL position. Responses are correlated back by order
/// (Google preserves it within a batch), so the id is for spec-compliance and
/// debugging, not parsing.
fn encode_batch(
    api_base: &str,
    calendar_id: &str,
    ops: &[(usize, &SyncOp)],
) -> Result<(String, String), AppError> {
    let boundary = format!("batch_{}", uuid::Uuid::now_v7());
    let mut body = String::new();
    for &(index, op) in ops {
        body.push_str("--");
        body.push_str(&boundary);
        body.push_str("\r\nContent-Type: application/http\r\nContent-ID: <item-");
        body.push_str(&index.to_string());
        body.push_str(">\r\n\r\n");

        let (method, path, json_body) = match op {
            SyncOp::Create(payload) => (
                "POST",
                events_path(api_base, calendar_id)?,
                Some(chunk_create_body(payload)),
            ),
            SyncOp::Update { event_id, payload } => (
                "PATCH",
                event_path(api_base, calendar_id, event_id)?,
                Some(chunk_update_body(payload)),
            ),
            SyncOp::Delete { event_id } => {
                ("DELETE", event_path(api_base, calendar_id, event_id)?, None)
            }
        };

        body.push_str(method);
        body.push(' ');
        body.push_str(&path);
        body.push_str(" HTTP/1.1\r\n");
        if let Some(json_body) = json_body {
            let serialized = serde_json::to_string(&json_body).map_err(|e| {
                AppError::CalendarSync(format!("internal: batch body serialize error: {e}"))
            })?;
            body.push_str("Content-Type: application/json\r\n\r\n");
            body.push_str(&serialized);
            body.push_str("\r\n");
        } else {
            body.push_str("\r\n");
        }
    }
    body.push_str("--");
    body.push_str(&boundary);
    body.push_str("--\r\n");
    Ok((boundary, body))
}

/// Extract the `boundary` value from a `multipart/mixed` Content-Type header.
fn boundary_from_content_type(content_type: &str) -> Option<String> {
    let idx = content_type.to_ascii_lowercase().find("boundary=")?;
    let start = idx.saturating_add("boundary=".len());
    let raw = content_type
        .get(start..)?
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"');
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_owned())
    }
}

/// Parse a `Retry-After` header value (integer-seconds form only).
///
/// Google Calendar, when it sends `Retry-After` at all, uses delta-seconds; the
/// HTTP-date form is treated as absent (backoff falls back to the computed delay).
fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Decode a `multipart/mixed` batch response into per-inner responses, in order.
fn parse_batch_response(
    content_type: &str,
    body: &str,
) -> Result<Vec<BatchInnerResponse>, AppError> {
    let boundary = boundary_from_content_type(content_type).ok_or_else(|| {
        AppError::CalendarSync("Google batch response missing multipart boundary".into())
    })?;
    let delimiter = format!("--{boundary}");

    let mut parts = Vec::new();
    for segment in body.split(delimiter.as_str()).skip(1) {
        let trimmed = segment.trim_start_matches(['\r', '\n']);
        if trimmed.starts_with("--") {
            break; // closing delimiter: "--{boundary}--"
        }
        if trimmed.trim().is_empty() {
            continue;
        }
        parts.push(parse_batch_part(trimmed)?);
    }
    Ok(parts)
}

/// Parse one multipart segment (part headers + the embedded HTTP response).
fn parse_batch_part(segment: &str) -> Result<BatchInnerResponse, AppError> {
    // Part headers (`Content-Type: application/http`, `Content-ID`) end at the
    // first blank line; the remainder is the embedded HTTP response.
    let inner = segment
        .split_once("\r\n\r\n")
        .map(|(_, rest)| rest)
        .ok_or_else(|| AppError::CalendarSync("Google batch response part is malformed".into()))?;

    // Split the embedded response into its status+headers block and its body.
    let (head, inner_body) = inner
        .split_once("\r\n\r\n")
        .map_or((inner, ""), |(head, body)| (head, body));

    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|status_line| status_line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            AppError::CalendarSync("Google batch response has no inner status".into())
        })?;

    let retry_after = lines
        .find(|line| line.to_ascii_lowercase().starts_with("retry-after:"))
        .and_then(|line| line.split_once(':'))
        .and_then(|(_, value)| parse_retry_after(value));

    Ok(BatchInnerResponse {
        status,
        retry_after,
        body: inner_body.trim().to_owned(),
    })
}

/// Whether an inner batch response should be retried.
///
/// 429 and transient 5xx always retry; 403 retries only when the body marks a
/// rate limit (`usageLimits` / `rateLimitExceeded`), never a permission denial.
fn inner_is_retryable(status: u16, body: &str) -> bool {
    match status {
        429 | 500 | 502 | 503 | 504 => true,
        403 => {
            body.contains("usageLimits")
                || body.contains("rateLimitExceeded")
                || body.contains("userRateLimitExceeded")
        }
        _ => false,
    }
}

/// Event write (`{id, etag}`) inner-part body.
#[derive(serde::Deserialize)]
struct EventWriteResponse {
    id: String,
    etag: Option<String>,
}

/// Parse an event write (`{id, etag}`) inner-part body.
fn parse_event_write(body: &str) -> Result<EventWriteResponse, AppError> {
    serde_json::from_str(body)
        .map_err(|e| AppError::CalendarSync(format!("Google API response parse error: {e}")))
}

/// A non-2xx inner response: retry if retryable, else a permanent error.
fn non_success_slot(status: u16, body: &str) -> Result<Slot, AppError> {
    if inner_is_retryable(status, body) {
        Ok(Slot::Retry)
    } else {
        Err(AppError::CalendarSync(format!(
            "Google batch operation failed with HTTP {status}"
        )))
    }
}

/// Classify one op's inner response into a finished result or a retry.
///
/// Returns `Err` for a permanent failure, which fails the whole cycle — Phase B
/// reconciles any already-applied ops on the next sync.
fn classify_part(op: &SyncOp, part: &BatchInnerResponse) -> Result<Slot, AppError> {
    let status = part.status;
    let success = (200..300).contains(&status);
    match op {
        SyncOp::Delete { .. } => {
            // 404 = already gone; idempotent success.
            if success || status == 404 {
                Ok(Slot::Done(SyncOpResult::Deleted))
            } else {
                non_success_slot(status, &part.body)
            }
        }
        SyncOp::Create(payload) => {
            if success {
                let resp = parse_event_write(&part.body)?;
                Ok(Slot::Done(SyncOpResult::Created {
                    chunk_id: payload.chunk_id.clone(),
                    event_id: resp.id,
                    etag: resp.etag,
                }))
            } else {
                non_success_slot(status, &part.body)
            }
        }
        SyncOp::Update { event_id, payload } => {
            if success {
                // We already know the event_id; only the etag matters from the body.
                let resp = parse_event_write(&part.body)?;
                Ok(Slot::Done(SyncOpResult::Updated {
                    chunk_id: payload.chunk_id.clone(),
                    event_id: event_id.clone(),
                    etag: resp.etag,
                }))
            } else {
                non_success_slot(status, &part.body)
            }
        }
    }
}

/// Random jitter in `[0, base)` derived from the sub-second nanos of `now`
/// (desyncs concurrent retriers without a new dependency).
fn jitter(base: Duration, now: DateTime<Utc>) -> Duration {
    let span = u64::try_from(base.as_nanos()).unwrap_or(u64::MAX);
    if span == 0 {
        return Duration::ZERO;
    }
    let now_nanos = u64::from(now.timestamp_subsec_nanos());
    Duration::from_nanos(now_nanos % span)
}

/// Larger of two optional `Retry-After` hints.
fn max_hint(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Truncated exponential backoff for retry `attempt` (1-based), plus jitter.
///
/// A server `Retry-After` hint, when present, overrides the computed delay (still
/// capped at [`MAX_BACKOFF`]).
fn retry_delay(
    backoff: &BackoffPolicy,
    attempt: u32,
    server_hint: Option<Duration>,
    now: DateTime<Utc>,
) -> Duration {
    if let Some(hint) = server_hint {
        return hint.min(MAX_BACKOFF);
    }
    let shift = attempt.saturating_sub(1).min(31);
    #[allow(clippy::arithmetic_side_effects)] // shift <= 31, so 1u32 << shift never overflows
    let factor = 1u32 << shift;
    backoff
        .base_delay
        .saturating_mul(factor)
        .saturating_add(jitter(backoff.base_delay, now))
        .min(MAX_BACKOFF)
}

/// Execute one ≤`batch_max` batch, retrying ONLY the rate-limited inner ops with
/// truncated exponential backoff. Returns `(original_index, result)` for every op
/// in the chunk, or `Err` on a permanent failure / exhausted retries.
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
fn execute_one_batch(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    calendar_id: &str,
    api_base: &str,
    batch_endpoint: &str,
    chunk: &[(usize, &SyncOp)],
) -> Result<Vec<(usize, SyncOpResult)>, AppError> {
    let mut pending: Vec<(usize, &SyncOp)> = chunk.to_vec();
    let mut done: Vec<(usize, SyncOpResult)> = Vec::with_capacity(chunk.len());
    let mut attempt: u32 = 0;

    loop {
        let (boundary, req_body) = encode_batch(api_base, calendar_id, &pending)?;
        let content_type = format!("multipart/mixed; boundary={boundary}");

        let response = execute_authorized_raw(provider, backoff, now, &|token| {
            provider
                .http()
                .post(batch_endpoint)
                .bearer_auth(token)
                .header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                .body(req_body.clone())
        })?;

        let status_u16 = response.status().as_u16();
        if !response.status().is_success() {
            return Err(AppError::CalendarSync(format!(
                "Google batch request failed with HTTP {status_u16}"
            )));
        }
        let resp_content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let text = response.text().map_err(|_| {
            AppError::CalendarSync("Google batch response parse error: failed to read body".into())
        })?;

        let parts = parse_batch_response(&resp_content_type, &text)?;
        if parts.len() != pending.len() {
            return Err(AppError::CalendarSync(format!(
                "Google batch response part count mismatch: expected {}, got {}",
                pending.len(),
                parts.len()
            )));
        }

        let mut next_pending: Vec<(usize, &SyncOp)> = Vec::new();
        let mut hint: Option<Duration> = None;
        for (&(index, op), part) in pending.iter().zip(parts.iter()) {
            match classify_part(op, part)? {
                Slot::Done(result) => done.push((index, result)),
                Slot::Retry => {
                    next_pending.push((index, op));
                    hint = max_hint(hint, part.retry_after);
                }
            }
        }

        if next_pending.is_empty() {
            return Ok(done);
        }

        attempt = attempt.saturating_add(1);
        if attempt >= backoff.max_attempts {
            return Err(AppError::CalendarSync(format!(
                "Google batch throttled: {} operation(s) still rate-limited after {} attempts",
                next_pending.len(),
                backoff.max_attempts
            )));
        }
        let delay = retry_delay(backoff, attempt, hint, now);
        log::info!(
            "google_http: batch retry — {} op(s) rate-limited, sleeping {}ms",
            next_pending.len(),
            delay.as_millis()
        );
        std::thread::sleep(delay);
        pending = next_pending;
    }
}

/// Push sync ops to Google via the `multipart/mixed` batch endpoint.
///
/// Chunks `ops` at `batch_max` and returns exactly one [`SyncOpResult`] per op,
/// in input order. Rate-limited ops are retried per batch with backoff; any
/// permanent failure or exhausted retry fails the whole call (all-or-nothing —
/// the caller records no bases, and Phase B reconciles already-applied ops on the
/// next cycle).
pub(crate) fn batch_sync_ops(
    provider: &GoogleCalendarSync,
    backoff: &BackoffPolicy,
    now: DateTime<Utc>,
    batch_max: usize,
    calendar_id: &str,
    ops: &[SyncOp],
) -> Result<Vec<SyncOpResult>, AppError> {
    if ops.is_empty() {
        return Ok(Vec::new());
    }
    // `slice::chunks` panics on 0; the cap is a const 250, but stay defensive.
    let batch_max = batch_max.max(1);
    let api_base = api_base_of(provider);
    let batch_endpoint = batch_url(&api_base)?;

    let mut results: Vec<Option<SyncOpResult>> = Vec::new();
    results.resize_with(ops.len(), || None);

    for (chunk_index, chunk) in ops.chunks(batch_max).enumerate() {
        let base_offset = chunk_index.saturating_mul(batch_max);
        let indexed: Vec<(usize, &SyncOp)> = chunk
            .iter()
            .enumerate()
            .map(|(i, op)| (base_offset.saturating_add(i), op))
            .collect();
        for (index, result) in execute_one_batch(
            provider,
            backoff,
            now,
            calendar_id,
            &api_base,
            &batch_endpoint,
            &indexed,
        )? {
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(result);
            }
        }
    }

    results
        .into_iter()
        .map(|slot| slot.ok_or_else(|| AppError::CalendarSync("internal: batch result gap".into())))
        .collect()
}

#[cfg(test)]
mod batch_tests;
