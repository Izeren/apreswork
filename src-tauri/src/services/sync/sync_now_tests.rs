// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for `sync_now` (G8 manual full sync) and `get_sync_status`.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use crate::db::sqlite::SqliteStore;
use crate::error::AppError;
use crate::scheduler::engine::DefaultScheduler;
use crate::services::sync::{get_sync_status, sync_now, SyncOutcome};
use crate::test_support::calendar::{make_event, MockCalendarSync};
use crate::test_support::{test_now, test_store};
use crate::traits::storage::{ConfigStore, ExternalEventStore, GoogleAuthStore};

use super::test_util::make_trigger;

fn call_sync_now(
    store: &Arc<SqliteStore>,
    sync: &MockCalendarSync,
    now: DateTime<Utc>,
) -> Result<SyncOutcome, AppError> {
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(store);
    sync_now(store.as_ref(), sync, &scheduler, &trigger, now)
}

fn run_sync_now_ok_no_push(store: &Arc<SqliteStore>, sync: &MockCalendarSync, now: DateTime<Utc>) {
    let result = call_sync_now(store, sync, now);
    assert!(result.is_ok(), "sync_now must succeed: {result:?}");
    assert_eq!(
        result.expect("checked ok").pushed,
        crate::services::sync::PushCounts::default(),
        "push leg must be a no-op in this scenario"
    );
}

fn assert_sync_now_records_error(
    store: &Arc<SqliteStore>,
    sync: &MockCalendarSync,
    now: DateTime<Utc>,
) -> String {
    let err = call_sync_now(store, sync, now).unwrap_err();
    assert!(
        matches!(err, AppError::CalendarSync(_)),
        "expected CalendarSync error, got: {err:?}"
    );
    let status = get_sync_status(store.as_ref()).expect("get status");
    assert_eq!(status.last_sync_at, None, "no success timestamp on failure");
    status.last_sync_error.expect("error must be recorded")
}

#[test]
fn sync_now_pulls_pushes_and_records_bookkeeping() {
    let store = Arc::new(test_store());
    let now = test_now();
    store
        .set_config_value("pull_calendar_ids", r#"["cal-a"]"#)
        .expect("set cal ids");
    // Pre-set an error so the success path provably clears it.
    store
        .set_config_value("last_sync_error", "old failure")
        .expect("seed error");

    let mut events = HashMap::new();
    events.insert(
        "cal-a".to_owned(),
        vec![make_event("cal-a", "ev-a1", "Meeting", true, now)],
    );
    let sync = MockCalendarSync::new(true, events);
    // No local chunks and no remote app events → the push touched nothing.
    run_sync_now_ok_no_push(&store, &sync, now);

    let mirrored = store
        .get_external_events_in_range(now - Duration::days(8), now + Duration::days(31))
        .expect("get events");
    assert_eq!(mirrored.len(), 1, "mirror row must exist after sync_now");

    let auth = store.get_google_auth().expect("get auth");
    assert_eq!(
        auth.and_then(|a| a.calendar_id).as_deref(),
        Some("mock-calendar-id"),
        "sync_cycle must have persisted the app calendar id"
    );

    let status = get_sync_status(store.as_ref()).expect("get status");
    assert_eq!(status.last_sync_at, Some(now));
    assert_eq!(status.last_sync_error, None);
}

// Either leg of the cycle failing must surface the cause and leave no success
// timestamp. A blank `pull_calendar_ids` parses to an empty selection exactly like
// an unset one, so the ensure-calendar row needs no separate setup path.
#[test_case(
    "",
    &MockCalendarSync::new(true, HashMap::new()).with_ensure_calendar_error(),
    "ensure_app_calendar failed";
    "ensure_app_calendar fails"
)]
#[test_case(
    r#"["cal-err"]"#,
    &MockCalendarSync::new(true, HashMap::new())
        .with_calendar_error("cal-err", AppError::CalendarSync("network timeout".into())),
    "network timeout";
    "pull fails"
)]
fn sync_now_error_records_last_sync_error(
    pull_calendar_ids: &str,
    sync: &MockCalendarSync,
    want_cause: &str,
) {
    let store = Arc::new(test_store());
    store
        .set_config_value("pull_calendar_ids", pull_calendar_ids)
        .expect("set cal ids");
    let msg = assert_sync_now_records_error(&store, sync, test_now());
    assert!(
        msg.contains(want_cause),
        "recorded error must carry the cause; got: {msg}"
    );
}

/// Provider unavailable: `sync_now` still succeeds (reschedule-only, matching
/// pull semantics) but bookkeeping is untouched — "last sync" means last
/// provider sync, not last local reschedule.
#[test]
fn sync_now_unavailable_skips_bookkeeping() {
    let store = Arc::new(test_store());
    let now = test_now();
    store
        .set_config_value("last_sync_error", "stale error")
        .expect("seed error");
    let sync = MockCalendarSync::new(false, HashMap::new());
    run_sync_now_ok_no_push(&store, &sync, now);

    let status = get_sync_status(store.as_ref()).expect("get status");
    assert_eq!(status.last_sync_at, None, "no timestamp while disconnected");
    assert_eq!(
        status.last_sync_error.as_deref(),
        Some("stale error"),
        "bookkeeping must be untouched while disconnected"
    );
}

#[test_case(None, "", None, None; "defaults are none")]
#[test_case(
    Some("2026-07-12T10:00:00+00:00"), "",
    Some("2026-07-12T10:00:00+00:00"), None;
    "valid timestamp parsed"
)]
#[test_case(Some("not-a-date"), "", None, None; "garbage timestamp is none")]
#[test_case(None, "  ", None, None; "whitespace error is none")]
#[test_case(None, "boom", None, Some("boom"); "error string surfaced")]
fn get_sync_status_parses_config(
    at: Option<&str>,
    err: &str,
    want_at: Option<&str>,
    want_err: Option<&str>,
) {
    let store = test_store();
    if let Some(at) = at {
        store
            .set_config_value("last_sync_at", at)
            .expect("set last_sync_at");
    }
    store
        .set_config_value("last_sync_error", err)
        .expect("set last_sync_error");

    let status = get_sync_status(&store).expect("get status");

    let want_at: Option<DateTime<Utc>> = want_at.map(|s| {
        DateTime::parse_from_rfc3339(s)
            .expect("parse expected")
            .with_timezone(&Utc)
    });
    assert_eq!(status.last_sync_at, want_at);
    assert_eq!(status.last_sync_error.as_deref(), want_err);
}
