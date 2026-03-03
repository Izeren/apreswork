// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for `sync_cycle` (G5 push sync).

use std::sync::Arc;

use chrono::{DateTime, Duration, SubsecRound, Utc};

use crate::db::sqlite::SqliteStore;
use crate::domain::enums::{ChunkStatus, TaskStatus};
use crate::domain::models::GoogleAuthState;
use crate::domain::models::{Chunk, ChunkSyncState, Task};
use crate::error::AppError;
use crate::scheduler::engine::DefaultScheduler;
use crate::services::sync::{sync_cycle, PushCounts};
use crate::test_support::{calendar::MockCalendarSync, test_now};
use crate::traits::calendar_sync::{RemoteChunkEvent, SyncOp};
use crate::traits::storage::{ChunkStore, ChunkSyncStateStore, GoogleAuthStore, Store, TaskStore};

use super::test_util::make_trigger;

fn insert_task_with_chunk(
    store: &dyn Store,
    start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> (String, String) {
    let task_id = uuid::Uuid::now_v7().to_string();

    let schedule_id = store
        .get_default_schedule()
        .expect("get default schedule")
        .id;

    let task = Task {
        id: task_id.clone(),
        title: "Test task".to_owned(),
        description: None,
        duration_minutes: 60,
        time_logged_minutes: 0,
        priority: crate::domain::enums::Priority::Medium,
        status: TaskStatus::Scheduled,
        start_date: None,
        deadline: Some(now + Duration::days(30)),
        schedule_id,
        min_chunk_minutes: 30,
        no_split: false,
        recurring_template_id: None,
        expire_at: None,
        is_pinned: false,
        labels: vec![],
        created_at: now,
        updated_at: now,
    };
    store.create_task(&task).expect("create task");

    let chunk_id = insert_extra_chunk(store, &task_id, start, now);
    (task_id, chunk_id)
}

fn insert_sync_base(
    store: &dyn Store,
    chunk_id: &str,
    event_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
) {
    store
        .upsert_chunk_sync_state(&ChunkSyncState {
            chunk_id: chunk_id.to_owned(),
            event_id: event_id.to_owned(),
            etag: Some("etag-v1".to_owned()),
            synced_start: start,
            synced_end: end,
            synced_title: "Test task".to_owned(),
            synced_description: "Après Work".to_owned(),
            updated_at: now,
        })
        .expect("upsert sync base");
}

fn make_remote_event(event_id: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> RemoteChunkEvent {
    RemoteChunkEvent {
        event_id: event_id.to_owned(),
        etag: Some("etag-v1".to_owned()),
        start,
        end,
        title: "Test task".to_owned(),
        description: Some("Après Work".to_owned()),
    }
}

fn connected_sync(store: &dyn Store, events: Vec<RemoteChunkEvent>) -> MockCalendarSync {
    store
        .set_google_auth(&GoogleAuthState {
            calendar_id: Some("cal-app".to_owned()),
            connected_at: None,
        })
        .expect("set google auth");
    MockCalendarSync::new(true, std::collections::HashMap::new())
        .with_app_calendar("cal-app", events)
}

fn run_cycle(
    store: &Arc<SqliteStore>,
    sync: &MockCalendarSync,
    now: DateTime<Utc>,
) -> Result<PushCounts, AppError> {
    let trigger = make_trigger(store);
    sync_cycle(store.as_ref(), sync, &DefaultScheduler, &trigger, now)
}

fn seed_synced_chunk(
    store: &dyn Store,
    event_id: &str,
    now: DateTime<Utc>,
) -> (String, String, DateTime<Utc>, DateTime<Utc>) {
    let start = now + Duration::hours(1);
    let end = start + Duration::hours(1);
    let (task_id, chunk_id) = insert_task_with_chunk(store, start, now);
    insert_sync_base(store, &chunk_id, event_id, start, end, now);
    (task_id, chunk_id, start, end)
}

fn bases_in_next_month(store: &dyn Store, now: DateTime<Utc>) -> Vec<ChunkSyncState> {
    store
        .get_chunk_sync_states_in_range(now, now + Duration::days(31))
        .expect("get bases")
}

fn scheduled_chunk(id: &str, task_id: &str, start: DateTime<Utc>, now: DateTime<Utc>) -> Chunk {
    Chunk {
        id: id.to_owned(),
        task_id: task_id.to_owned(),
        start_time: start,
        end_time: start + Duration::hours(1),
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn store_with_synced_chunk(event_id: &str) -> (Arc<SqliteStore>, DateTime<Utc>, String) {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let (_task_id, chunk_id, _, _) = seed_synced_chunk(store.as_ref(), event_id, now);
    (store, now, chunk_id)
}

struct TwoChunkTask {
    store: Arc<SqliteStore>,
    now: DateTime<Utc>,
    chunk1: String,
    chunk2: String,
    start1: DateTime<Utc>,
    start2: DateTime<Utc>,
}

fn two_chunk_task() -> TwoChunkTask {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let start1 = now + Duration::hours(1);
    let start2 = now + Duration::hours(3);
    let (task_id, chunk1) = insert_task_with_chunk(store.as_ref(), start1, now);
    let chunk2 = insert_extra_chunk(store.as_ref(), &task_id, start2, now);
    TwoChunkTask {
        store,
        now,
        chunk1,
        chunk2,
        start1,
        start2,
    }
}

fn push_events(
    store: &Arc<SqliteStore>,
    now: DateTime<Utc>,
    events: Vec<RemoteChunkEvent>,
) -> (PushCounts, Vec<SyncOp>) {
    let sync = connected_sync(store.as_ref(), events);
    let counts = run_cycle(store, &sync, now).expect("sync_cycle ok");
    let ops = sync.get_recorded_sync_ops();
    (counts, ops)
}

fn assert_noop(ops: &[SyncOp], counts: PushCounts) {
    assert!(ops.is_empty(), "expected no push ops; got {ops:?}");
    assert_eq!(counts, PushCounts::default(), "no-op sync must count zero");
}

fn assert_single_base_times(
    store: &dyn Store,
    now: DateTime<Utc>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) {
    let bases = bases_in_next_month(store, now);
    assert_eq!(bases.len(), 1);
    assert_eq!(bases[0].synced_start, start);
    assert_eq!(bases[0].synced_end, end);
}

fn run_cycle_one_remote(
    store: &Arc<SqliteStore>,
    now: DateTime<Utc>,
    remote: RemoteChunkEvent,
) -> Vec<SyncOp> {
    let sync = connected_sync(store.as_ref(), vec![remote]);
    run_cycle(store, &sync, now).expect("sync_cycle ok");
    sync.get_recorded_sync_ops()
}

fn store_with_no_calendar_id() -> (Arc<SqliteStore>, DateTime<Utc>) {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    store
        .set_google_auth(&GoogleAuthState {
            calendar_id: None,
            connected_at: None,
        })
        .expect("set google auth");
    (store, now)
}

#[test]
fn no_op_when_provider_unavailable() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let sync = MockCalendarSync::default(); // available=false
    let scheduler = DefaultScheduler;
    let trigger = make_trigger(&store);
    let now = test_now();

    sync_cycle(store.as_ref(), &sync, &scheduler, &trigger, now).expect("sync_cycle ok");

    assert!(
        sync.get_recorded_sync_ops().is_empty(),
        "no ops must be recorded when provider is unavailable"
    );
}

#[test]
fn creates_event_for_new_chunk() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let chunk_start = now + Duration::hours(1);

    let (_task_id, chunk_id) = insert_task_with_chunk(store.as_ref(), chunk_start, now);

    // No sync base, no remote events → should Create.
    let sync = connected_sync(store.as_ref(), vec![]);
    let counts = run_cycle(&store, &sync, now).expect("sync_cycle ok");

    let ops = sync.get_recorded_sync_ops();
    assert_eq!(ops.len(), 1, "must issue exactly one Create op");
    assert!(
        matches!(&ops[0], crate::traits::calendar_sync::SyncOp::Create(p) if p.chunk_id == chunk_id),
        "op must be Create for the new chunk"
    );
    assert_eq!(
        counts,
        PushCounts {
            created: 1,
            updated: 0,
            deleted: 0
        },
        "returned counts must reflect the single Create"
    );

    // Base row must be persisted.
    let bases = bases_in_next_month(store.as_ref(), now);
    assert_eq!(bases.len(), 1, "one sync base must exist");
    assert_eq!(bases[0].chunk_id, chunk_id);
}

#[test]
fn updates_locally_changed_chunk() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let (_task_id, chunk_id, original_start, original_end) =
        seed_synced_chunk(store.as_ref(), "ev-1", now);

    let new_start = original_start + Duration::minutes(30);
    let new_end = new_start + Duration::hours(1);
    let mut chunk = store.get_chunk(&chunk_id).expect("get chunk").unwrap();
    chunk.start_time = new_start;
    chunk.end_time = new_end;
    chunk.updated_at = now + Duration::seconds(1);
    store.update_chunk(&chunk).expect("update chunk");

    // Remote has original time (= base) → local changed → Update.
    let remote = make_remote_event("ev-1", original_start, original_end);
    let (counts, ops) = push_events(&store, now, vec![remote]);
    assert_eq!(ops.len(), 1, "must issue exactly one Update op");
    assert!(
        matches!(&ops[0], crate::traits::calendar_sync::SyncOp::Update { event_id, .. } if event_id == "ev-1"),
        "op must be Update on ev-1"
    );
    assert_eq!(
        counts,
        PushCounts {
            created: 0,
            updated: 1,
            deleted: 0
        },
        "returned counts must reflect the single Update"
    );

    // Base must reflect new times.
    assert_single_base_times(store.as_ref(), now, new_start, new_end);
}

#[test]
fn skips_unchanged_chunk() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();

    // Base exactly matches local chunk and remote event → nothing changed.
    let (_task_id, _chunk_id, start, end) = seed_synced_chunk(store.as_ref(), "ev-same", now);
    let remote = make_remote_event("ev-same", start, end);
    let (counts, ops) = push_events(&store, now, vec![remote]);
    assert_noop(&ops, counts);
}

/// Google stores whole seconds: a base recorded with sub-second precision vs
/// the provider's truncated echo must classify as "unchanged" (Case C), not
/// as a remote move (Case E) that would pin the chunk.
#[test]
fn sub_second_remote_drift_is_not_a_remote_change() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    // Chunk + base carry sub-second precision (as pre-fix syncs recorded).
    let start = (now + Duration::hours(1)).trunc_subsecs(0) + Duration::nanoseconds(606_881_160);
    let end = start + Duration::hours(1);

    let (_task_id, chunk_id) = insert_task_with_chunk(store.as_ref(), start, now);
    insert_sync_base(store.as_ref(), &chunk_id, "ev-drift", start, end, now);

    // Remote echoes the same instants truncated to whole seconds.
    let remote = make_remote_event("ev-drift", start.trunc_subsecs(0), end.trunc_subsecs(0));
    let (counts, ops) = push_events(&store, now, vec![remote]);
    assert_noop(&ops, counts);

    let chunk = store.get_chunk(&chunk_id).expect("get chunk").unwrap();
    assert!(
        !chunk.is_fixed,
        "sub-second drift must not be accepted as a remote move (would pin the chunk)"
    );
}

/// A local chunk differing from its base only below one second is not a
/// local change — no Update op may be pushed.
#[test]
fn sub_second_local_drift_is_not_a_local_change() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let base_start = (now + Duration::hours(1)).trunc_subsecs(0);
    let base_end = base_start + Duration::hours(1);

    // Chunk sits 606ms after the whole-second base.
    let chunk_start = base_start + Duration::milliseconds(606);
    let (_task_id, chunk_id) = insert_task_with_chunk(store.as_ref(), chunk_start, now);
    insert_sync_base(
        store.as_ref(),
        &chunk_id,
        "ev-local",
        base_start,
        base_end,
        now,
    );

    // Remote matches the base exactly.
    let remote = make_remote_event("ev-local", base_start, base_end);
    let (counts, ops) = push_events(&store, now, vec![remote]);
    assert_noop(&ops, counts);
}

#[test]
fn remote_delete_removes_local_chunk_and_reschedules() {
    let (store, now, chunk_id) = store_with_synced_chunk("ev-gone");

    // Remote returns no events (deleted ev-gone).
    let sync = connected_sync(store.as_ref(), vec![]);
    run_cycle(&store, &sync, now).expect("sync_cycle ok");

    // Chunk must be deleted.
    let chunk_after = store.get_chunk(&chunk_id).expect("get chunk");
    assert!(
        chunk_after.is_none(),
        "chunk must be deleted when remote removed the event"
    );

    // Sync base must be gone (cascaded via ON DELETE CASCADE).
    let bases = store
        .get_chunk_sync_states_in_range(now - Duration::days(1), now + Duration::days(31))
        .expect("get bases");
    assert!(bases.is_empty(), "sync base must cascade-delete with chunk");

    // No SyncOp should have been issued for the deletion.
    let ops = sync.get_recorded_sync_ops();
    assert!(
        ops.is_empty(),
        "no SyncOps for a remote-deleted chunk; got {ops:?}"
    );
}

/// Scheduler double that always fails — pins the error path of the
/// post-merge incremental reschedule.
struct FailingScheduler;

impl crate::traits::scheduling::Scheduler for FailingScheduler {
    fn schedule(
        &self,
        _input: crate::traits::scheduling::ScheduleInput,
    ) -> Result<crate::traits::scheduling::ScheduleResult, AppError> {
        Err(AppError::Internal("mock: scheduler failed".to_owned()))
    }
}

#[test]
fn reschedule_error_after_remote_delete_propagates() {
    let (store, now, chunk_id) = store_with_synced_chunk("ev-gone");

    // Remote deleted the event → local delete + reschedule, which fails.
    let sync = connected_sync(store.as_ref(), vec![]);
    let trigger = make_trigger(&store);

    let err = sync_cycle(store.as_ref(), &sync, &FailingScheduler, &trigger, now).unwrap_err();
    assert!(
        matches!(&err, AppError::Internal(msg) if msg.contains("scheduler failed")),
        "reschedule failure must propagate, got: {err:?}"
    );

    // The Step-2 local apply committed before the reschedule ran.
    assert!(
        store.get_chunk(&chunk_id).expect("get chunk").is_none(),
        "chunk deletion must have committed before the failed reschedule"
    );
}

#[test]
fn mass_delete_guard_aborts() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();

    for i in 0..10u32 {
        let start = now + Duration::hours(i64::from(i) + 1);
        let end = start + Duration::hours(1);
        let (_, chunk_id) = insert_task_with_chunk(store.as_ref(), start, now);
        insert_sync_base(
            store.as_ref(),
            &chunk_id,
            &format!("ev-{i}"),
            start,
            end,
            now,
        );
    }

    // Remote returns no events → would delete all 10.
    let sync = connected_sync(store.as_ref(), vec![]);
    let err = run_cycle(&store, &sync, now).unwrap_err();
    assert!(
        matches!(&err, AppError::CalendarSync(msg) if msg.contains("mass delete guard")),
        "expected mass delete guard error, got: {err:?}"
    );
}

#[test]
fn honors_valid_remote_move() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let (_task_id, chunk_id, _, _) = seed_synced_chunk(store.as_ref(), "ev-move", now);

    // Remote moved the event to T+2h–T+3h.
    let new_start = now + Duration::hours(2);
    let new_end = new_start + Duration::hours(1);
    let remote = make_remote_event("ev-move", new_start, new_end);
    let sync = connected_sync(store.as_ref(), vec![remote]);
    run_cycle(&store, &sync, now).expect("sync_cycle ok");

    // No SyncOps issued (remote move is accepted).
    let ops = sync.get_recorded_sync_ops();
    assert!(
        ops.is_empty(),
        "no ops for accepted remote move; got {ops:?}"
    );

    // Chunk must be updated locally to new times and pinned.
    let chunk = store.get_chunk(&chunk_id).expect("get chunk").unwrap();
    assert_eq!(chunk.start_time, new_start, "chunk start must be updated");
    assert_eq!(chunk.end_time, new_end, "chunk end must be updated");
    assert!(chunk.is_fixed, "chunk must be pinned after remote move");

    // Sync base must reflect new times.
    assert_single_base_times(store.as_ref(), now, new_start, new_end);
}

/// Insert a second scheduled chunk for an existing task. Returns its id.
fn insert_extra_chunk(
    store: &dyn Store,
    task_id: &str,
    start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> String {
    let chunk_id = uuid::Uuid::now_v7().to_string();
    store
        .create_chunk(&scheduled_chunk(&chunk_id, task_id, start, now))
        .expect("create extra chunk");
    chunk_id
}

#[test]
fn remote_delete_of_both_chunks_same_task_dedups_reschedule() {
    let TwoChunkTask {
        store,
        now,
        chunk1,
        chunk2,
        start1,
        start2,
    } = two_chunk_task();
    insert_sync_base(
        store.as_ref(),
        &chunk1,
        "ev-1",
        start1,
        start1 + Duration::hours(1),
        now,
    );
    insert_sync_base(
        store.as_ref(),
        &chunk2,
        "ev-2",
        start2,
        start2 + Duration::hours(1),
        now,
    );

    // Remote deleted both events (2 of 2 stays under the max(5, n/2) guard).
    let sync = connected_sync(store.as_ref(), vec![]);
    run_cycle(&store, &sync, now).expect("sync_cycle ok");

    assert!(
        store.get_chunk(&chunk1).expect("get chunk1").is_none(),
        "chunk1 must be deleted"
    );
    assert!(
        store.get_chunk(&chunk2).expect("get chunk2").is_none(),
        "chunk2 must be deleted"
    );
}

#[test]
fn accepts_two_remote_moves_on_same_task() {
    let TwoChunkTask {
        store,
        now,
        chunk1,
        chunk2,
        start1,
        start2,
    } = two_chunk_task();
    // Bases must carry the multi-chunk descriptions or the content-changed
    // comparison would route to case G (app wins) instead of accepting the move.
    for (chunk_id, event_id, start, part) in
        [(&chunk1, "ev-1", start1, 1), (&chunk2, "ev-2", start2, 2)]
    {
        store
            .upsert_chunk_sync_state(&ChunkSyncState {
                chunk_id: (*chunk_id).clone(),
                event_id: event_id.to_owned(),
                etag: Some("etag-v1".to_owned()),
                synced_start: start,
                synced_end: start + Duration::hours(1),
                synced_title: "Test task".to_owned(),
                synced_description: format!("Part {part} of 2 — Après Work"),
                updated_at: now,
            })
            .expect("upsert sync base");
    }

    // Remote moved both events forward 30 minutes (still before the deadline).
    let moved1 = start1 + Duration::minutes(30);
    let moved2 = start2 + Duration::minutes(30);
    let sync = connected_sync(
        store.as_ref(),
        vec![
            make_remote_event("ev-1", moved1, moved1 + Duration::hours(1)),
            make_remote_event("ev-2", moved2, moved2 + Duration::hours(1)),
        ],
    );
    run_cycle(&store, &sync, now).expect("sync_cycle ok");

    let ops = sync.get_recorded_sync_ops();
    assert!(ops.is_empty(), "no ops for accepted moves; got {ops:?}");

    let c1 = store.get_chunk(&chunk1).expect("get chunk1").unwrap();
    assert_eq!(c1.start_time, moved1);
    assert!(c1.is_fixed, "chunk1 must be pinned");
    let c2 = store.get_chunk(&chunk2).expect("get chunk2").unwrap();
    assert_eq!(c2.start_time, moved2);
    assert!(c2.is_fixed, "chunk2 must be pinned");
}

#[test_case::test_case(true ; "title_changed")]
#[test_case::test_case(false ; "description_changed")]
fn content_change_triggers_update_op(change_title: bool) {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let (task_id, _chunk_id, start, end) = seed_synced_chunk(store.as_ref(), "ev-1", now);

    // Change content only — times stay equal to the base.
    let mut task = store.get_task(&task_id).expect("get task").unwrap();
    if change_title {
        task.title = "Renamed task".to_owned();
    } else {
        task.description = Some("New notes".to_owned());
    }
    store.update_task(&task).expect("update task");

    // Remote still at base times/content.
    let remote = make_remote_event("ev-1", start, end);
    let ops = run_cycle_one_remote(&store, now, remote);
    assert_eq!(ops.len(), 1, "content change must issue one Update");
    match &ops[0] {
        crate::traits::calendar_sync::SyncOp::Update { event_id, payload } => {
            assert_eq!(event_id, "ev-1");
            if change_title {
                assert_eq!(payload.title, "Renamed task");
            } else {
                assert!(
                    payload.description.contains("New notes"),
                    "description must carry the new notes: {}",
                    payload.description
                );
            }
        }
        other => panic!("expected Update, got {other:?}"),
    }

    // Base must reflect the new content.
    let bases = bases_in_next_month(store.as_ref(), now);
    assert_eq!(bases.len(), 1);
    if change_title {
        assert_eq!(bases[0].synced_title, "Renamed task");
    } else {
        assert!(bases[0].synced_description.contains("New notes"));
    }
}

#[test]
fn reverts_invalid_remote_move_past_deadline() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let original_start = now + Duration::minutes(30);
    let original_end = original_start + Duration::hours(1);

    let (task_id, chunk_id) = insert_task_with_chunk(store.as_ref(), original_start, now);

    // Set a tight deadline: task expires at original_end.
    let mut task = store.get_task(&task_id).expect("get task").unwrap();
    task.deadline = Some(original_end);
    store.update_task(&task).expect("update task");

    insert_sync_base(
        store.as_ref(),
        &chunk_id,
        "ev-revert",
        original_start,
        original_end,
        now,
    );

    // Remote moved event end PAST the deadline.
    let bad_end = original_end + Duration::hours(1);
    let remote = make_remote_event("ev-revert", original_start, bad_end);
    // Must issue an Update op reverting to original times.
    let ops = run_cycle_one_remote(&store, now, remote);
    assert_eq!(ops.len(), 1, "must revert with an Update op");
    assert!(
        matches!(&ops[0], crate::traits::calendar_sync::SyncOp::Update { event_id, payload }
            if event_id == "ev-revert"
            && payload.start == original_start
            && payload.end == original_end),
        "Update op must restore original times; got {ops:?}"
    );
}

#[test]
fn conflict_app_wins() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();

    // Base (synced) times: T+0 – T+1h.
    let base_start = now + Duration::hours(1);
    let base_end = base_start + Duration::hours(1);

    // Local chunk moved to T+30min – T+1h30min.
    let local_start = now + Duration::minutes(30 + 60);
    let local_end = local_start + Duration::hours(1);

    let (_, chunk_id) = insert_task_with_chunk(store.as_ref(), local_start, now);
    // Manually set start to base so seeded chunk matches insert_task_with_chunk,
    // then update to local_start:
    let mut chunk = store.get_chunk(&chunk_id).expect("get chunk").unwrap();
    chunk.start_time = local_start;
    chunk.end_time = local_end;
    chunk.updated_at = now + Duration::seconds(5);
    store.update_chunk(&chunk).expect("update chunk");

    insert_sync_base(
        store.as_ref(),
        &chunk_id,
        "ev-conflict",
        base_start,
        base_end,
        now,
    );

    // Remote also changed: moved to T+15min – T+1h15min (different from base).
    let remote_start = base_start + Duration::minutes(15);
    let remote_end = remote_start + Duration::hours(1);
    let remote = make_remote_event("ev-conflict", remote_start, remote_end);
    // App wins: must issue Update with LOCAL times.
    let ops = run_cycle_one_remote(&store, now, remote);
    assert_eq!(ops.len(), 1, "must issue exactly one Update op");
    assert!(
        matches!(&ops[0], crate::traits::calendar_sync::SyncOp::Update { payload, .. }
            if payload.start == local_start && payload.end == local_end),
        "Update must carry local (app) times; got {ops:?}"
    );
}

#[test]
fn orphaned_remote_event_deleted() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let start = now + Duration::hours(1);
    let end = start + Duration::hours(1);

    // No local chunks, no sync bases — but remote has one app-owned event.
    let remote = make_remote_event("ev-orphan", start, end);
    let (counts, ops) = push_events(&store, now, vec![remote]);
    assert_eq!(ops.len(), 1, "must issue exactly one Delete op");
    assert!(
        matches!(&ops[0], crate::traits::calendar_sync::SyncOp::Delete { event_id } if event_id == "ev-orphan"),
        "must delete the orphaned remote event"
    );
    assert_eq!(
        counts,
        PushCounts {
            created: 0,
            updated: 0,
            deleted: 1
        },
        "returned counts must reflect the single Delete"
    );
}

#[test]
fn ensure_calendar_error_propagates() {
    // No calendar_id in google_auth → triggers ensure_app_calendar.
    let (store, now) = store_with_no_calendar_id();

    let sync =
        MockCalendarSync::new(true, std::collections::HashMap::new()).with_ensure_calendar_error();
    let err = run_cycle(&store, &sync, now).unwrap_err();
    assert!(
        matches!(err, AppError::CalendarSync(_)),
        "expected CalendarSync error from ensure_app_calendar, got: {err:?}"
    );
}

#[test]
fn stores_calendar_id_when_none_present() {
    // No calendar_id stored yet.
    let (store, now) = store_with_no_calendar_id();

    let sync = MockCalendarSync::new(true, std::collections::HashMap::new())
        .with_app_calendar("new-cal-id", vec![]);
    run_cycle(&store, &sync, now).expect("sync_cycle ok");

    // The calendar_id must be persisted.
    let auth = store
        .get_google_auth()
        .expect("get google auth")
        .expect("auth row must exist");
    assert_eq!(
        auth.calendar_id,
        Some("new-cal-id".to_owned()),
        "calendar_id must be stored after ensure_app_calendar"
    );
}

#[test]
fn execute_sync_ops_error_propagates() {
    let store = Arc::new(SqliteStore::new_in_memory());
    let now = test_now();
    let chunk_start = now + Duration::hours(1);

    // A chunk with no sync base → triggers a Create op.
    insert_task_with_chunk(store.as_ref(), chunk_start, now);

    // Provider is available but execute_sync_ops returns an error.
    let sync = connected_sync(store.as_ref(), vec![]).with_execute_ops_error("provider boom");
    let err = run_cycle(&store, &sync, now).unwrap_err();
    assert!(
        matches!(err, crate::error::AppError::CalendarSync(_)),
        "expected CalendarSync error from execute_sync_ops, got: {err:?}"
    );
}

#[test_case::test_case(None, "Après Work" ; "no_description")]
#[test_case::test_case(Some("Do laundry"), "Après Work\n\nDo laundry" ; "with_description")]
#[test_case::test_case(Some("   "), "Après Work" ; "whitespace_only_acts_as_none")]
fn make_chunk_description_single_chunk(description: Option<&str>, expected: &str) {
    let task = crate::domain::models::Task {
        description: description.map(str::to_owned),
        ..crate::domain::models::Task::test_default()
    };
    let desc = super::make_chunk_description(&task, 0, 1);
    assert_eq!(desc, expected);
}

#[test]
fn make_chunk_description_multi_chunk_with_description() {
    let task = crate::domain::models::Task {
        description: Some("Long project".to_owned()),
        ..crate::domain::models::Task::test_default()
    };
    let desc = super::make_chunk_description(&task, 1, 3);
    assert_eq!(desc, "Part 2 of 3 — Après Work\n\nLong project");
}

#[test]
fn upsert_chunk_sync_base_no_ops_when_owning_task_missing() {
    // Defensive guard: a chunk whose owning task row is gone (an FK-impossible
    // state) is skipped, leaving no sync base — matching the pre-extraction
    // `else continue` that both call sites shared.
    let store = SqliteStore::new_in_memory();
    let now = test_now();
    let start = now + Duration::hours(1);
    let orphan = scheduled_chunk("orphan-chunk", "no-such-task", start, now);

    super::upsert_chunk_sync_base(&store, &orphan, "ev-orphan", None, now)
        .expect("missing task must no-op, not error");

    assert!(
        bases_in_next_month(&store, now).is_empty(),
        "no sync base may be written when the owning task is missing"
    );
}
