// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Single-row `ExternalEventStore` tests: `upsert_external_event`,
//! `get_external_event`, and `delete_external_event` (plus their `TxStore`
//! variants). The window-based `replace_external_events_in_window` path lives
//! in the sibling `sync_state` module.

use crate::db::sqlite::SqliteStore;
use crate::domain::models::ExternalEventRecord;
use crate::traits::storage::{ExternalEventStore, Store};

use super::make_external_event;

#[test]
fn upsert_external_event_inserts_and_get_round_trips() {
    let store = SqliteStore::new_in_memory();

    let mut event = make_external_event("evt-single", "cal-a", 8, 9);
    event.description = Some("Single-row description".to_owned());

    store.upsert_external_event(&event).expect("upsert insert");

    let got = store
        .get_external_event("cal-a", "evt-single")
        .expect("get")
        .expect("Some after upsert");
    assert_eq!(got, event, "single-row read must round-trip all fields");
}

#[test]
fn upsert_external_event_conflict_preserves_id_and_updates_fields() {
    let store = SqliteStore::new_in_memory();

    let original = ExternalEventRecord {
        id: "orig-single-id".to_owned(),
        ..make_external_event("evt-conflict", "cal-a", 8, 9)
    };
    store
        .upsert_external_event(&original)
        .expect("upsert original");

    let updated = ExternalEventRecord {
        id: "new-single-id".to_owned(), // must be ignored on conflict
        title: "Updated single title".to_owned(),
        description: Some("now with a description".to_owned()),
        busy: false,
        declined: true,
        ..make_external_event("evt-conflict", "cal-a", 9, 10)
    };
    store
        .upsert_external_event(&updated)
        .expect("upsert conflict");

    let got = store
        .get_external_event("cal-a", "evt-conflict")
        .expect("get")
        .expect("Some");
    assert_eq!(got.id, "orig-single-id", "original id must be preserved");
    assert_eq!(got.title, "Updated single title", "title must be updated");
    assert_eq!(got.description, Some("now with a description".to_owned()));
    assert!(!got.busy, "busy must be updated");
    assert!(got.declined, "declined must be updated");
    assert_eq!(got.start_time, updated.start_time, "start must be updated");
}

#[test]
fn get_external_event_returns_none_when_absent() {
    let store = SqliteStore::new_in_memory();

    // Empty store → None.
    assert!(
        store
            .get_external_event("cal-a", "nope")
            .expect("get empty")
            .is_none(),
        "absent event must be None"
    );

    // Seed one event, then verify the (calendar_id, event_id) key discriminates.
    let event = make_external_event("evt-key", "cal-a", 8, 9);
    store.upsert_external_event(&event).expect("seed");

    assert!(
        store
            .get_external_event("cal-b", "evt-key")
            .expect("get other calendar")
            .is_none(),
        "same event_id on a different calendar must be None"
    );
    assert!(
        store
            .get_external_event("cal-a", "evt-other")
            .expect("get other event")
            .is_none(),
        "different event_id on the same calendar must be None"
    );
    assert!(
        store
            .get_external_event("cal-a", "evt-key")
            .expect("get match")
            .is_some(),
        "exact key match must be Some"
    );
}

#[test]
fn delete_external_event_removes_only_target_row() {
    let store = SqliteStore::new_in_memory();

    let keep = make_external_event("evt-keep", "cal-a", 8, 9);
    let drop = make_external_event("evt-drop", "cal-a", 10, 11);
    store.upsert_external_event(&keep).expect("seed keep");
    store.upsert_external_event(&drop).expect("seed drop");

    store
        .delete_external_event("cal-a", "evt-drop")
        .expect("delete");

    assert!(
        store
            .get_external_event("cal-a", "evt-drop")
            .expect("get dropped")
            .is_none(),
        "deleted event must be gone"
    );
    assert!(
        store
            .get_external_event("cal-a", "evt-keep")
            .expect("get kept")
            .is_some(),
        "sibling event must remain"
    );
}

#[test]
fn delete_external_event_idempotent_when_absent() {
    let store = SqliteStore::new_in_memory();
    // Deleting a row that does not exist must succeed without error.
    store
        .delete_external_event("cal-a", "evt-missing")
        .expect("idempotent delete must not error");
}

#[test]
fn tx_store_single_external_event_methods() {
    let store = SqliteStore::new_in_memory();

    let event = make_external_event("evt-tx-single", "cal-tx", 8, 9);

    store
        .with_tx(&mut |tx| {
            // upsert + get via TxStore.
            tx.upsert_external_event(&event)?;
            let got = tx.get_external_event("cal-tx", "evt-tx-single")?;
            assert!(got.is_some(), "TxStore get must find the upserted row");

            // delete + confirm via TxStore.
            tx.delete_external_event("cal-tx", "evt-tx-single")?;
            let after = tx.get_external_event("cal-tx", "evt-tx-single")?;
            assert!(after.is_none(), "row must be gone after TxStore delete");
            Ok(())
        })
        .expect("with_tx single-row methods");
}

#[test]
fn upsert_external_event_round_trips_all_day_flag() {
    let store = SqliteStore::new_in_memory();

    // All-day record persists and reads back as all_day = true.
    let all_day = ExternalEventRecord {
        all_day: true,
        ..make_external_event("evt-allday", "cal-a", 8, 9)
    };
    store
        .upsert_external_event(&all_day)
        .expect("upsert all-day");
    let got = store
        .get_external_event("cal-a", "evt-allday")
        .expect("get")
        .expect("Some after upsert");
    assert!(
        got.all_day,
        "all_day flag must survive the store round-trip"
    );

    // A plain timed record round-trips as all_day = false.
    let timed = make_external_event("evt-timed", "cal-a", 10, 11);
    store.upsert_external_event(&timed).expect("upsert timed");
    let got_timed = store
        .get_external_event("cal-a", "evt-timed")
        .expect("get")
        .expect("Some");
    assert!(!got_timed.all_day, "a timed event stays all_day = false");
}
