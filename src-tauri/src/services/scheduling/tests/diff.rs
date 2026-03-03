// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `diff_chunks` pairing algorithm.

use chrono::{DateTime, Duration, TimeZone, Utc};

use super::make_chunk_at;
use crate::domain::models::Chunk;
use crate::services::scheduling::{diff_chunks, DiffOp};

fn base_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 18, 0, 0).unwrap()
}

fn delete_ids(ops: &[DiffOp]) -> Vec<&str> {
    ops.iter()
        .filter_map(|op| match op {
            DiffOp::Delete { chunk_id } => Some(chunk_id.as_str()),
            _ => None,
        })
        .collect()
}

fn keep_ids(ops: &[DiffOp]) -> Vec<&str> {
    ops.iter()
        .filter_map(|op| match op {
            DiffOp::Keep { chunk_id } => Some(chunk_id.as_str()),
            _ => None,
        })
        .collect()
}

fn update_ids(ops: &[DiffOp]) -> Vec<&str> {
    ops.iter()
        .filter_map(|op| match op {
            DiffOp::Update { chunk_id, .. } => Some(chunk_id.as_str()),
            _ => None,
        })
        .collect()
}

fn create_ids(ops: &[DiffOp]) -> Vec<&str> {
    ops.iter()
        .filter_map(|op| match op {
            DiffOp::Create { chunk } => Some(chunk.id.as_str()),
            _ => None,
        })
        .collect()
}

/// Diff `old` against `new`, assert exactly one op that is an `Update`, and
/// return its `(chunk_id, new_start, new_end, google_event_id)`.
fn assert_single_update(
    old: &[Chunk],
    new: &[Chunk],
) -> (String, DateTime<Utc>, DateTime<Utc>, Option<String>) {
    let ops = diff_chunks(old, new);
    assert_eq!(ops.len(), 1, "expected exactly one op, got: {ops:?}");
    match ops.into_iter().next().expect("one op") {
        DiffOp::Update {
            chunk_id,
            new_start,
            new_end,
            google_event_id,
        } => (chunk_id, new_start, new_end, google_event_id),
        other => panic!("expected UPDATE, got: {other:?}"),
    }
}

#[test]
fn diff_chunks_empty_both() {
    let ops = diff_chunks(&[], &[]);
    assert!(ops.is_empty(), "expected no ops, got: {ops:?}");
}

#[test]
fn diff_chunks_empty_old() {
    let t = base_time();
    let new_chunks = vec![
        make_chunk_at("n1", "task-1", t, t + Duration::hours(1), None),
        make_chunk_at(
            "n2",
            "task-1",
            t + Duration::hours(2),
            t + Duration::hours(3),
            None,
        ),
    ];

    let ops = diff_chunks(&[], &new_chunks);

    assert_eq!(ops.len(), 2, "expected 2 ops");
    assert!(
        ops.iter().all(|op| matches!(op, DiffOp::Create { .. })),
        "expected all CREATE, got: {ops:?}"
    );
}

#[test]
fn diff_chunks_empty_new() {
    let t = base_time();
    let old_chunks = vec![
        make_chunk_at("o1", "task-1", t, t + Duration::hours(1), None),
        make_chunk_at(
            "o2",
            "task-1",
            t + Duration::hours(2),
            t + Duration::hours(3),
            None,
        ),
    ];

    let ops = diff_chunks(&old_chunks, &[]);

    assert_eq!(ops.len(), 2, "expected 2 ops");
    assert!(
        ops.iter().all(|op| matches!(op, DiffOp::Delete { .. })),
        "expected all DELETE, got: {ops:?}"
    );
    let deleted_ids = delete_ids(&ops);
    assert!(deleted_ids.contains(&"o1"));
    assert!(deleted_ids.contains(&"o2"));
}

#[test]
fn diff_chunks_all_keep_idempotent() {
    let t = base_time();
    let chunks = vec![
        make_chunk_at("c1", "task-1", t, t + Duration::hours(1), None),
        make_chunk_at(
            "c2",
            "task-1",
            t + Duration::hours(2),
            t + Duration::hours(3),
            None,
        ),
    ];

    let ops = diff_chunks(&chunks, &chunks);

    assert_eq!(ops.len(), 2, "expected 2 ops");
    assert!(
        ops.iter().all(|op| matches!(op, DiffOp::Keep { .. })),
        "expected all KEEP, got: {ops:?}"
    );
    let kept_ids = keep_ids(&ops);
    assert!(kept_ids.contains(&"c1"));
    assert!(kept_ids.contains(&"c2"));
}

/// Chunk shifted by 30 min → UPDATE op with correct new times.
#[test]
fn diff_chunks_shift_produces_update() {
    let t = base_time();
    let old = vec![make_chunk_at(
        "o1",
        "task-1",
        t,
        t + Duration::hours(1),
        None,
    )];
    let new_start = t + Duration::minutes(30);
    let new_end = t + Duration::hours(1) + Duration::minutes(30);
    let new = vec![make_chunk_at("n1", "task-1", new_start, new_end, None)];

    let (chunk_id, got_start, got_end, google_event_id) = assert_single_update(&old, &new);
    assert_eq!(chunk_id, "o1");
    assert_eq!(got_start, new_start);
    assert_eq!(got_end, new_end);
    assert!(google_event_id.is_none());
}

#[test]
fn diff_chunks_remove_produces_delete() {
    let t = base_time();
    let old = vec![make_chunk_at(
        "o1",
        "task-1",
        t,
        t + Duration::hours(1),
        None,
    )];

    let ops = diff_chunks(&old, &[]);

    assert_eq!(ops.len(), 1);
    assert!(
        matches!(&ops[0], DiffOp::Delete { chunk_id } if chunk_id == "o1"),
        "expected DELETE for o1, got: {:?}",
        ops[0]
    );
}

#[test]
fn diff_chunks_add_produces_create() {
    let t = base_time();
    let new_chunk = make_chunk_at("n1", "task-1", t, t + Duration::hours(1), None);

    let ops = diff_chunks(&[], std::slice::from_ref(&new_chunk));

    assert_eq!(ops.len(), 1);
    match &ops[0] {
        DiffOp::Create { chunk } => {
            assert_eq!(chunk.id, "n1");
            assert_eq!(chunk.start_time, t);
        }
        other => panic!("expected CREATE, got: {other:?}"),
    }
}

/// Old chunk with `google_event_id`, times change → UPDATE preserves `google_event_id`.
#[test]
fn diff_chunks_google_event_id_preserved_on_update() {
    let t = base_time();
    let old = vec![make_chunk_at(
        "o1",
        "task-1",
        t,
        t + Duration::hours(1),
        Some("gcal-evt-123"),
    )];
    let new_start = t + Duration::minutes(15);
    let new_end = t + Duration::hours(1) + Duration::minutes(15);
    let new = vec![make_chunk_at("n1", "task-1", new_start, new_end, None)];

    let (chunk_id, _, _, google_event_id) = assert_single_update(&old, &new);
    assert_eq!(chunk_id, "o1");
    assert_eq!(google_event_id.as_deref(), Some("gcal-evt-123"));
}

#[test]
fn diff_chunks_google_event_id_preserved_on_keep() {
    let t = base_time();
    let old = vec![make_chunk_at(
        "o1",
        "task-1",
        t,
        t + Duration::hours(1),
        Some("gcal-evt-456"),
    )];
    // New chunk has same times (different ID is irrelevant — we match by times).
    let new = vec![make_chunk_at(
        "n1",
        "task-1",
        t,
        t + Duration::hours(1),
        None,
    )];

    let ops = diff_chunks(&old, &new);

    assert_eq!(ops.len(), 1);
    match &ops[0] {
        DiffOp::Keep { chunk_id } => {
            assert_eq!(chunk_id, "o1");
        }
        other => panic!("expected KEEP, got: {other:?}"),
    }
}

#[test]
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
fn diff_chunks_mixed_scenario() {
    let t = base_time();

    let a_old = make_chunk_at("a-old", "task-a", t, t + Duration::hours(1), None);
    let a_new = make_chunk_at("a-new", "task-a", t, t + Duration::hours(1), None);

    let b_old = make_chunk_at("b-old", "task-b", t, t + Duration::hours(1), None);
    let b_new = make_chunk_at(
        "b-new",
        "task-b",
        t + Duration::hours(2),
        t + Duration::hours(3),
        None,
    );

    let c_old = make_chunk_at(
        "c-old",
        "task-c",
        t + Duration::hours(4),
        t + Duration::hours(5),
        None,
    );
    let c_new = make_chunk_at(
        "c-new",
        "task-c",
        t + Duration::hours(6),
        t + Duration::hours(7),
        None,
    );

    let old_chunks = vec![a_old, b_old, c_old];
    let new_chunks = vec![a_new, b_new, c_new];

    let ops = diff_chunks(&old_chunks, &new_chunks);

    let keeps = keep_ids(&ops);
    let updates = update_ids(&ops);
    let deletes = delete_ids(&ops);
    let creates = create_ids(&ops);

    assert!(keeps.contains(&"a-old"), "expected KEEP for a-old");

    // task-b old chunk → UPDATE (times differ significantly — no old near b_new).
    // task-b: b_old is at t, b_new is at t+2h. Since there's only one old chunk
    // for task-b, it pairs with b_new regardless → UPDATE.
    assert!(
        updates.contains(&"b-old"),
        "expected UPDATE for b-old; updates={updates:?}"
    );

    // task-c: c_old is at t+4h, c_new is at t+6h.
    // They are 2h apart. Since task-c has exactly one old and one new chunk,
    // they pair together → UPDATE for c-old (not DELETE/CREATE).
    // Verify either UPDATE or DELETE+CREATE based on algorithm pairing.
    let c_updated = updates.contains(&"c-old");
    let c_deleted = deletes.contains(&"c-old");
    let c_created = creates.contains(&"c-new");
    assert!(
        c_updated || (c_deleted && c_created),
        "task-c chunks must be either UPDATE or DELETE+CREATE"
    );

    assert!(!deletes.contains(&"a-old"), "a-old should not be deleted");
    assert!(!creates.contains(&"a-new"), "a-new should not be created");
}

/// Greedy pairing: two old chunks, two new chunks — closest match wins.
///
/// old: [t+0h, t+4h]  new: [t+30min, t+3h]
/// Correct pairing: t+0h↔t+30min (dist=30min), t+4h↔t+3h (dist=1h)
/// Wrong pairing would be: t+0h↔t+3h, t+4h↔t+30min
#[test]
fn diff_chunks_greedy_pairing_closest_match() {
    let t = base_time();

    let old = vec![
        make_chunk_at("o-early", "task-1", t, t + Duration::hours(1), None),
        make_chunk_at(
            "o-late",
            "task-1",
            t + Duration::hours(4),
            t + Duration::hours(5),
            None,
        ),
    ];
    let new = vec![
        make_chunk_at(
            "n-early",
            "task-1",
            t + Duration::minutes(30),
            t + Duration::hours(1) + Duration::minutes(30),
            None,
        ),
        make_chunk_at(
            "n-late",
            "task-1",
            t + Duration::hours(3),
            t + Duration::hours(4),
            None,
        ),
    ];

    let ops = diff_chunks(&old, &new);

    // Should be 2 UPDATE ops (both pairs differ in times).
    assert_eq!(ops.len(), 2, "expected 2 ops, got: {ops:?}");

    let updated: Vec<_> = ops
        .iter()
        .filter_map(|op| {
            if let DiffOp::Update {
                chunk_id,
                new_start,
                ..
            } = op
            {
                Some((chunk_id.as_str(), *new_start))
            } else {
                None
            }
        })
        .collect();

    // o-early paired with n-early (closest, 30 min apart).
    let o_early_update = updated.iter().find(|(id, _)| *id == "o-early");
    assert!(o_early_update.is_some(), "o-early should have an UPDATE op");
    assert_eq!(
        o_early_update.unwrap().1,
        t + Duration::minutes(30),
        "o-early should be updated to t+30min"
    );

    // o-late paired with n-late (closest, 1h apart).
    let o_late_update = updated.iter().find(|(id, _)| *id == "o-late");
    assert!(o_late_update.is_some(), "o-late should have an UPDATE op");
    assert_eq!(
        o_late_update.unwrap().1,
        t + Duration::hours(3),
        "o-late should be updated to t+3h"
    );
}
