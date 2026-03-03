// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `ExternalEventStore`, `GoogleAuthStore`, and `ChunkSyncStateStore` impls.

use chrono::{TimeZone, Utc};
use test_case::test_case;

use super::make_external_event;
use crate::db::sqlite::SqliteStore;
use crate::domain::models::{ChunkSyncState, ExternalEventRecord, GoogleAuthState};
use crate::traits::storage::{
    ChunkSyncStateStore, ConfigStore, ExternalEventStore, GoogleAuthStore, Store,
};

fn window_start() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 13, 0, 0, 0).unwrap()
}

fn window_end() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 14, 0, 0, 0).unwrap()
}

fn seed_cal_a_and_b(store: &SqliteStore, event_id_a: &str, event_id_b: &str) {
    let ev_a = make_external_event(event_id_a, "cal-a", 8, 9);
    let ev_b = make_external_event(event_id_b, "cal-b", 10, 11);
    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[ev_a])
        .expect("seed cal-a");
    store
        .replace_external_events_in_window("cal-b", window_start(), window_end(), &[ev_b])
        .expect("seed cal-b");
}

#[test]
fn replace_and_get_round_trips_all_fields() {
    let store = SqliteStore::new_in_memory();

    let mut event1 = make_external_event("evt-rt-1", "cal-a", 8, 9);
    event1.description = Some("A description".to_owned());
    event1.busy = true;
    event1.declined = false;

    let mut event2 = make_external_event("evt-rt-2", "cal-a", 10, 11);
    event2.description = None;
    event2.busy = false;
    event2.declined = true;

    store
        .replace_external_events_in_window(
            "cal-a",
            window_start(),
            window_end(),
            &[event1.clone(), event2.clone()],
        )
        .expect("replace");

    let mut results = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get");
    results.sort_by(|a, b| a.event_id.cmp(&b.event_id));

    assert_eq!(results.len(), 2);

    let r1 = results
        .iter()
        .find(|e| e.event_id == "evt-rt-1")
        .expect("find evt-rt-1");
    assert_eq!(r1, &event1);

    let r2 = results
        .iter()
        .find(|e| e.event_id == "evt-rt-2")
        .expect("find evt-rt-2");
    assert_eq!(r2, &event2);
}

#[test]
fn upsert_preserves_original_id() {
    let store = SqliteStore::new_in_memory();

    let original = ExternalEventRecord {
        id: "orig-id".to_owned(),
        event_id: "evt-upsert".to_owned(),
        title: "Original".to_owned(),
        ..make_external_event("evt-upsert", "cal-a", 8, 9)
    };

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[original])
        .expect("first replace");

    let updated = ExternalEventRecord {
        id: "new-id".to_owned(), // different id — should be ignored on conflict
        event_id: "evt-upsert".to_owned(),
        title: "Updated title".to_owned(),
        ..make_external_event("evt-upsert", "cal-a", 9, 10)
    };

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[updated])
        .expect("second replace");

    let results = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "orig-id", "original id must be preserved");
    assert_eq!(results[0].title, "Updated title", "title must be updated");
}

#[test]
fn replace_removes_missing_event() {
    let store = SqliteStore::new_in_memory();

    let ev_keep = make_external_event("evt-keep", "cal-a", 8, 9);
    let ev_remove = make_external_event("evt-remove", "cal-a", 10, 11);

    store
        .replace_external_events_in_window(
            "cal-a",
            window_start(),
            window_end(),
            &[ev_keep.clone(), ev_remove],
        )
        .expect("seed two events");

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[ev_keep])
        .expect("replace with one");

    let results = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event_id, "evt-keep");
}

#[test]
fn history_retention() {
    let store = SqliteStore::new_in_memory();

    // Event A: calendar "cal-a", on 2026-03-12 — OUTSIDE the window (2026-03-13).
    let ev_a = ExternalEventRecord {
        id: uuid::Uuid::now_v7().to_string(),
        calendar_id: "cal-a".to_owned(),
        event_id: "evt-hist-a".to_owned(),
        title: "Historical event".to_owned(),
        description: None,
        start_time: Utc.with_ymd_and_hms(2026, 3, 12, 10, 0, 0).unwrap(),
        end_time: Utc.with_ymd_and_hms(2026, 3, 12, 11, 0, 0).unwrap(),
        busy: true,
        declined: false,
        all_day: false,
        updated_at: Utc.with_ymd_and_hms(2026, 3, 12, 0, 0, 0).unwrap(),
    };
    let prev_day_start = Utc.with_ymd_and_hms(2026, 3, 12, 0, 0, 0).unwrap();
    let prev_day_end = Utc.with_ymd_and_hms(2026, 3, 13, 0, 0, 0).unwrap();
    store
        .replace_external_events_in_window("cal-a", prev_day_start, prev_day_end, &[ev_a])
        .expect("seed cal-a history");

    let ev_b = make_external_event("evt-hist-b", "cal-b", 10, 11);
    store
        .replace_external_events_in_window("cal-b", window_start(), window_end(), &[ev_b])
        .expect("seed cal-b event");

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[])
        .expect("empty replace for cal-a");

    let all_a = store
        .get_external_events_in_range(prev_day_start, prev_day_end)
        .expect("get cal-a range");
    assert!(
        all_a.iter().any(|e| e.event_id == "evt-hist-a"),
        "event A must survive (outside window)"
    );

    let all_b = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get cal-b range");
    assert!(
        all_b.iter().any(|e| e.event_id == "evt-hist-b"),
        "event B must survive (different calendar)"
    );
}

#[test]
fn empty_batch_clears_window_for_calendar_only() {
    let store = SqliteStore::new_in_memory();

    seed_cal_a_and_b(&store, "evt-cl-a", "evt-cl-b");

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[])
        .expect("clear cal-a");

    let results = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get");

    assert!(
        results.iter().all(|e| e.event_id != "evt-cl-a"),
        "cal-a event must be gone"
    );
    assert!(
        results.iter().any(|e| e.event_id == "evt-cl-b"),
        "cal-b event must remain"
    );
}

#[test_case("evt-ends-at-start", Utc.with_ymd_and_hms(2026, 3, 13, 9, 0, 0).unwrap(), Utc.with_ymd_and_hms(2026, 3, 13, 10, 0, 0).unwrap(); "ends_at_start")]
#[test_case("evt-starts-at-end", Utc.with_ymd_and_hms(2026, 3, 13, 12, 0, 0).unwrap(), Utc.with_ymd_and_hms(2026, 3, 13, 13, 0, 0).unwrap(); "starts_at_end")]
fn get_range_excludes_touching_boundary(
    event_id: &str,
    event_start: chrono::DateTime<Utc>,
    event_end: chrono::DateTime<Utc>,
) {
    let store = SqliteStore::new_in_memory();
    let range_start = Utc.with_ymd_and_hms(2026, 3, 13, 10, 0, 0).unwrap();
    let range_end = Utc.with_ymd_and_hms(2026, 3, 13, 12, 0, 0).unwrap();
    let ev = ExternalEventRecord {
        id: uuid::Uuid::now_v7().to_string(),
        calendar_id: "cal-bnd".to_owned(),
        event_id: event_id.to_owned(),
        title: format!("{event_id} range boundary"),
        description: None,
        start_time: event_start,
        end_time: event_end,
        busy: true,
        declined: false,
        all_day: false,
        updated_at: Utc.with_ymd_and_hms(2026, 3, 13, 0, 0, 0).unwrap(),
    };
    let seed_start = Utc.with_ymd_and_hms(2026, 3, 13, 0, 0, 0).unwrap();
    let seed_end = Utc.with_ymd_and_hms(2026, 3, 14, 0, 0, 0).unwrap();
    store
        .replace_external_events_in_window("cal-bnd", seed_start, seed_end, &[ev])
        .expect("seed boundary event");
    let results = store
        .get_external_events_in_range(range_start, range_end)
        .expect("get");
    assert!(
        results.is_empty(),
        "events touching the boundary must be excluded, got: {results:?}"
    );
}

#[test]
fn clear_all_external_events_empties_table() {
    let store = SqliteStore::new_in_memory();

    let ev1 = make_external_event("evt-clr-1", "cal-a", 8, 9);
    let ev2 = make_external_event("evt-clr-2", "cal-b", 10, 11);

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[ev1])
        .expect("seed cal-a");
    store
        .replace_external_events_in_window("cal-b", window_start(), window_end(), &[ev2])
        .expect("seed cal-b");

    store
        .clear_all_external_events()
        .expect("clear all external events");

    let results = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get after clear");
    assert!(results.is_empty(), "table must be empty after clear");
}

#[test]
fn google_auth_lifecycle() {
    let store = SqliteStore::new_in_memory();

    assert!(
        store.get_google_auth().expect("get 1").is_none(),
        "expect None before any set"
    );

    let ts = Utc.with_ymd_and_hms(2026, 3, 13, 8, 0, 0).unwrap();
    let auth_full = GoogleAuthState {
        calendar_id: Some("cal-id-123".to_owned()),
        connected_at: Some(ts),
    };

    store.set_google_auth(&auth_full).expect("set full");
    let got = store
        .get_google_auth()
        .expect("get 2")
        .expect("Some after set");
    assert_eq!(got.calendar_id, Some("cal-id-123".to_owned()));
    assert_eq!(got.connected_at, Some(ts));

    let auth_empty = GoogleAuthState {
        calendar_id: None,
        connected_at: None,
    };
    store.set_google_auth(&auth_empty).expect("set empty");
    let got_empty = store
        .get_google_auth()
        .expect("get 3")
        .expect("Some after overwrite");
    assert!(got_empty.calendar_id.is_none());
    assert!(got_empty.connected_at.is_none());

    store.clear_google_auth().expect("clear");
    assert!(
        store.get_google_auth().expect("get 4").is_none(),
        "expect None after clear"
    );
}

#[test]
fn config_value_get_set() {
    let store = SqliteStore::new_in_memory();

    let provider = store
        .get_config_value("sync_provider")
        .expect("get seeded key");
    assert_eq!(provider, Some("google".to_owned()));

    let missing = store
        .get_config_value("this_key_does_not_exist")
        .expect("get missing key");
    assert!(missing.is_none());

    store
        .set_config_value("my_test_key", "my_value")
        .expect("set");
    let got = store
        .get_config_value("my_test_key")
        .expect("get after set");
    assert_eq!(got, Some("my_value".to_owned()));

    store
        .set_config_value("my_test_key", "updated_value")
        .expect("overwrite");
    let got2 = store
        .get_config_value("my_test_key")
        .expect("get after overwrite");
    assert_eq!(got2, Some("updated_value".to_owned()));
}

#[test]
fn clear_all_chunk_sync_state() {
    let (store, _t) = store_with_chunk_sync_state("c-sync-1", "gcal-ev-css");

    store
        .clear_all_chunk_sync_state()
        .expect("clear chunk sync state");

    let conn = store.conn.lock().expect("lock");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunk_sync_state", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(count, 0, "chunk_sync_state must be empty after clear");
}

#[test]
fn delete_external_events_for_calendar_removes_only_target() {
    let store = SqliteStore::new_in_memory();

    seed_cal_a_and_b(&store, "evt-del-a", "evt-del-b");

    store
        .delete_external_events_for_calendar("cal-b")
        .expect("delete cal-b");

    let remaining = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get after delete");

    assert_eq!(remaining.len(), 1, "only cal-a row must remain");
    assert_eq!(remaining[0].calendar_id, "cal-a");
}

#[test]
fn delete_external_events_for_calendar_noop_when_absent() {
    let store = SqliteStore::new_in_memory();
    // Deleting a calendar that has no rows must succeed without error.
    store
        .delete_external_events_for_calendar("cal-missing")
        .expect("delete missing calendar must not error");

    let remaining = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get");
    assert!(remaining.is_empty());
}

#[test]
fn get_mirrored_calendar_ids_returns_distinct_sorted() {
    let store = SqliteStore::new_in_memory();

    let ev_a1 = make_external_event("evt-m-a1", "cal-a", 8, 9);
    let ev_a2 = make_external_event("evt-m-a2", "cal-a", 10, 11);
    let ev_b = make_external_event("evt-m-b", "cal-b", 8, 9);

    store
        .replace_external_events_in_window("cal-a", window_start(), window_end(), &[ev_a1, ev_a2])
        .expect("seed cal-a");
    store
        .replace_external_events_in_window("cal-b", window_start(), window_end(), &[ev_b])
        .expect("seed cal-b");

    let ids = store.get_mirrored_calendar_ids().expect("get mirrored ids");
    assert_eq!(ids, vec!["cal-a", "cal-b"], "must be distinct and sorted");
}

#[test]
fn get_mirrored_calendar_ids_empty_when_no_events() {
    let store = SqliteStore::new_in_memory();
    let ids = store
        .get_mirrored_calendar_ids()
        .expect("get mirrored ids empty");
    assert!(ids.is_empty(), "must be empty when no events");
}

#[test]
fn tx_store_new_external_event_methods() {
    let store = SqliteStore::new_in_memory();

    seed_cal_a_and_b(&store, "evt-tx-a", "evt-tx-b");

    store
        .with_tx(&mut |tx| {
            let ids = tx.get_mirrored_calendar_ids()?;
            assert_eq!(ids, vec!["cal-a", "cal-b"]);

            tx.delete_external_events_for_calendar("cal-b")?;
            let ids_after = tx.get_mirrored_calendar_ids()?;
            assert_eq!(ids_after, vec!["cal-a"]);
            Ok(())
        })
        .expect("with_tx new methods");

    // Changes committed — verify outside the transaction.
    let ids = store.get_mirrored_calendar_ids().expect("get after tx");
    assert_eq!(ids, vec!["cal-a"]);
}

#[test]
fn with_tx_through_tx_store_impls() {
    let store = SqliteStore::new_in_memory();

    let event = make_external_event("evt-tx", "cal-tx", 10, 11);

    store
        .with_tx(&mut |tx| {
            tx.replace_external_events_in_window(
                "cal-tx",
                window_start(),
                window_end(),
                std::slice::from_ref(&event),
            )?;
            let events = tx.get_external_events_in_range(window_start(), window_end())?;
            assert_eq!(events.len(), 1, "event visible inside transaction");
            Ok(())
        })
        .expect("with_tx");

    // Transaction committed — visible outside.
    let results = store
        .get_external_events_in_range(window_start(), window_end())
        .expect("get after tx");
    assert_eq!(results.len(), 1, "event visible after transaction commit");
}

#[test]
fn config_value_via_tx_store() {
    let store = SqliteStore::new_in_memory();

    store
        .with_tx(&mut |tx| {
            tx.set_config_value("tx_cfg_key", "tx_cfg_value")?;
            let v = tx.get_config_value("tx_cfg_key")?;
            assert_eq!(v, Some("tx_cfg_value".to_owned()));
            Ok(())
        })
        .expect("with_tx config value");

    // Committed value is visible outside the transaction.
    let v = store.get_config_value("tx_cfg_key").expect("get after tx");
    assert_eq!(v, Some("tx_cfg_value".to_owned()));
}

#[test]
fn tx_store_remaining_impls() {
    let store = SqliteStore::new_in_memory();

    let event = make_external_event("evt-rem", "cal-rem", 10, 11);
    // Seed an event so clear has something to remove.
    store
        .replace_external_events_in_window("cal-rem", window_start(), window_end(), &[event])
        .expect("seed event");

    let ts = chrono::Utc.with_ymd_and_hms(2026, 3, 13, 8, 0, 0).unwrap();
    store
        .with_tx(&mut |tx| {
            // ExternalEventStore: clear_all_external_events via TxStore.
            tx.clear_all_external_events()?;
            let after_clear = tx.get_external_events_in_range(window_start(), window_end())?;
            assert!(after_clear.is_empty(), "events cleared inside tx");

            // GoogleAuthStore via TxStore.
            let auth = GoogleAuthState {
                calendar_id: Some("tx-cal-id".to_owned()),
                connected_at: Some(ts),
            };
            tx.set_google_auth(&auth)?;
            let got = tx.get_google_auth()?.expect("some after set");
            assert_eq!(got.calendar_id, Some("tx-cal-id".to_owned()));
            tx.clear_google_auth()?;
            assert!(tx.get_google_auth()?.is_none(), "cleared inside tx");

            // ChunkSyncStateStore via TxStore.
            tx.clear_all_chunk_sync_state()?;

            Ok(())
        })
        .expect("with_tx remaining impls");
}

/// Helper: seed a task and chunk for FK integrity, return `(task_id, chunk_id)`.
fn seed_task_and_chunk(store: &SqliteStore, chunk_id: &str) -> (String, String) {
    let task_id = format!("task-{chunk_id}");
    store.with_conn_for_test(|conn| {
        let schedule_id: String = conn
            .query_row("SELECT id FROM schedules WHERE is_default = 1", [], |row| {
                row.get(0)
            })
            .expect("default schedule");
        let ts = "2026-07-12T10:00:00+00:00";
        conn.execute(
            "INSERT INTO tasks \
             (id, title, duration_minutes, time_logged_minutes, priority, status, \
              deadline, schedule_id, min_chunk_minutes, no_split, created_at, updated_at) \
             VALUES (?1, 'T', 60, 0, 2, 'pending', \
                     '2026-12-31T23:59:59+00:00', ?2, 30, 0, ?3, ?3)",
            rusqlite::params![task_id, schedule_id, ts],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO chunks \
             (id, task_id, start_time, end_time, status, is_fixed, created_at, updated_at) \
             VALUES (?1, ?2, \
                     '2026-07-12T10:00:00+00:00', '2026-07-12T11:00:00+00:00', \
                     'scheduled', 0, ?3, ?3)",
            rusqlite::params![chunk_id, task_id, ts],
        )
        .expect("insert chunk");
    });
    (task_id, chunk_id.to_owned())
}

fn make_sync_base(
    chunk_id: &str,
    event_id: &str,
    start: chrono::DateTime<Utc>,
    end: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
) -> ChunkSyncState {
    ChunkSyncState {
        chunk_id: chunk_id.to_owned(),
        event_id: event_id.to_owned(),
        etag: Some("etag-v1".to_owned()),
        synced_start: start,
        synced_end: end,
        synced_title: "Test title".to_owned(),
        synced_description: "Après Work".to_owned(),
        updated_at: now,
    }
}

/// A fresh store with one `ChunkSyncState` row for `chunk_id`/`event_id`
/// spanning `[t, t + 1h)`. Returns `(store, t)` for range-query offsets.
fn store_with_chunk_sync_state(
    chunk_id: &str,
    event_id: &str,
) -> (SqliteStore, chrono::DateTime<Utc>) {
    let store = SqliteStore::new_in_memory();
    let t = chrono::Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap();
    seed_task_and_chunk(&store, chunk_id);
    let base = make_sync_base(chunk_id, event_id, t, t + chrono::Duration::hours(1), t);
    store.upsert_chunk_sync_state(&base).expect("upsert");
    (store, t)
}

#[test_case(chrono::Duration::minutes(30), chrono::Duration::hours(2), true; "overlapping")]
#[test_case(chrono::Duration::hours(2), chrono::Duration::hours(3), false; "nonoverlapping")]
fn get_chunk_sync_states_in_range_query(
    start_offset: chrono::Duration,
    end_offset: chrono::Duration,
    should_overlap: bool,
) {
    let (store, t) = store_with_chunk_sync_state("c-range", "ev-range");
    let results = store
        .get_chunk_sync_states_in_range(t + start_offset, t + end_offset)
        .expect("get in range");

    if should_overlap {
        assert_eq!(results.len(), 1, "must return the overlapping base");
        assert_eq!(results[0].chunk_id, "c-range");
        assert_eq!(results[0].event_id, "ev-range");
    } else {
        assert!(
            results.is_empty(),
            "must exclude non-overlapping base, got: {results:?}"
        );
    }
}

#[test]
fn upsert_chunk_sync_state_inserts_and_updates_etag() {
    let store = SqliteStore::new_in_memory();
    let t = chrono::Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap();

    seed_task_and_chunk(&store, "c-upsert");

    let base_v1 = ChunkSyncState {
        etag: Some("v1".to_owned()),
        ..make_sync_base(
            "c-upsert",
            "ev-upsert",
            t,
            t + chrono::Duration::hours(1),
            t,
        )
    };
    store.upsert_chunk_sync_state(&base_v1).expect("upsert v1");

    let base_v2 = ChunkSyncState {
        etag: Some("v2".to_owned()),
        ..make_sync_base(
            "c-upsert",
            "ev-upsert",
            t,
            t + chrono::Duration::hours(1),
            t,
        )
    };
    store.upsert_chunk_sync_state(&base_v2).expect("upsert v2");

    let results = store
        .get_chunk_sync_states_in_range(
            t - chrono::Duration::minutes(1),
            t + chrono::Duration::hours(2),
        )
        .expect("get");

    assert_eq!(results.len(), 1, "must be a single row after two upserts");
    assert_eq!(
        results[0].etag,
        Some("v2".to_owned()),
        "etag must reflect the second upsert"
    );
}

#[test]
fn delete_chunk_sync_state_removes_row() {
    let store = SqliteStore::new_in_memory();
    let t = chrono::Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap();

    seed_task_and_chunk(&store, "c-delete");
    let base = make_sync_base(
        "c-delete",
        "ev-delete",
        t,
        t + chrono::Duration::hours(1),
        t,
    );
    store.upsert_chunk_sync_state(&base).expect("upsert");

    let before = store
        .get_chunk_sync_states_in_range(
            t - chrono::Duration::minutes(1),
            t + chrono::Duration::hours(2),
        )
        .expect("get before");
    assert_eq!(before.len(), 1, "must exist before delete");

    store.delete_chunk_sync_state("c-delete").expect("delete");

    let after = store
        .get_chunk_sync_states_in_range(
            t - chrono::Duration::minutes(1),
            t + chrono::Duration::hours(2),
        )
        .expect("get after");
    assert!(after.is_empty(), "must be gone after delete");
}

#[test]
fn tx_store_chunk_sync_state_get_and_delete() {
    let store = SqliteStore::new_in_memory();
    let t = chrono::Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap();

    seed_task_and_chunk(&store, "c-tx-ops");
    let base = make_sync_base(
        "c-tx-ops",
        "ev-tx-ops",
        t,
        t + chrono::Duration::hours(1),
        t,
    );
    store.upsert_chunk_sync_state(&base).expect("upsert");

    store
        .with_tx(&mut |tx| {
            let rows = tx.get_chunk_sync_states_in_range(
                t - chrono::Duration::minutes(1),
                t + chrono::Duration::hours(2),
            )?;
            assert_eq!(rows.len(), 1, "TxStore get must find the seeded row");
            assert_eq!(rows[0].chunk_id, "c-tx-ops");

            tx.delete_chunk_sync_state("c-tx-ops")?;

            let after = tx.get_chunk_sync_states_in_range(
                t - chrono::Duration::minutes(1),
                t + chrono::Duration::hours(2),
            )?;
            assert!(after.is_empty(), "row must be deleted via TxStore");
            Ok(())
        })
        .expect("with_tx ok");
}

#[test]
fn tx_store_clear_all_chunk_sync_state() {
    let store = SqliteStore::new_in_memory();
    let t = chrono::Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap();

    seed_task_and_chunk(&store, "c-clear-1");
    seed_task_and_chunk(&store, "c-clear-2");
    let b1 = make_sync_base("c-clear-1", "ev-c1", t, t + chrono::Duration::hours(1), t);
    let b2 = make_sync_base(
        "c-clear-2",
        "ev-c2",
        t + chrono::Duration::hours(2),
        t + chrono::Duration::hours(3),
        t,
    );
    store.upsert_chunk_sync_state(&b1).expect("upsert b1");
    store.upsert_chunk_sync_state(&b2).expect("upsert b2");

    store
        .with_tx(&mut |tx| tx.clear_all_chunk_sync_state())
        .expect("with_tx ok");

    let after = store
        .get_chunk_sync_states_in_range(
            t - chrono::Duration::days(1),
            t + chrono::Duration::days(1),
        )
        .expect("get after");
    assert!(after.is_empty(), "all sync state rows must be cleared");
}
