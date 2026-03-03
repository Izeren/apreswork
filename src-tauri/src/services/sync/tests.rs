// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for `disconnect_provider`, `parse_pull_calendar_ids`,
//! `pull_external_events`, `get_pull_calendars`, and `set_pull_calendars`.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use super::test_util::{count_rows, make_trigger};
use super::{
    disconnect_provider, get_pull_calendars, parse_pull_calendar_ids, pull_and_reschedule,
    pull_external_events, set_pull_calendars,
};
use crate::calendar::google::{GoogleCalendarSync, GoogleCredentials, GoogleEndpoints};
use crate::calendar::google_token::{KeyringStore, PersistedCredential};
use crate::calendar::noop::NoopCalendarSync;
use crate::db::sqlite::SqliteStore;
use crate::domain::models::{Chunk, ChunkSyncState, ExternalEventRecord, GoogleAuthState, Task};
use crate::error::AppError;
use crate::scheduler::engine::DefaultScheduler;
use crate::test_support::calendar::{make_event, MockCalendarSync};
use crate::test_support::{
    default_config, seed_chunk, seed_task, test_now, test_store, test_store_with_config,
};
use crate::traits::calendar_sync::ExternalEvent;
use crate::traits::scheduling::ScheduleResult;
use crate::traits::storage::{
    ChunkSyncStateStore, ConfigStore, ExternalEventStore, GoogleAuthStore,
};

fn make_store_with_sync_data() -> SqliteStore {
    let store = SqliteStore::new_in_memory();

    store
        .set_google_auth(&GoogleAuthState {
            calendar_id: Some("cal-id".to_owned()),
            connected_at: None,
        })
        .expect("seed google_auth");

    let event = ext_event_record(
        "ev-1",
        "cal-id",
        "gcal-ev-1",
        "Test event",
        true,
        test_now(),
    );
    let window_start = test_now() - Duration::hours(1);
    let window_end = test_now() + Duration::hours(2);
    store
        .replace_external_events_in_window("cal-id", window_start, window_end, &[event])
        .expect("seed external_events");

    // Seed chunk_sync_state row: needs a task + chunk for FK.
    let task = Task::test_default().with_id("t-sync");
    seed_task(&store, &task);
    let chunk = Chunk::test_default().with_id("c-sync").with_task("t-sync");
    seed_chunk(&store, &chunk);
    store
        .upsert_chunk_sync_state(&ChunkSyncState {
            chunk_id: "c-sync".to_owned(),
            event_id: "gcal-ev-1".to_owned(),
            etag: None,
            synced_start: chunk.start_time,
            synced_end: chunk.end_time,
            synced_title: "Sync task".to_owned(),
            synced_description: String::new(),
            updated_at: chunk.updated_at,
        })
        .expect("seed chunk_sync_state");

    store
}

/// Build an [`ExternalEventRecord`] with `end_time` = `start_time` + 1h and
/// `updated_at` = `start_time` — the shape every seed fixture in this module
/// needs; callers wanting other cadences build the struct directly.
fn ext_event_record(
    id: &str,
    calendar_id: &str,
    event_id: &str,
    title: &str,
    busy: bool,
    start_time: DateTime<Utc>,
) -> ExternalEventRecord {
    ExternalEventRecord {
        id: id.to_owned(),
        calendar_id: calendar_id.to_owned(),
        event_id: event_id.to_owned(),
        title: title.to_owned(),
        description: None,
        start_time,
        end_time: start_time + Duration::hours(1),
        busy,
        declined: false,
        all_day: false,
        updated_at: start_time,
    }
}

fn select_pull_calendars(store: &SqliteStore, ids: &str) {
    store
        .set_config_value("pull_calendar_ids", ids)
        .expect("set cal ids");
}

fn store_with_pull_calendars(ids: &str) -> SqliteStore {
    let store = test_store_with_config(default_config());
    select_pull_calendars(&store, ids);
    store
}

/// A `MockCalendarSync` event map holding one calendar's events, built from
/// `(event_id, title, busy)` triples that all share `now` as their start time.
fn single_cal_events(
    calendar_id: &str,
    specs: &[(&str, &str, bool)],
    now: DateTime<Utc>,
) -> HashMap<String, Vec<ExternalEvent>> {
    let events = specs
        .iter()
        .map(|(event_id, title, busy)| make_event(calendar_id, event_id, title, *busy, now))
        .collect();
    HashMap::from([(calendar_id.to_owned(), events)])
}

/// Fresh default-config store and a standard 7-day-back / 30-day-forward pull window.
fn deselection_test_store() -> (SqliteStore, DateTime<Utc>, DateTime<Utc>, DateTime<Utc>) {
    let store = test_store_with_config(default_config());
    let now = test_now();
    (
        store,
        now,
        now - Duration::days(8),
        now + Duration::days(30),
    )
}

fn assert_empty_pull_preserves_count(
    store: &SqliteStore,
    now: DateTime<Utc>,
    expected: i64,
    msg: &str,
) {
    assert_eq!(count_rows(store, "external_events"), expected);
    let sync = MockCalendarSync::new(true, HashMap::new());
    pull_external_events(store, &sync, now).expect("pull ok");
    assert_eq!(count_rows(store, "external_events"), expected, "{msg}");
}

#[test]
fn disconnect_provider_clears_all_sync_tables() {
    let keyring = KeyringStore::with_mock_entry(std::sync::Arc::new(
        keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default())),
    ));
    keyring
        .save(&PersistedCredential {
            refresh_token: Some("rt".to_owned()),
            expires_at: test_now() + Duration::hours(1),
        })
        .expect("seed keyring credential");

    let provider = GoogleCalendarSync::with_mock_keyring(
        GoogleCredentials {
            client_id: "ci".to_owned(),
            client_secret: "cs".to_owned(),
        },
        keyring.clone(),
        GoogleEndpoints {
            auth_url: "http://127.0.0.1:1/auth".to_owned(),
            token_url: "http://127.0.0.1:1/token".to_owned(),
            api_base_url: "http://127.0.0.1:1/api".to_owned(),
        },
        std::time::Duration::from_secs(5),
    );

    let store = make_store_with_sync_data();

    disconnect_provider(&store, &provider).expect("disconnect_provider");

    assert_eq!(
        count_rows(&store, "google_auth"),
        0,
        "google_auth must be empty"
    );
    assert_eq!(
        count_rows(&store, "chunk_sync_state"),
        0,
        "chunk_sync_state must be empty"
    );
    assert_eq!(
        count_rows(&store, "external_events"),
        0,
        "external_events must be empty"
    );

    assert!(
        keyring.load().expect("load").is_none(),
        "keyring credential must be deleted after disconnect"
    );
}

#[test]
fn disconnect_provider_error_propagates_db_rows_remain() {
    let store = make_store_with_sync_data();
    let sync = MockCalendarSync::new(false, HashMap::new()).with_disconnect_error();
    let err = disconnect_provider(&store, &sync).unwrap_err();
    assert!(
        matches!(err, AppError::CalendarSync(_)),
        "expected CalendarSync error, got: {err:?}"
    );

    // DB rows must be untouched: provider error aborts before the with_tx.
    assert_eq!(
        count_rows(&store, "google_auth"),
        1,
        "google_auth must be untouched on provider error"
    );
}

#[test]
fn disconnect_provider_noop_clears_db_tables() {
    // NoopCalendarSync.disconnect() always returns Ok(());
    // verify the DB wipe still runs.
    let store = make_store_with_sync_data();
    disconnect_provider(&store, &NoopCalendarSync).expect("disconnect noop");
    assert_eq!(count_rows(&store, "google_auth"), 0);
    assert_eq!(count_rows(&store, "chunk_sync_state"), 0);
    assert_eq!(count_rows(&store, "external_events"), 0);
}

fn assert_ids_result(got: Result<Vec<String>, AppError>, want: Result<Vec<String>, AppError>) {
    match (got, want) {
        (Ok(got), Ok(want)) => assert_eq!(got, want),
        (Err(AppError::Validation(_)), Err(AppError::Validation(_))) => {}
        (got, want) => panic!("expected {want:?}, got {got:?}"),
    }
}

#[test_case(None, Ok(vec![]) ; "none_returns_empty")]
#[test_case(Some(""), Ok(vec![]) ; "empty_string_returns_empty")]
#[test_case(Some("   "), Ok(vec![]) ; "whitespace_only_returns_empty")]
#[test_case(Some("[]"), Ok(vec![]) ; "empty_array_returns_empty")]
#[test_case(
    Some(r#"["a","b"]"#),
    Ok(vec!["a".to_owned(), "b".to_owned()]);
    "two_ids_parsed"
)]
#[test_case(Some("not json"), Err(AppError::Validation(String::new())) ; "invalid_json_validation_error")]
#[test_case(Some("[1,2]"), Err(AppError::Validation(String::new())) ; "array_of_numbers_validation_error")]
// test_case passes Option<&str> and a Result by value; the Ok variant uses
// owned Strings so parameters are fully self-contained.
#[allow(clippy::needless_pass_by_value)]
fn parse_pull_calendar_ids_parametrized(
    raw: Option<&str>,
    expected: Result<Vec<String>, AppError>,
) {
    let result = parse_pull_calendar_ids(raw);
    assert_ids_result(result, expected);
}

#[test_case(None, Ok(vec![]) ; "unset_returns_empty")]
#[test_case(Some(r#"["a","b"]"#), Ok(vec!["a".to_owned(), "b".to_owned()]) ; "set_returns_values")]
#[test_case(
    Some("not-json"),
    Err(AppError::Validation(String::new()));
    "malformed_returns_validation_error"
)]
// test_case passes Result<Vec<String>, AppError> by value; the Ok variant uses owned Strings.
#[allow(clippy::needless_pass_by_value)]
fn get_pull_calendars_parametrized(raw: Option<&str>, expected: Result<Vec<String>, AppError>) {
    let store = test_store_with_config(default_config());
    if let Some(v) = raw {
        store.set_config_value("pull_calendar_ids", v).expect("set");
    }
    let result = get_pull_calendars(&store);
    assert_ids_result(result, expected);
}

#[test_case(&["cal-a", "cal-b"], r#"["cal-a","cal-b"]"# ; "two_ids")]
#[test_case(&[], "[]" ; "empty_list")]
fn set_pull_calendars_writes_raw_json(ids: &[&str], expected_raw: &str) {
    let store = test_store_with_config(default_config());
    let owned: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
    set_pull_calendars(&store, &owned).expect("set ok");
    let raw = store
        .get_config_value("pull_calendar_ids")
        .expect("get raw")
        .expect("value must exist");
    assert_eq!(raw, expected_raw);
}

#[test_case(
    vec!["a".to_owned(), "b".to_owned(), "a".to_owned()],
    vec!["a".to_owned(), "b".to_owned()];
    "dedup_preserves_first_seen_order"
)]
#[test_case(
    vec!["x".to_owned(), "y".to_owned(), "z".to_owned(), "y".to_owned()],
    vec!["x".to_owned(), "y".to_owned(), "z".to_owned()];
    "dedup_middle_duplicate"
)]
#[test_case(
    vec!["  cal-a  ".to_owned(), " cal-b".to_owned()],
    vec!["cal-a".to_owned(), "cal-b".to_owned()];
    "trims_whitespace"
)]
#[test_case(
    vec!["cal-1".to_owned(), "cal-2".to_owned(), "cal-3".to_owned()],
    vec!["cal-1".to_owned(), "cal-2".to_owned(), "cal-3".to_owned()];
    "roundtrip"
)]
// test_case passes Vec<String> by value; the inner owned Strings are correct.
#[allow(clippy::needless_pass_by_value)]
fn set_pull_calendars_deduplicates(input: Vec<String>, expected: Vec<String>) {
    let store = test_store_with_config(default_config());
    set_pull_calendars(&store, &input).expect("set ok");
    let ids_back = get_pull_calendars(&store).expect("get ok");
    assert_eq!(ids_back, expected);
}

#[test]
fn set_pull_calendars_blank_id_returns_validation_error_and_does_not_write() {
    let store = test_store_with_config(default_config());
    // Migration 005 seeds pull_calendar_ids to ""; record what it is BEFORE.
    let before = store
        .get_config_value("pull_calendar_ids")
        .expect("get before");

    let ids = vec!["cal-a".to_owned(), "   ".to_owned()];
    let err = set_pull_calendars(&store, &ids).unwrap_err();
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation error, got: {err:?}"
    );

    // The config key must not have changed (no write on error).
    let after = store
        .get_config_value("pull_calendar_ids")
        .expect("get after");
    assert_eq!(
        after, before,
        "config key must be unchanged when set_pull_calendars returns an error"
    );
}

#[test]
fn pull_noop_when_unavailable() {
    let store = store_with_pull_calendars(r#"["cal-a"]"#);

    let sync = MockCalendarSync::new(false, HashMap::new());
    let now = test_now();

    pull_external_events(&store, &sync, now).expect("pull ok");

    assert!(
        sync.recorded_calls().is_empty(),
        "no list_events calls when unavailable"
    );
    assert_eq!(count_rows(&store, "external_events"), 0);
}

#[test]
fn pull_noop_when_no_calendars_selected() {
    // Default seed has pull_calendar_ids = "" → no calendars configured.
    let store = test_store_with_config(default_config());

    let sync = MockCalendarSync::new(true, HashMap::new());
    let now = test_now();

    pull_external_events(&store, &sync, now).expect("pull ok");

    assert!(sync.recorded_calls().is_empty());
}

#[test]
fn pull_mirrors_selected_calendars() {
    let store = store_with_pull_calendars(r#"["cal-a","cal-b"]"#);

    let now = test_now();
    let mut events = HashMap::new();
    events.insert(
        "cal-a".to_owned(),
        vec![
            make_event("cal-a", "ev-a1", "Alpha 1", true, now),
            make_event("cal-a", "ev-a2", "Alpha 2", false, now),
        ],
    );
    events.insert(
        "cal-b".to_owned(),
        vec![make_event("cal-b", "ev-b1", "Beta 1", true, now)],
    );

    let sync = MockCalendarSync::new(true, events);

    pull_external_events(&store, &sync, now).expect("pull ok");

    assert_eq!(
        count_rows(&store, "external_events"),
        3,
        "mirror must hold all provider events"
    );

    // All rows must have updated_at == now (injected timestamp).
    let cfg = store.get_config().expect("config");
    let horizon_end = now + Duration::days(cfg.planning_horizon_days);
    let mirrored = store
        .get_external_events_in_range(now - Duration::days(8), horizon_end)
        .expect("get events");
    for row in &mirrored {
        assert_eq!(
            row.updated_at, now,
            "row {} must have updated_at == now",
            row.event_id
        );
    }
}

#[test]
fn pull_window_is_seven_days_back_to_horizon() {
    let store = store_with_pull_calendars(r#"["cal-a","cal-b"]"#);

    let now = test_now();
    let sync = MockCalendarSync::new(true, HashMap::new());

    pull_external_events(&store, &sync, now).expect("pull ok");

    let calls = sync.recorded_calls();
    assert_eq!(calls.len(), 2, "one call per calendar");

    let config = store.get_config().expect("config");
    let expected_start = now - Duration::days(7);
    let expected_end = now + Duration::days(config.planning_horizon_days);

    for (cal, start, end) in &calls {
        assert_eq!(
            *start, expected_start,
            "window start for {cal} must be now - 7 days"
        );
        assert_eq!(
            *end, expected_end,
            "window end for {cal} must be now + planning_horizon_days"
        );
    }
}

#[test]
fn pull_second_run_applies_remote_changes() {
    let store = store_with_pull_calendars(r#"["cal-a"]"#);

    let now = test_now();

    let events_v1 = single_cal_events(
        "cal-a",
        &[("ev-A", "Original A", true), ("ev-B", "Event B", true)],
        now,
    );
    let sync_v1 = MockCalendarSync::new(true, events_v1);
    pull_external_events(&store, &sync_v1, now).expect("first pull ok");

    let after_first = store
        .get_external_events_in_range(now - Duration::days(8), now + Duration::days(30))
        .expect("get after first");
    let row_a = after_first
        .iter()
        .find(|e| e.event_id == "ev-A")
        .expect("ev-A exists after first pull");
    let row_a_id = row_a.id.clone();

    let now2 = now + Duration::seconds(1);
    let events_v2 = single_cal_events(
        "cal-a",
        &[("ev-A", "Updated A", true), ("ev-C", "New C", false)],
        now2,
    );
    let sync_v2 = MockCalendarSync::new(true, events_v2);
    pull_external_events(&store, &sync_v2, now2).expect("second pull ok");

    let after_second = store
        .get_external_events_in_range(now - Duration::days(8), now2 + Duration::days(30))
        .expect("get after second");

    let event_ids: Vec<&str> = after_second.iter().map(|e| e.event_id.as_str()).collect();
    assert!(event_ids.contains(&"ev-A"), "ev-A must still exist");
    assert!(event_ids.contains(&"ev-C"), "ev-C must be present");
    assert!(!event_ids.contains(&"ev-B"), "ev-B must be removed");

    // ev-A must retain the same row id (upsert, not delete+insert).
    let updated_a = after_second
        .iter()
        .find(|e| e.event_id == "ev-A")
        .expect("ev-A in second pull");
    assert_eq!(
        updated_a.id, row_a_id,
        "ev-A row id must be preserved across updates"
    );
    assert_eq!(updated_a.title, "Updated A", "ev-A title must be updated");
    assert_eq!(
        updated_a.updated_at, now2,
        "ev-A updated_at must reflect second pull time"
    );
}

#[test]
fn pull_error_on_second_calendar_keeps_first() {
    let store = store_with_pull_calendars(r#"["cal-a","cal-b"]"#);

    let now = test_now();
    let events = single_cal_events("cal-a", &[("ev-a1", "From A", true)], now);
    let sync = MockCalendarSync::new(true, events)
        .with_calendar_error("cal-b", AppError::CalendarSync("network error".into()));

    let err = pull_external_events(&store, &sync, now).unwrap_err();
    assert!(
        matches!(err, AppError::CalendarSync(_)),
        "expected CalendarSync error, got: {err:?}"
    );

    // cal-a was processed first → its row must be mirrored.
    assert_eq!(
        count_rows(&store, "external_events"),
        1,
        "cal-a events must be mirrored even when cal-b fails"
    );
    let rows = store
        .get_external_events_in_range(now - Duration::days(8), now + Duration::days(30))
        .expect("get");
    assert_eq!(rows[0].calendar_id, "cal-a");
}

/// Rows for a deselected calendar must be removed on pull; selected calendar
/// rows must survive.
#[test]
fn deselected_calendar_rows_cleared_on_pull() {
    let (store, now, window_start, window_end) = deselection_test_store();

    // Seed rows for both cal-a and cal-b via direct store calls.
    let ev_a = ext_event_record("row-a", "cal-a", "ev-a1", "Event A", true, now);
    let ev_b = ext_event_record("row-b", "cal-b", "ev-b1", "Event B", true, now);
    store
        .replace_external_events_in_window("cal-a", window_start, window_end, &[ev_a])
        .expect("seed cal-a");
    store
        .replace_external_events_in_window("cal-b", window_start, window_end, &[ev_b])
        .expect("seed cal-b");
    assert_eq!(count_rows(&store, "external_events"), 2);

    // Select only cal-a; cal-b is deselected.
    select_pull_calendars(&store, r#"["cal-a"]"#);

    let sync = MockCalendarSync::new(true, HashMap::new());
    pull_external_events(&store, &sync, now).expect("pull ok");

    // cal-b rows must be gone; cal-a rows survive (empty batch → window cleared
    // but historical rows outside the window are retained; here the row is
    // inside the window so it gets deleted by the empty batch — that is correct
    // pull behaviour: the remote returned zero events for cal-a this window).
    // What the test asserts is that cal-b's row was cleaned up by the
    // deselected-calendar sweep, not by the per-calendar window replace.
    let all_ids: Vec<String> = {
        let events = store
            .get_external_events_in_range(window_start, window_end)
            .expect("get");
        events.iter().map(|e| e.calendar_id.clone()).collect()
    };
    assert!(
        !all_ids.contains(&"cal-b".to_owned()),
        "cal-b rows must be removed for deselected calendar"
    );
}

#[test]
fn deselected_calendar_rows_cleared_idempotent() {
    let (store, now, window_start, window_end) = deselection_test_store();
    let ev_b = ext_event_record("row-b", "cal-b", "ev-b1", "Event B", true, now);
    store
        .replace_external_events_in_window("cal-b", window_start, window_end, &[ev_b])
        .expect("seed cal-b");

    select_pull_calendars(&store, r#"["cal-a"]"#);

    let sync = MockCalendarSync::new(true, HashMap::new());
    pull_external_events(&store, &sync, now).expect("first pull ok");
    // Second pull: cal-b is already gone — must not error.
    pull_external_events(&store, &sync, now).expect("second pull ok (idempotent)");
}

#[test]
fn selected_calendar_rows_not_touched_on_pull() {
    let store = test_store_with_config(default_config());

    let now = test_now();
    let window_start = now - Duration::days(60);
    let window_end = now - Duration::days(50);
    // Seed a historical event far outside the pull window so
    // replace_external_events_in_window won't delete it.
    let ev_a = ext_event_record(
        "hist-a",
        "cal-a",
        "hist-ev",
        "Historical",
        false,
        now - Duration::days(55),
    );
    store
        .replace_external_events_in_window("cal-a", window_start, window_end, &[ev_a])
        .expect("seed hist event");

    select_pull_calendars(&store, r#"["cal-a"]"#);

    // The historical row is outside the pull window → not removed by
    // replace_external_events_in_window. The deselected-calendar cleanup
    // must not have touched cal-a either.
    assert_empty_pull_preserves_count(
        &store,
        now,
        1,
        "selected calendar historical row must not be removed by cleanup",
    );
}

#[test]
fn pull_and_reschedule_runs_pull_and_reschedule() {
    let store = std::sync::Arc::new(test_store());
    select_pull_calendars(&store, r#"["cal-a"]"#);

    let now = test_now();
    let events = single_cal_events("cal-a", &[("ev-1", "Event 1", true)], now);
    let sync = MockCalendarSync::new(true, events);
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(&store);

    let result = pull_and_reschedule(store.as_ref(), &sync, &scheduler, &trigger, now);

    assert!(
        result.is_ok(),
        "pull_and_reschedule must succeed: {result:?}"
    );
    let _: ScheduleResult = result.expect("ok");

    assert_eq!(
        count_rows(&store, "external_events"),
        1,
        "mirror row must exist after pull_and_reschedule"
    );
}

#[test]
fn pull_and_reschedule_pull_error_propagates() {
    let store = std::sync::Arc::new(test_store());
    select_pull_calendars(&store, r#"["cal-err"]"#);

    let now = test_now();
    let sync = MockCalendarSync::new(true, HashMap::new())
        .with_calendar_error("cal-err", AppError::CalendarSync("network timeout".into()));
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(&store);

    let err = pull_and_reschedule(store.as_ref(), &sync, &scheduler, &trigger, now).unwrap_err();
    assert!(
        matches!(err, AppError::CalendarSync(_)),
        "expected CalendarSync error, got: {err:?}"
    );

    assert_eq!(
        count_rows(&store, "external_events"),
        0,
        "no mirror rows must exist when pull errors"
    );
}

#[test]
fn pull_events_outside_window_retained() {
    let store = store_with_pull_calendars(r#"["cal-a"]"#);

    let now = test_now();

    // Seed a historical event 60 days in the past (safely outside any pull window).
    let hist_start = now - Duration::days(60);
    let hist_end = hist_start + Duration::hours(1);
    let historical = ext_event_record(
        "hist-row",
        "cal-a",
        "hist-ev",
        "Ancient event",
        false,
        hist_start,
    );
    // Place it in its own narrow window that won't overlap the pull window.
    store
        .replace_external_events_in_window(
            "cal-a",
            hist_start - Duration::hours(1),
            hist_end + Duration::hours(1),
            &[historical],
        )
        .expect("seed historical event");

    // Pull returns zero events for cal-a (remote side is empty in this window).
    // Verify it's there before and still present after — it's outside the pull window.
    assert_empty_pull_preserves_count(
        &store,
        now,
        1,
        "historical event outside pull window must be retained",
    );
    let rows = store
        .get_external_events_in_range(
            hist_start - Duration::hours(1),
            hist_end + Duration::hours(1),
        )
        .expect("get historical");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_id, "hist-ev");
}
