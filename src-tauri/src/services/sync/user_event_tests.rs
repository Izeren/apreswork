// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the user-owned event write-through service functions —
//! `create_user_event`, `update_user_event`, and `delete_user_event` (G11).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use super::test_util::{count_rows, make_trigger};
use super::{create_user_event, delete_user_event, update_user_event};
use crate::db::sqlite::SqliteStore;
use crate::domain::models::ExternalEventRecord;
use crate::error::AppError;
use crate::scheduler::engine::DefaultScheduler;
use crate::services::trigger::RescheduleTrigger;
use crate::test_support::calendar::{MockCalendarSync, UserEventOp};
use crate::test_support::test_store;
use crate::traits::calendar_sync::UserEventPayload;
use crate::traits::storage::ExternalEventStore;

/// A valid future user-event payload (2h–3h from `now`).
fn valid_payload(now: DateTime<Utc>) -> UserEventPayload {
    UserEventPayload {
        title: "Dinner".to_owned(),
        description: Some("with friends".to_owned()),
        start: now + Duration::hours(2),
        end: now + Duration::hours(3),
        all_day: false,
    }
}

/// Seed one mirrored external event; returns its stable DB row id.
fn seed_external_event(
    store: &SqliteStore,
    calendar_id: &str,
    event_id: &str,
    title: &str,
    now: DateTime<Utc>,
) -> String {
    let id = format!("seed-{event_id}");
    let record = ExternalEventRecord {
        id: id.clone(),
        calendar_id: calendar_id.to_owned(),
        event_id: event_id.to_owned(),
        title: title.to_owned(),
        description: None,
        start_time: now + Duration::hours(1),
        end_time: now + Duration::hours(2),
        busy: true,
        declined: false,
        all_day: false,
        updated_at: now,
    };
    store.upsert_external_event(&record).expect("seed upsert");
    id
}

/// The mock provider, scheduler, and reschedule trigger shared by every
/// test that doesn't need provider-error injection.
fn bare_rig(
    store: &std::sync::Arc<SqliteStore>,
) -> (MockCalendarSync, DefaultScheduler, RescheduleTrigger) {
    let sync = MockCalendarSync::new(true, HashMap::new());
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(store);
    (sync, scheduler, trigger)
}

/// A fresh store, clock, and [`bare_rig`] — the setup shared by every test
/// that starts from an empty mirror (nothing seeded yet).
fn fresh_rig() -> (
    std::sync::Arc<SqliteStore>,
    DateTime<Utc>,
    MockCalendarSync,
    DefaultScheduler,
    RescheduleTrigger,
) {
    let store = std::sync::Arc::new(test_store());
    let now = Utc::now();
    let (sync, scheduler, trigger) = bare_rig(&store);
    (store, now, sync, scheduler, trigger)
}

/// Build a rig, call `create_user_event` for `"cal-a"` with `payload`/`now`,
/// and unwrap the result. Returns `(store, sync, record)` for assertions.
fn create_ok(
    now: DateTime<Utc>,
    payload: &UserEventPayload,
) -> (
    std::sync::Arc<SqliteStore>,
    MockCalendarSync,
    ExternalEventRecord,
) {
    let store = std::sync::Arc::new(test_store());
    let (sync, scheduler, trigger) = bare_rig(&store);
    let record = create_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        payload,
        now,
    )
    .expect("create ok");
    (store, sync, record)
}

// create_user_event ──────────────────────────────────────────────────────────

#[test]
fn create_user_event_writes_through_mirrors_and_reschedules() {
    let now = Utc::now();
    let payload = valid_payload(now);
    let (store, sync, record) = create_ok(now, &payload);

    // Returned record echoes the payload with provider-owned identity.
    assert_eq!(record.calendar_id, "cal-a");
    assert_eq!(record.event_id, "mock-ev-0");
    assert_eq!(record.title, "Dinner");
    assert_eq!(record.description.as_deref(), Some("with friends"));
    assert_eq!(record.start_time, payload.start);
    assert_eq!(record.end_time, payload.end);
    assert!(record.busy, "fresh own event is busy");
    assert!(!record.declined);
    assert!(
        !record.all_day,
        "a timed payload mirrors as all_day = false"
    );
    assert_eq!(record.updated_at, now, "updated_at is the injected now");

    // Provider recorded exactly one Create op with the payload verbatim.
    assert_eq!(
        sync.get_recorded_user_event_ops(),
        vec![UserEventOp::Create {
            calendar_id: "cal-a".to_owned(),
            payload,
        }]
    );

    // Mirror holds the new row under the provider-assigned event id.
    assert_eq!(count_rows(&store, "external_events"), 1);
    let mirrored = store
        .get_external_event("cal-a", "mock-ev-0")
        .expect("get")
        .expect("mirror row exists");
    assert_eq!(mirrored.id, record.id);
    assert_eq!(mirrored.title, "Dinner");
}

#[test]
fn create_user_event_all_day_mirrors_all_day_flag() {
    let now = Utc::now();
    let payload = UserEventPayload {
        all_day: true,
        ..valid_payload(now)
    };
    let (store, _sync, record) = create_ok(now, &payload);

    // The all-day flag flows payload → provider echo → mirror record and row.
    assert!(
        record.all_day,
        "an all-day payload mirrors as all_day = true"
    );
    let mirrored = store
        .get_external_event("cal-a", "mock-ev-0")
        .expect("get")
        .expect("mirror row exists");
    assert!(mirrored.all_day, "the mirror row persists the all_day flag");
}

#[test]
fn create_user_event_provider_error_leaves_no_mirror_row() {
    let store = std::sync::Arc::new(test_store());
    let now = Utc::now();
    let sync = MockCalendarSync::new(true, HashMap::new()).with_user_event_error("boom");
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(&store);

    let err = create_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        &valid_payload(now),
        now,
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::CalendarSync(_)),
        "expected CalendarSync, got: {err:?}"
    );
    assert_eq!(
        count_rows(&store, "external_events"),
        0,
        "provider failure must not mirror anything"
    );
}

/// Which field of the create request is invalid.
#[derive(Debug, Clone, Copy)]
enum BadInput {
    BlankCalendar,
    BlankTitle,
    NonPositiveDuration,
}

#[test_case(BadInput::BlankCalendar ; "blank calendar id")]
#[test_case(BadInput::BlankTitle ; "blank title")]
#[test_case(BadInput::NonPositiveDuration ; "start not before end")]
fn create_user_event_rejects_invalid_input(bad: BadInput) {
    let (store, now, sync, scheduler, trigger) = fresh_rig();

    let (calendar_id, payload) = match bad {
        BadInput::BlankCalendar => ("   ", valid_payload(now)),
        BadInput::BlankTitle => (
            "cal-a",
            UserEventPayload {
                title: "   ".to_owned(),
                ..valid_payload(now)
            },
        ),
        // start == end is a zero-length event: start is not strictly before end.
        BadInput::NonPositiveDuration => (
            "cal-a",
            UserEventPayload {
                start: now + Duration::hours(3),
                end: now + Duration::hours(3),
                ..valid_payload(now)
            },
        ),
    };

    let err = create_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        calendar_id,
        &payload,
        now,
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation, got: {err:?}"
    );
    // Validation fails before any provider write or mirror mutation.
    assert!(sync.get_recorded_user_event_ops().is_empty());
    assert_eq!(count_rows(&store, "external_events"), 0);
}

// update_user_event ──────────────────────────────────────────────────────────

#[test]
fn update_user_event_writes_through_remirrors_preserving_row_id() {
    let store = std::sync::Arc::new(test_store());
    let now = Utc::now();
    let seed_id = seed_external_event(store.as_ref(), "cal-a", "ev-1", "Original", now);

    let (sync, scheduler, trigger) = bare_rig(&store);
    let payload = UserEventPayload {
        title: "Updated dinner".to_owned(),
        ..valid_payload(now)
    };

    let record = update_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "ev-1",
        &payload,
        now,
    )
    .expect("update ok");

    assert_eq!(record.title, "Updated dinner");
    assert_eq!(record.event_id, "ev-1");
    assert_eq!(
        record.id, seed_id,
        "returned record echoes the persisted row id, not a throwaway surrogate"
    );

    // Provider recorded the Update op with the new payload.
    assert_eq!(
        sync.get_recorded_user_event_ops(),
        vec![UserEventOp::Update {
            calendar_id: "cal-a".to_owned(),
            event_id: "ev-1".to_owned(),
            payload,
        }]
    );

    // Mirror updated in place: same DB id (upsert on conflict), new title.
    let back = store
        .get_external_event("cal-a", "ev-1")
        .expect("get")
        .expect("still mirrored");
    assert_eq!(back.id, seed_id, "row id preserved across update");
    assert_eq!(back.title, "Updated dinner");
    assert_eq!(count_rows(&store, "external_events"), 1);
}

#[test]
fn update_user_event_unmirrored_is_not_found_and_skips_provider() {
    let (store, now, sync, scheduler, trigger) = fresh_rig();

    let err = update_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "missing",
        &valid_payload(now),
        now,
    )
    .unwrap_err();
    assert!(
        matches!(err, AppError::NotFound { .. }),
        "expected NotFound, got: {err:?}"
    );
    assert!(
        sync.get_recorded_user_event_ops().is_empty(),
        "provider must not be called for an unmirrored event"
    );
}

#[test]
fn update_user_event_provider_error_leaves_mirror_unchanged() {
    let store = std::sync::Arc::new(test_store());
    let now = Utc::now();
    seed_external_event(store.as_ref(), "cal-a", "ev-1", "Original", now);
    let sync = MockCalendarSync::new(true, HashMap::new()).with_user_event_error("boom");
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(&store);

    let err = update_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "ev-1",
        &UserEventPayload {
            title: "Should not persist".to_owned(),
            ..valid_payload(now)
        },
        now,
    )
    .unwrap_err();
    assert!(matches!(err, AppError::CalendarSync(_)), "got: {err:?}");

    let back = store
        .get_external_event("cal-a", "ev-1")
        .expect("get")
        .expect("row still present");
    assert_eq!(back.title, "Original", "mirror unchanged on provider error");
}

// delete_user_event ──────────────────────────────────────────────────────────

#[test]
fn delete_user_event_writes_through_and_removes_mirror() {
    let store = std::sync::Arc::new(test_store());
    let now = Utc::now();
    seed_external_event(store.as_ref(), "cal-a", "ev-1", "Doomed", now);
    let (sync, scheduler, trigger) = bare_rig(&store);

    delete_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "ev-1",
        now,
    )
    .expect("delete ok");

    assert!(
        store
            .get_external_event("cal-a", "ev-1")
            .expect("get")
            .is_none(),
        "mirror row removed"
    );
    assert_eq!(
        sync.get_recorded_user_event_ops(),
        vec![UserEventOp::Delete {
            calendar_id: "cal-a".to_owned(),
            event_id: "ev-1".to_owned(),
        }]
    );
}

#[test]
fn delete_user_event_unmirrored_is_not_found_and_skips_provider() {
    let (store, now, sync, scheduler, trigger) = fresh_rig();

    let err = delete_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "missing",
        now,
    )
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound { .. }), "got: {err:?}");
    assert!(sync.get_recorded_user_event_ops().is_empty());
}

#[test]
fn delete_user_event_provider_error_keeps_mirror_row() {
    let store = std::sync::Arc::new(test_store());
    let now = Utc::now();
    seed_external_event(store.as_ref(), "cal-a", "ev-1", "Survivor", now);
    let sync = MockCalendarSync::new(true, HashMap::new()).with_user_event_error("boom");
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(&store);

    let err = delete_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "ev-1",
        now,
    )
    .unwrap_err();
    assert!(matches!(err, AppError::CalendarSync(_)), "got: {err:?}");
    assert!(
        store
            .get_external_event("cal-a", "ev-1")
            .expect("get")
            .is_some(),
        "mirror row must survive a failed provider delete"
    );
}

/// Both update and delete reject a blank `event_id` at the trust boundary,
/// before touching the provider.
#[test]
fn update_and_delete_reject_blank_event_id() {
    let (store, now, sync, scheduler, trigger) = fresh_rig();

    let update_err = update_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "   ",
        &valid_payload(now),
        now,
    )
    .unwrap_err();
    assert!(
        matches!(update_err, AppError::Validation(_)),
        "got: {update_err:?}"
    );

    let delete_err = delete_user_event(
        store.as_ref(),
        &sync,
        &scheduler,
        &trigger,
        "cal-a",
        "   ",
        now,
    )
    .unwrap_err();
    assert!(
        matches!(delete_err, AppError::Validation(_)),
        "got: {delete_err:?}"
    );

    assert!(sync.get_recorded_user_event_ops().is_empty());
}
