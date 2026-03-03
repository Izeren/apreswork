// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the reschedule trigger (child module of `services::trigger`).

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use chrono::{DateTime, Utc};
use test_case::test_case;

use super::{
    crossed_midnight_utc, flush_pending_reschedule, policy_for, run_background_maintenance,
    timer_tick, DefaultExecutor, Mutation, PendingReschedule, RescheduleExecutor, RescheduleMode,
    RescheduleTrigger, BACKGROUND_CHECK_INTERVAL,
};
use crate::db::sqlite::SqliteStore;
use crate::domain::enums::ChunkStatus;
use crate::domain::models::Chunk;
use crate::error::AppError;
use crate::test_support::{seed_chunk, test_now, test_store};
use crate::traits::scheduling::ScheduleResult;
use crate::traits::storage::Store;

struct MockExecutor {
    full_calls: Mutex<Vec<DateTime<Utc>>>,
    incremental_calls: Mutex<Vec<(Vec<String>, DateTime<Utc>)>>,
}

impl MockExecutor {
    fn new() -> Self {
        Self {
            full_calls: Mutex::new(Vec::new()),
            incremental_calls: Mutex::new(Vec::new()),
        }
    }

    fn full_call_count(&self) -> usize {
        self.full_calls.lock().expect("lock").len()
    }

    fn incremental_call_count(&self) -> usize {
        self.incremental_calls.lock().expect("lock").len()
    }

    fn last_incremental_ids(&self) -> Vec<String> {
        self.incremental_calls
            .lock()
            .expect("lock")
            .last()
            .map(|(ids, _)| ids.clone())
            .unwrap_or_default()
    }
}

impl RescheduleExecutor for MockExecutor {
    fn execute_full(
        &self,
        _store: &dyn crate::traits::storage::Store,
        now: DateTime<Utc>,
    ) -> Result<ScheduleResult, AppError> {
        self.full_calls.lock().expect("lock").push(now);
        Ok(ScheduleResult {
            placed_chunks: Vec::new(),
            warnings: Vec::new(),
        })
    }

    fn execute_incremental(
        &self,
        _store: &dyn crate::traits::storage::Store,
        task_ids: &[String],
        now: DateTime<Utc>,
    ) -> Result<ScheduleResult, AppError> {
        self.incremental_calls
            .lock()
            .expect("lock")
            .push((task_ids.to_vec(), now));
        Ok(ScheduleResult {
            placed_chunks: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

fn make_trigger(executor: Arc<MockExecutor>) -> RescheduleTrigger {
    RescheduleTrigger::with_debounce_duration(
        Arc::new(test_store()),
        executor,
        Duration::from_secs(3),
    )
}

fn make_zero_debounce_trigger(executor: Arc<MockExecutor>) -> RescheduleTrigger {
    RescheduleTrigger::new(Arc::new(test_store()), executor)
}

/// Return an `Instant` guaranteed to lie in the past (1 second ago).
///
/// Uses `checked_sub` to satisfy `clippy::unchecked_time_subtraction`.
/// The subtraction is safe: program uptime is always > 1 s when tests run.
fn past_instant() -> std::time::Instant {
    std::time::Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or(std::time::Instant::now())
}

fn expire_and_flush(trigger: &RescheduleTrigger) {
    {
        let mut guard = trigger.pending.lock().expect("lock");
        if let Some(p) = guard.as_mut() {
            p.deadline = past_instant();
        }
    }
    trigger.flush_if_ready().expect("flush");
}

fn set_pending_full_ready(trigger: &RescheduleTrigger) {
    let mut guard = trigger.pending.lock().expect("lock");
    *guard = Some(PendingReschedule {
        mode: RescheduleMode::Full,
        deadline: past_instant(),
    });
}

fn trigger_debounced_incremental(trigger: &RescheduleTrigger, task_id: &str) {
    trigger
        .trigger(
            RescheduleMode::Incremental {
                task_ids: vec![task_id.to_owned()],
            },
            false,
        )
        .expect("trigger incremental");
}

#[test_case(RescheduleMode::Full, 1, 0 ; "immediate_full_executes_now")]
#[test_case(RescheduleMode::Incremental { task_ids: vec![] }, 0, 1 ; "immediate_incremental_executes_now")]
fn immediate_trigger_executes_mode(mode: RescheduleMode, expect_full: usize, expect_inc: usize) {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    let result = trigger.trigger(mode, true).expect("trigger");
    assert!(result.is_some());
    assert_eq!(exec.full_call_count(), expect_full);
    assert_eq!(exec.incremental_call_count(), expect_inc);
}

#[test]
fn debounced_stores_pending() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    let result = trigger
        .trigger(RescheduleMode::Full, false)
        .expect("trigger");
    assert!(result.is_none());
    assert_eq!(exec.full_call_count(), 0);
    let guard = trigger.pending.lock().expect("lock");
    assert!(guard.is_some());
}

#[test]
fn zero_debounce_executes_without_storing_pending() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_zero_debounce_trigger(exec.clone());

    let result = trigger
        .trigger(
            RescheduleMode::Incremental {
                task_ids: vec!["t1".to_owned()],
            },
            false,
        )
        .expect("trigger");

    assert!(result.is_some());
    assert_eq!(exec.incremental_call_count(), 1);
    let guard = trigger.pending.lock().expect("lock");
    assert!(guard.is_none());
}

#[test_case(true, 1 ; "flush_executes_when_deadline_passed")]
#[test_case(false, 0 ; "flush_noop_before_deadline")]
fn flush_respects_deadline(past_deadline: bool, expect_calls: usize) {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    {
        let mut guard = trigger.pending.lock().expect("lock");
        *guard = Some(PendingReschedule {
            mode: RescheduleMode::Full,
            deadline: if past_deadline {
                past_instant()
            } else {
                Instant::now() + Duration::from_secs(60)
            },
        });
    }
    let result = trigger.flush_if_ready().expect("flush");
    assert_eq!(result.is_some(), past_deadline);
    assert_eq!(exec.full_call_count(), expect_calls);
}

#[test]
fn coalesce_incremental_merges_task_ids() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    trigger_debounced_incremental(&trigger, "t1");
    trigger_debounced_incremental(&trigger, "t2");
    expire_and_flush(&trigger);
    assert_eq!(exec.incremental_call_count(), 1);
    let ids = exec.last_incremental_ids();
    assert!(ids.contains(&"t1".to_owned()));
    assert!(ids.contains(&"t2".to_owned()));
}

#[test_case(true ; "coalesce_full_supersedes_incremental")]
#[test_case(false ; "coalesce_incremental_with_full_becomes_full")]
fn coalesce_modes_full_wins(incremental_first: bool) {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    if incremental_first {
        trigger_debounced_incremental(&trigger, "t1");
        trigger
            .trigger(RescheduleMode::Full, false)
            .expect("trigger full");
    } else {
        trigger
            .trigger(RescheduleMode::Full, false)
            .expect("trigger full");
        trigger_debounced_incremental(&trigger, "t1");
    }
    expire_and_flush(&trigger);
    assert_eq!(exec.full_call_count(), 1);
    assert_eq!(exec.incremental_call_count(), 0);
}

#[test]
fn immediate_drains_pending() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    trigger_debounced_incremental(&trigger, "t1");
    trigger
        .trigger(RescheduleMode::Full, true)
        .expect("immediate full");
    let guard = trigger.pending.lock().expect("lock");
    assert!(guard.is_none());
    assert_eq!(exec.full_call_count(), 1);
    assert_eq!(exec.incremental_call_count(), 0);
}

#[test]
fn empty_flush_noop() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    let result = trigger.flush_if_ready().expect("flush");
    assert!(result.is_none());
    assert_eq!(exec.full_call_count(), 0);
    assert_eq!(exec.incremental_call_count(), 0);
}

#[test_case(true, 1 ; "flush_pending_reschedule_executes_ready_pending_work")]
#[test_case(false, 0 ; "flush_pending_reschedule_noop_when_nothing_is_ready")]
fn flush_pending_reschedule_behavior(set_pending_ready: bool, expect_full_calls: usize) {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    if set_pending_ready {
        set_pending_full_ready(&trigger);
    }
    flush_pending_reschedule(&trigger);
    assert_eq!(exec.full_call_count(), expect_full_calls);
    assert_eq!(exec.incremental_call_count(), 0);
    let guard = trigger.pending.lock().expect("lock");
    assert!(guard.is_none());
}

#[test]
fn deduplicated_task_ids_on_merge() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger(exec.clone());
    trigger_debounced_incremental(&trigger, "t1");
    trigger_debounced_incremental(&trigger, "t1");
    expire_and_flush(&trigger);
    let ids = exec.last_incremental_ids();
    let t1_count = ids.iter().filter(|id| id.as_str() == "t1").count();
    assert_eq!(t1_count, 1, "task_ids should be deduplicated");
}

/// Expected single-task incremental mode for policy pinning assertions.
fn incremental_for(task_id: &str) -> RescheduleMode {
    RescheduleMode::Incremental {
        task_ids: vec![task_id.to_owned()],
    }
}

// Pins the one trigger-policy table (DESIGN.md §5.2.2). A failure here means
// the policy changed — update the design doc table in the same change.
#[test_case(
    Mutation::TaskCreated { task_id: "t1".to_owned() },
    incremental_for("t1"), false ; "task_created_debounced_incremental"
)]
#[test_case(
    Mutation::TaskUpdated { task_id: "t1".to_owned(), to_backlog: false },
    incremental_for("t1"), false ; "task_updated_debounced_incremental"
)]
#[test_case(
    Mutation::TaskUpdated { task_id: "t1".to_owned(), to_backlog: true },
    RescheduleMode::Full, false ; "task_updated_to_backlog_debounced_full"
)]
#[test_case(
    Mutation::TaskDeleted,
    RescheduleMode::Full, false ; "task_deleted_debounced_full"
)]
#[test_case(
    Mutation::TaskCancelled,
    RescheduleMode::Full, false ; "task_cancelled_debounced_full"
)]
#[test_case(
    Mutation::TaskCompleted { task_id: "t1".to_owned() },
    RescheduleMode::Full, true ; "task_completed_immediate_full"
)]
#[test_case(
    Mutation::FixedChunkCreated,
    RescheduleMode::Full, true ; "fixed_chunk_created_immediate_full"
)]
#[test_case(
    Mutation::ChunkCompleted { task_id: "t1".to_owned() },
    incremental_for("t1"), true ; "chunk_completed_immediate_incremental"
)]
#[test_case(
    Mutation::ChunkReopened { task_id: "t1".to_owned() },
    incremental_for("t1"), true ; "chunk_reopened_immediate_incremental"
)]
#[test_case(
    Mutation::ChunkMoved { task_id: "t1".to_owned() },
    incremental_for("t1"), false ; "chunk_moved_debounced_incremental"
)]
#[test_case(
    Mutation::ChunkResized { task_id: "t1".to_owned() },
    incremental_for("t1"), false ; "chunk_resized_debounced_incremental"
)]
#[test_case(
    Mutation::ChunkLocked { task_id: "t1".to_owned() },
    incremental_for("t1"), false ; "chunk_locked_debounced_incremental"
)]
#[test_case(
    Mutation::ChunkUnlocked { task_id: "t1".to_owned() },
    incremental_for("t1"), false ; "chunk_unlocked_debounced_incremental"
)]
#[test_case(
    Mutation::FixedChunkDeleted { task_id: "t1".to_owned() },
    incremental_for("t1"), false ; "fixed_chunk_deleted_debounced_incremental"
)]
#[test_case(
    Mutation::TemplateCreated,
    RescheduleMode::Full, true ; "template_created_immediate_full"
)]
#[test_case(
    Mutation::TemplateUpdated,
    RescheduleMode::Full, true ; "template_updated_immediate_full"
)]
#[test_case(
    Mutation::TemplateDeleted,
    RescheduleMode::Full, true ; "template_deleted_immediate_full"
)]
#[test_case(
    Mutation::ScheduleCreated,
    RescheduleMode::Full, true ; "schedule_created_immediate_full"
)]
#[test_case(
    Mutation::ScheduleUpdated,
    RescheduleMode::Full, true ; "schedule_updated_immediate_full"
)]
#[test_case(
    Mutation::ScheduleDeleted,
    RescheduleMode::Full, true ; "schedule_deleted_immediate_full"
)]
#[test_case(
    Mutation::ConfigUpdated,
    RescheduleMode::Full, true ; "config_updated_immediate_full"
)]
fn policy_for_pins_the_trigger_table(
    mutation: Mutation,
    expected_mode: RescheduleMode,
    expected_immediate: bool,
) {
    assert_eq!(policy_for(mutation), (expected_mode, expected_immediate));
}

#[test_case(false, Mutation::ChunkCompleted { task_id: "t1".to_owned() }, 0, 1, false ; "immediate_incremental_policy")]
#[test_case(false, Mutation::ConfigUpdated, 1, 0, false ; "immediate_full_policy")]
#[test_case(false, Mutation::ChunkMoved { task_id: "t1".to_owned() }, 0, 0, true ; "debounced_policy")]
#[test_case(true, Mutation::TaskCreated { task_id: "t1".to_owned() }, 0, 1, false ; "zero_debounce_executes_debounced_policy")]
fn trigger_mutation_policy(
    zero_debounce: bool,
    mutation: Mutation,
    expect_full: usize,
    expect_inc: usize,
    expect_pending: bool,
) {
    let exec = Arc::new(MockExecutor::new());
    let trigger = if zero_debounce {
        make_zero_debounce_trigger(exec.clone())
    } else {
        make_trigger(exec.clone())
    };
    trigger
        .trigger_mutation(mutation)
        .expect("trigger_mutation");
    assert_eq!(exec.full_call_count(), expect_full);
    assert_eq!(exec.incremental_call_count(), expect_inc);
    let guard = trigger.pending.lock().expect("lock");
    assert_eq!(guard.is_some(), expect_pending);
}

#[test]
fn execute_waits_for_held_mutation_guard() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = Arc::new(make_zero_debounce_trigger(exec.clone()));

    let guard = trigger.mutation_guard().expect("guard");
    let (tx, rx) = std::sync::mpsc::channel();
    let thread_trigger = Arc::clone(&trigger);
    let handle = std::thread::spawn(move || {
        thread_trigger
            .trigger(RescheduleMode::Full, true)
            .expect("trigger");
        tx.send(()).expect("send");
    });

    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    assert_eq!(exec.full_call_count(), 0);

    drop(guard);
    rx.recv_timeout(Duration::from_secs(5))
        .expect("trigger completes once the guard is released");
    assert_eq!(exec.full_call_count(), 1);
    handle.join().expect("join");
}

#[test]
fn mutation_guard_is_reacquirable_after_drop() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_zero_debounce_trigger(exec.clone());

    drop(trigger.mutation_guard().expect("first acquire"));
    drop(trigger.mutation_guard().expect("second acquire"));
    // The command pattern: guard dropped, then trigger — must not deadlock.
    trigger
        .trigger(RescheduleMode::Full, true)
        .expect("trigger after guard released");
    assert_eq!(exec.full_call_count(), 1);
}

#[test]
fn default_executor_new_does_not_panic() {
    use crate::scheduler::engine::DefaultScheduler;
    let sched = Arc::new(DefaultScheduler);
    let _exec = DefaultExecutor::new(sched);
}

fn make_trigger_with_store(
    store: Arc<dyn Store + Send + Sync>,
    executor: Arc<MockExecutor>,
) -> RescheduleTrigger {
    RescheduleTrigger::new(store, executor)
}

fn run_check_past_due(store: SqliteStore, now: DateTime<Utc>) -> (bool, Arc<MockExecutor>) {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger_with_store(Arc::new(store), exec.clone());
    let result = trigger.check_past_due(now).expect("check_past_due");
    (result, exec)
}

/// Seed a scheduled chunk spanning one hour and ending at `end_time`.
fn seed_scheduled_chunk_ending_at(store: &SqliteStore, id: &str, end_time: DateTime<Utc>) {
    seed_chunk(
        store,
        &Chunk::test_default()
            .with_id(id)
            .with_status(ChunkStatus::Scheduled)
            .with_times(end_time - chrono::Duration::hours(1), end_time),
    );
}

/// `(last_check, now)` straddling UTC midnight: 23:59 previous day → 00:01 next day.
fn midnight_crossing_times() -> (DateTime<Utc>, DateTime<Utc>) {
    let last_check = "2026-03-29T23:59:00Z"
        .parse::<DateTime<Utc>>()
        .expect("parse last_check");
    let now = "2026-03-30T00:01:00Z"
        .parse::<DateTime<Utc>>()
        .expect("parse now");
    (last_check, now)
}

/// A maintenance tick far enough in the past that the check interval has elapsed.
fn elapsed_maintenance_tick() -> Instant {
    Instant::now()
        .checked_sub(BACKGROUND_CHECK_INTERVAL + Duration::from_secs(1))
        .expect("past instant")
}

/// Run background maintenance with a fresh elapsed tick and assert it advanced
/// `last_check` to `now` and triggered exactly one full reschedule. Consolidates
/// the shared tail every "midnight crossing fires" background-maintenance test
/// shares.
fn assert_background_maintenance_triggers_once(
    trigger: &RescheduleTrigger,
    last_check: &mut DateTime<Utc>,
    now: DateTime<Utc>,
    exec: &MockExecutor,
) {
    let mut last_maintenance_tick = elapsed_maintenance_tick();
    run_background_maintenance(trigger, last_check, &mut last_maintenance_tick, now);
    assert_eq!(*last_check, now);
    assert_eq!(exec.full_call_count(), 1);
}

#[test_case(false, 0 ; "check_past_due_no_chunks_returns_false")]
#[test_case(true, 1 ; "check_past_due_with_past_due_chunk_triggers_full_reschedule")]
fn check_past_due_behavior(seed_past_due_chunk: bool, expect_full_calls: usize) {
    let now = test_now();
    let store = test_store();
    if seed_past_due_chunk {
        let end_time = now - chrono::Duration::hours(2);
        seed_scheduled_chunk_ending_at(&store, "chunk-past-due", end_time);
    }
    let (result, exec) = run_check_past_due(store, now);
    assert_eq!(result, seed_past_due_chunk);
    assert_eq!(exec.full_call_count(), expect_full_calls);
}

// Boundary: exactly 1 h ago → the cutoff is `now - 1h`.
// The real SQL query uses `end_time < cutoff` (strict less-than), so a chunk
// ending *exactly* at cutoff is NOT returned.
//
// extra_seconds == 0: end_time == cutoff → NOT past-due (no chunk seeded)
// extra_seconds == 1: end_time == cutoff - 1s → IS past-due (chunk seeded)
#[test_case(0, false ; "exactly_1h_boundary_not_past_due")]
#[test_case(1, true  ; "1h_plus_1s_is_past_due")]
fn check_past_due_boundary(extra_seconds: i64, expect_triggered: bool) {
    let now = test_now();
    // cutoff = now - 1h; end_time = now - 1h - extra_seconds
    let end_time = now - chrono::Duration::hours(1) - chrono::Duration::seconds(extra_seconds);
    let store = test_store();
    // Only seed a chunk when extra_seconds > 0 (i.e., end_time < cutoff).
    // When extra_seconds == 0, end_time == cutoff and the strict-less-than
    // predicate excludes it, so we leave the store empty to match real behaviour.
    if expect_triggered {
        seed_scheduled_chunk_ending_at(&store, "chunk-boundary", end_time);
    }
    let (result, exec) = run_check_past_due(store, now);
    assert_eq!(result, expect_triggered);
    let expected_calls = usize::from(expect_triggered);
    assert_eq!(exec.full_call_count(), expected_calls);
}

#[test]
fn background_maintenance_skips_until_interval_elapsed() {
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_zero_debounce_trigger(exec.clone());
    let original_last_check = test_now() - chrono::Duration::hours(4);
    let mut last_check = original_last_check;
    let mut last_maintenance_tick = Instant::now();

    run_background_maintenance(
        &trigger,
        &mut last_check,
        &mut last_maintenance_tick,
        test_now(),
    );

    assert_eq!(last_check, original_last_check);
    assert_eq!(exec.full_call_count(), 0);
    assert_eq!(exec.incremental_call_count(), 0);
}

#[test]
fn background_maintenance_runs_midnight_reschedule_once_interval_elapsed() {
    let exec = Arc::new(MockExecutor::new());
    // Empty real store: no past-due chunks → only midnight crossing fires.
    let store = Arc::new(test_store());
    let trigger = make_trigger_with_store(store, exec.clone());
    let (mut last_check, now) = midnight_crossing_times();

    assert_background_maintenance_triggers_once(&trigger, &mut last_check, now, &exec);
}

#[test]
fn background_maintenance_skips_midnight_when_past_due_already_triggered() {
    let (mut last_check, now) = midnight_crossing_times();
    // Seed a chunk that ended 2 hours before `now` — past the 1-hour grace.
    let end_time = now - chrono::Duration::hours(2);
    let store = test_store();
    seed_scheduled_chunk_ending_at(&store, "chunk-past-due-bg", end_time);
    let exec = Arc::new(MockExecutor::new());
    let trigger = make_trigger_with_store(Arc::new(store), exec.clone());

    assert_background_maintenance_triggers_once(&trigger, &mut last_check, now, &exec);
}

#[test]
fn timer_tick_skips_when_no_trigger_resolves() {
    let mut last_check = test_now() - chrono::Duration::hours(4);
    let original_last_check = last_check;
    let mut last_maintenance_tick = elapsed_maintenance_tick();

    timer_tick(
        &|| None,
        &mut last_check,
        &mut last_maintenance_tick,
        test_now(),
    );

    assert_eq!(
        last_check, original_last_check,
        "an empty resolution must not advance maintenance state"
    );
}

#[test]
fn timer_tick_runs_maintenance_on_the_resolved_trigger() {
    let exec = Arc::new(MockExecutor::new());
    let store = Arc::new(test_store());
    let trigger = Arc::new(make_trigger_with_store(store, exec.clone()));
    let (mut last_check, now) = midnight_crossing_times();
    let mut last_maintenance_tick = elapsed_maintenance_tick();

    let resolved = trigger.clone();
    timer_tick(
        &move || Some(resolved.clone()),
        &mut last_check,
        &mut last_maintenance_tick,
        now,
    );

    assert_eq!(last_check, now);
    assert_eq!(exec.full_call_count(), 1, "midnight reschedule should fire");
}

#[test_case(
    "2024-01-15T10:00:00Z", "2024-01-15T23:59:59Z", false;
    "same_day_no_crossing"
)]
#[test_case(
    "2024-01-15T23:59:59Z", "2024-01-16T00:00:00Z", true;
    "exactly_midnight_crossing"
)]
#[test_case(
    "2024-01-15T00:00:00Z", "2024-01-16T00:00:00Z", true;
    "full_day_apart_crosses"
)]
#[test_case(
    "2024-01-15T12:00:00Z", "2024-01-17T12:00:00Z", true;
    "two_days_apart_crosses"
)]
#[test_case(
    "2024-01-15T00:00:00Z", "2024-01-15T00:00:00Z", false;
    "identical_timestamps_no_crossing"
)]
fn crossed_midnight_utc_cases(prev_str: &str, now_str: &str, expected: bool) {
    let prev = prev_str.parse::<DateTime<Utc>>().expect("parse prev");
    let now = now_str.parse::<DateTime<Utc>>().expect("parse now");
    assert_eq!(crossed_midnight_utc(prev, now), expected);
}
