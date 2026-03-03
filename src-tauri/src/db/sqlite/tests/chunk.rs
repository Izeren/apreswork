// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `ChunkStore` implementation and chunk conversion helpers.

use chrono::{DateTime, Duration, TimeZone, Utc};
use test_case::test_case;

use super::{create_labeled_task, make_test_chunk, make_test_task};
use crate::db::sqlite::chunk::{chunk_status_from_str, chunk_status_to_str};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::ChunkStatus;
use crate::domain::inputs::AgendaItem;
use crate::domain::models::Chunk;
use crate::test_support::fixture_base;
use crate::traits::storage::{ChunkStore, Store, TaskStore};

#[test]
fn create_and_get_chunk_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let chunk = make_test_chunk(&store);

    store.create_chunk(&chunk).expect("create_chunk");
    let loaded = store
        .get_chunk(&chunk.id)
        .expect("get_chunk")
        .expect("chunk should exist");

    assert_eq!(loaded.id, chunk.id);
    assert_eq!(loaded.task_id, chunk.task_id);
    assert_eq!(loaded.start_time, chunk.start_time);
    assert_eq!(loaded.end_time, chunk.end_time);
    assert_eq!(loaded.status, chunk.status);
    assert_eq!(loaded.is_fixed, chunk.is_fixed);
    assert_eq!(loaded.logged_minutes, chunk.logged_minutes);
    assert_eq!(loaded.completed_at, chunk.completed_at);
    assert_eq!(loaded.google_event_id, chunk.google_event_id);
}

#[test]
fn get_chunk_not_found() {
    let store = SqliteStore::new_in_memory();
    let result = store.get_chunk("nonexistent-id").expect("get_chunk");
    assert!(result.is_none());
}

#[test]
fn update_chunk_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let mut chunk = make_test_chunk(&store);
    store.create_chunk(&chunk).expect("create_chunk");

    chunk.status = ChunkStatus::Completed;
    chunk.is_fixed = true;
    chunk.logged_minutes = Some(45);
    chunk.completed_at = Some(Utc.with_ymd_and_hms(2026, 3, 13, 19, 5, 0).unwrap());
    chunk.google_event_id = Some("gcal-event-123".to_owned());
    chunk.start_time = Utc.with_ymd_and_hms(2026, 3, 13, 17, 0, 0).unwrap();
    chunk.end_time = Utc.with_ymd_and_hms(2026, 3, 13, 18, 0, 0).unwrap();
    store.update_chunk(&chunk).expect("update_chunk");

    let loaded = store
        .get_chunk(&chunk.id)
        .expect("get_chunk")
        .expect("chunk should exist");
    assert_eq!(loaded.status, ChunkStatus::Completed);
    assert!(loaded.is_fixed);
    assert_eq!(loaded.logged_minutes, Some(45));
    assert_eq!(loaded.completed_at, chunk.completed_at);
    assert_eq!(loaded.google_event_id, Some("gcal-event-123".to_owned()));
    assert_eq!(loaded.start_time, chunk.start_time);
    assert_eq!(loaded.end_time, chunk.end_time);
}

#[test]
fn delete_chunk_removes_chunk() {
    let store = SqliteStore::new_in_memory();
    let chunk = make_test_chunk(&store);
    store.create_chunk(&chunk).expect("create_chunk");

    store.delete_chunk(&chunk.id).expect("delete_chunk");

    let loaded = store.get_chunk(&chunk.id).expect("get_chunk");
    assert!(loaded.is_none());
}

#[test]
fn get_chunks_for_task_filters_by_task() {
    let store = SqliteStore::new_in_memory();

    let chunk_a1 = make_test_chunk(&store);
    store.create_chunk(&chunk_a1).expect("create chunk_a1");

    let chunk_a2 = Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        start_time: Utc.with_ymd_and_hms(2026, 3, 13, 20, 0, 0).unwrap(),
        end_time: Utc.with_ymd_and_hms(2026, 3, 13, 21, 0, 0).unwrap(),
        ..chunk_a1
    };
    store.create_chunk(&chunk_a2).expect("create chunk_a2");

    let other_task_chunk = make_test_chunk(&store);
    store
        .create_chunk(&other_task_chunk)
        .expect("create other_task_chunk");

    let chunks_a = store
        .get_chunks_for_task(&chunk_a2.task_id)
        .expect("get_chunks_for_task A");
    assert_eq!(chunks_a.len(), 2);

    let chunks_other = store
        .get_chunks_for_task(&other_task_chunk.task_id)
        .expect("get_chunks_for_task other");
    assert_eq!(chunks_other.len(), 1);
}

#[test]
fn get_chunks_for_task_empty() {
    let store = SqliteStore::new_in_memory();
    let task = make_test_task(&store);
    store.create_task(&task).expect("create_task");

    let chunks = store
        .get_chunks_for_task(&task.id)
        .expect("get_chunks_for_task");
    assert!(chunks.is_empty());
}

fn chunk_start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 13, 18, 0, 0).unwrap()
}

// Offsets are minutes from chunk_start().
#[test_case(-30, 30, 1 ; "overlap from left")]
#[test_case(-180, -120, 0 ; "no overlap before")]
#[test_case(-60, 0, 0 ; "adjacent before exclusive")]
#[test_case(60, 120, 0 ; "adjacent after exclusive")]
#[test_case(30, 90, 1 ; "overlap from right")]
fn get_chunks_in_range_cases(start_offset: i64, end_offset: i64, expected_count: usize) {
    let store = SqliteStore::new_in_memory();
    let chunk = make_test_chunk(&store);
    store.create_chunk(&chunk).expect("create_chunk");
    let base = chunk_start();
    let result = store
        .get_chunks_in_range(
            base + Duration::minutes(start_offset),
            base + Duration::minutes(end_offset),
        )
        .expect("get_chunks_in_range");
    assert_eq!(result.len(), expected_count);
}

/// Seed the four `is_fixed` × `completed` chunk variants shared by the
/// auto-chunks and fixed-or-completed query tests: plain auto, fixed-only,
/// completed-only, and fixed+completed. Returns them in seed order.
fn seed_chunk_variants(store: &SqliteStore) -> (Chunk, Chunk, Chunk, Chunk) {
    let auto_chunk = make_test_chunk(store);
    store.create_chunk(&auto_chunk).expect("create auto");

    let mut fixed_chunk = make_test_chunk(store);
    fixed_chunk.is_fixed = true;
    store.create_chunk(&fixed_chunk).expect("create fixed");

    let mut completed_chunk = make_test_chunk(store);
    completed_chunk.status = ChunkStatus::Completed;
    completed_chunk.completed_at = Some(fixture_base());
    store
        .create_chunk(&completed_chunk)
        .expect("create completed");

    let mut fixed_completed = make_test_chunk(store);
    fixed_completed.is_fixed = true;
    fixed_completed.status = ChunkStatus::Completed;
    fixed_completed.completed_at = Some(fixture_base());
    store
        .create_chunk(&fixed_completed)
        .expect("create fixed+completed");

    (auto_chunk, fixed_chunk, completed_chunk, fixed_completed)
}

#[test]
fn get_auto_chunks_returns_non_fixed_non_completed() {
    let store = SqliteStore::new_in_memory();
    let (auto_chunk, ..) = seed_chunk_variants(&store);

    let auto = store.get_auto_chunks().expect("get_auto_chunks");
    assert_eq!(auto.len(), 1);
    assert_eq!(auto[0].id, auto_chunk.id);
}

#[test]
fn get_all_fixed_and_completed_returns_correct_set() {
    let store = SqliteStore::new_in_memory();
    let (auto_chunk, fixed_chunk, completed_chunk, fixed_completed) = seed_chunk_variants(&store);

    let result = store
        .get_all_fixed_and_completed()
        .expect("get_all_fixed_and_completed");
    assert_eq!(result.len(), 3);

    let ids: Vec<&str> = result.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&fixed_chunk.id.as_str()));
    assert!(ids.contains(&completed_chunk.id.as_str()));
    assert!(ids.contains(&fixed_completed.id.as_str()));
    assert!(!ids.contains(&auto_chunk.id.as_str()));
}

#[test_case(ChunkStatus::Scheduled, "scheduled" ; "scheduled roundtrips")]
#[test_case(ChunkStatus::Completed, "completed" ; "completed roundtrips")]
fn chunk_status_helpers_roundtrip(variant: ChunkStatus, expected_str: &str) {
    assert_eq!(chunk_status_to_str(variant), expected_str);
    assert_eq!(chunk_status_from_str(expected_str).unwrap(), variant);
}

#[test]
fn chunk_status_from_str_rejects_unknown() {
    assert!(chunk_status_from_str("bogus").is_err());
}

/// Create a chunk in the `fixture_base` window for the given task.
fn make_chunk_for_task(task_id: &str) -> Chunk {
    let base = fixture_base();
    Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task_id.to_owned(),
        start_time: base,
        end_time: base + chrono::Duration::hours(1),
        ..Chunk::test_default()
    }
}

fn agenda_for_fixture_day(store: &SqliteStore) -> Vec<AgendaItem> {
    let base = fixture_base();
    store
        .get_agenda_in_range(base, base + chrono::Duration::hours(2))
        .expect("get_agenda_in_range")
}

#[test]
fn get_agenda_in_range_labels_no_cross_contamination() {
    let store = SqliteStore::new_in_memory();

    let task_a = create_labeled_task(&store, &["foo"]);
    let task_b = create_labeled_task(&store, &["bar"]);

    store
        .create_chunk(&make_chunk_for_task(&task_a.id))
        .expect("create chunk_a");
    store
        .create_chunk(&make_chunk_for_task(&task_b.id))
        .expect("create chunk_b");

    let items = agenda_for_fixture_day(&store);
    assert_eq!(items.len(), 2);

    let item_a = items
        .iter()
        .find(|i| i.chunk.task_id == task_a.id)
        .expect("item for task_a");
    let item_b = items
        .iter()
        .find(|i| i.chunk.task_id == task_b.id)
        .expect("item for task_b");

    assert_eq!(item_a.task_labels, vec!["foo"]);
    assert_eq!(item_b.task_labels, vec!["bar"]);
}

#[test]
fn get_agenda_in_range_no_labels_returns_empty_vec() {
    let store = SqliteStore::new_in_memory();

    let task = make_test_task(&store);
    store.create_task(&task).expect("create task");
    store
        .create_chunk(&make_chunk_for_task(&task.id))
        .expect("create chunk");

    let items = agenda_for_fixture_day(&store);
    assert_eq!(items.len(), 1);
    assert!(items[0].task_labels.is_empty());
}

#[test]
fn get_agenda_in_range_two_chunks_same_task_both_get_labels() {
    let store = SqliteStore::new_in_memory();

    let task = create_labeled_task(&store, &["shared"]);

    let base = fixture_base();
    let chunk1 = Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task.id.clone(),
        start_time: base,
        end_time: base + chrono::Duration::minutes(30),
        ..Chunk::test_default()
    };
    let chunk2 = Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task.id.clone(),
        start_time: base + chrono::Duration::minutes(30),
        end_time: base + chrono::Duration::hours(1),
        ..Chunk::test_default()
    };
    store.create_chunk(&chunk1).expect("create chunk1");
    store.create_chunk(&chunk2).expect("create chunk2");

    let items = agenda_for_fixture_day(&store);
    assert_eq!(items.len(), 2);
    for item in &items {
        assert_eq!(
            item.task_labels,
            vec!["shared"],
            "both agenda items must carry the task label"
        );
    }
}

fn no_optionals(_chunk: &mut Chunk) {}

fn all_optionals(chunk: &mut Chunk) {
    chunk.logged_minutes = Some(55);
    chunk.completed_at = Some(completed_at_fixture());
    chunk.google_event_id = Some("gcal-abc-123".to_owned());
}

fn completed_at_fixture() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 13, 19, 5, 0).unwrap()
}

#[test_case(no_optionals, None, None, None ; "all optional None")]
#[test_case(all_optionals, Some(55), Some(completed_at_fixture()), Some("gcal-abc-123") ; "all optional Some")]
fn create_chunk_optional_fields_roundtrip(
    setup: fn(&mut Chunk),
    expected_logged: Option<i64>,
    expected_completed_at: Option<DateTime<Utc>>,
    expected_gcal: Option<&str>,
) {
    let store = SqliteStore::new_in_memory();
    let mut chunk = make_test_chunk(&store);
    setup(&mut chunk);
    store.create_chunk(&chunk).expect("create_chunk");
    let loaded = store
        .get_chunk(&chunk.id)
        .expect("get_chunk")
        .expect("chunk should exist");
    assert_eq!(loaded.logged_minutes, expected_logged);
    assert_eq!(loaded.completed_at, expected_completed_at);
    assert_eq!(loaded.google_event_id, expected_gcal.map(str::to_owned));
}

#[test]
fn get_fixed_scheduled_chunks_via_tx_store_visible_in_transaction() {
    let store = SqliteStore::new_in_memory();
    let (_, fixed_chunk, ..) = seed_chunk_variants(&store);

    store
        .with_tx(&mut |tx| {
            let result = tx.get_fixed_scheduled_chunks()?;
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].id, fixed_chunk.id);
            Ok(())
        })
        .expect("with_tx");
}

#[test]
fn get_fixed_scheduled_chunks_returns_only_fixed_scheduled() {
    let store = SqliteStore::new_in_memory();
    let (auto_chunk, fixed_chunk, completed_chunk, fixed_completed) = seed_chunk_variants(&store);

    let result = store
        .get_fixed_scheduled_chunks()
        .expect("get_fixed_scheduled_chunks");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, fixed_chunk.id);

    let ids: Vec<&str> = result.iter().map(|c| c.id.as_str()).collect();
    assert!(
        !ids.contains(&auto_chunk.id.as_str()),
        "auto should not be returned"
    );
    assert!(
        !ids.contains(&completed_chunk.id.as_str()),
        "completed should not be returned"
    );
    assert!(
        !ids.contains(&fixed_completed.id.as_str()),
        "fixed+completed should not be returned"
    );
}

#[test]
fn delete_task_cascades_to_chunks() {
    let store = SqliteStore::new_in_memory();
    let chunk = make_test_chunk(&store);
    store.create_chunk(&chunk).expect("create_chunk");

    assert!(store.get_chunk(&chunk.id).expect("get").is_some());

    store.delete_task(&chunk.task_id).expect("delete_task");

    let loaded = store.get_chunk(&chunk.id).expect("get_chunk after delete");
    assert!(
        loaded.is_none(),
        "chunk should be cascade-deleted with task"
    );
}
