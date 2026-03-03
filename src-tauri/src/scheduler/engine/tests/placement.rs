// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Placement tests (`DefaultScheduler`): priority ordering, tiebreaks, greedy
//! fill, slot eligibility, and `start_date` filtering.

use chrono::{DateTime, Utc};
use test_case::test_case;

use crate::domain::enums::{ChunkStatus, Priority};
use crate::domain::models::Task;
use crate::scheduler::engine::consume_slot;
use crate::test_support::utc;
use crate::traits::scheduling::{AvailableSlot, ScheduleResult, WarningKind};

use super::{
    assert_no_placement_no_warnings, assert_single_unschedulable, default_slot, make_fixed_chunk,
    make_slot, make_task, run, run_with_fixed, run_with_policy, total_placed_minutes,
};

/// Run `tasks`/`slots`, assert exactly one chunk was placed starting at
/// `expected_start`, and return the result for further assertions.
fn assert_single_chunk_starts_at(
    tasks: Vec<Task>,
    slots: Vec<AvailableSlot>,
    expected_start: DateTime<Utc>,
) -> ScheduleResult {
    let result = run(tasks, slots);
    assert_eq!(result.placed_chunks.len(), 1);
    assert_eq!(result.placed_chunks[0].start_time, expected_start);
    result
}

fn two_day_18h_slots() -> Vec<AvailableSlot> {
    vec![
        make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0)),
        make_slot("s1", utc(2026, 3, 17, 18, 0), utc(2026, 3, 17, 19, 0)),
    ]
}

fn task_with_start_date_march_17() -> Task {
    let mut task = make_task("t1", "Task", "s1", 60);
    task.start_date = Some(utc(2026, 3, 17, 0, 0));
    task
}

#[test]
fn priority_ordering_critical_placed_before_low() {
    // Two tasks each needing 60 min. Only one 60-min slot available.
    let slot = default_slot();

    let mut critical = make_task("t-crit", "Critical Task", "s1", 60);
    critical.priority = Priority::Critical;

    let mut low = make_task("t-low", "Low Task", "s1", 60);
    low.priority = Priority::Low;

    // Pass low first so we verify sorting, not input order.
    let result = run(vec![low, critical], vec![slot]);

    assert_eq!(result.placed_chunks.len(), 1);
    assert_eq!(result.placed_chunks[0].task_id, "t-crit");

    // The low task is unschedulable (slot consumed by critical).
    assert_single_unschedulable(&result);
    assert_eq!(result.warnings[0].task_id, "t-low");
}

fn make_tiebreak_task(id: &str, title: &str, dur: i64, deadline: Option<DateTime<Utc>>) -> Task {
    let mut task = make_task(id, title, "s1", dur);
    task.deadline = deadline;
    task
}

#[test_case(
    make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0)),
    make_tiebreak_task("t-early", "Early Deadline", 60, Some(utc(2026, 3, 17, 0, 0))),
    make_tiebreak_task("t-late", "Late Deadline", 60, Some(utc(2026, 3, 20, 0, 0))),
    "t-early" ; "earlier_deadline_placed_first"
)]
#[test_case(
    make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 18, 30)),
    make_tiebreak_task("t-short", "Short Task", 30, None),
    make_tiebreak_task("t-long", "Long Task", 120, None),
    "t-short" ; "shorter_remaining_placed_first"
)]
#[test_case(
    make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0)),
    make_tiebreak_task("t-dl", "Has Deadline", 60, Some(utc(2026, 3, 20, 0, 0))),
    make_tiebreak_task("t-no", "No Deadline", 60, None),
    "t-dl" ; "no_deadline_sorts_after_deadline"
)]
#[test_case(
    make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0)),
    make_tiebreak_task("t-a", "Alpha", 60, None),
    make_tiebreak_task("t-b", "Beta", 60, None),
    "t-a" ; "title_alphabetically_first_placed_first"
)]
fn tiebreak_ordering(slot: AvailableSlot, winner: Task, loser: Task, expected_winner_id: &str) {
    // Loser is passed first to verify sorting is by criterion, not input order.
    let result = run(vec![loser, winner], vec![slot]);
    assert_eq!(result.placed_chunks.len(), 1);
    assert_eq!(result.placed_chunks[0].task_id, expected_winner_id);
}

#[test]
fn greedy_fill_across_multiple_slots() {
    // Task needs 90 min; slots: 60 min + 60 min.
    let slots = two_day_18h_slots();
    let task = make_task("t1", "Task", "s1", 90);

    let result = run(vec![task], slots);

    assert_eq!(result.placed_chunks.len(), 2);
    assert!(result.warnings.is_empty());
    assert_eq!(total_placed_minutes(&result), 90);
}

#[test]
fn min_chunk_skip_slot_smaller_than_min_chunk_minutes() {
    // min_chunk_minutes = 30. First slot is only 20 min (too small).
    // Second slot is 60 min (large enough).
    let mut task = make_task("t1", "Task", "s1", 60);
    task.min_chunk_minutes = 30;

    let slots = vec![
        make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 18, 20)),
        make_slot("s1", utc(2026, 3, 16, 19, 0), utc(2026, 3, 16, 20, 0)),
    ];

    let result = assert_single_chunk_starts_at(vec![task], slots, utc(2026, 3, 16, 19, 0));
    assert!(result.warnings.is_empty());
}

fn run_min_chunk30_two_slots(
    dur: i64,
    slot2_start: DateTime<Utc>,
    slot2_end: DateTime<Utc>,
) -> ScheduleResult {
    let mut task = make_task("t1", "Task", "s1", dur);
    task.min_chunk_minutes = 30;
    let slots = vec![
        make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 18, 30)),
        make_slot("s1", slot2_start, slot2_end),
    ];
    run(vec![task], slots)
}

/// Remainder < `min_chunk_minutes` must not land in a slot narrower than `min_chunk_minutes`
/// (old code produced micro-chunks). Exception: if the slot is at least `min_chunk_minutes`
/// wide, the undersized remainder fills it (final-chunk rule).
#[test_case(40, utc(2026, 3, 16, 19, 0), utc(2026, 3, 16, 19, 30), 2, 40, false ; "final_chunk_exception_allows_undersized_last_chunk")]
#[test_case(34, utc(2026, 3, 16, 20, 56), utc(2026, 3, 16, 21, 0), 1, 30, true ; "undersized_remainder_not_placed_in_slot_below_min_chunk")]
fn min_chunk_undersized_remainder(
    dur: i64,
    slot2_start: DateTime<Utc>,
    slot2_end: DateTime<Utc>,
    expected_chunk_count: usize,
    expected_total_minutes: i64,
    expect_unschedulable: bool,
) {
    let result = run_min_chunk30_two_slots(dur, slot2_start, slot2_end);
    assert_eq!(result.placed_chunks.len(), expected_chunk_count);
    assert_eq!(total_placed_minutes(&result), expected_total_minutes);
    if expect_unschedulable {
        assert_single_unschedulable(&result);
    } else {
        assert!(result.warnings.is_empty());
    }
}

#[test]
fn no_split_task_placed_as_single_chunk() {
    let mut task = make_task("t1", "Task", "s1", 60);
    task.no_split = true;

    let slots = vec![
        make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 18, 30)),
        make_slot("s1", utc(2026, 3, 16, 19, 0), utc(2026, 3, 16, 20, 30)),
    ];

    let result = run(vec![task], slots);

    assert_eq!(result.placed_chunks.len(), 1);
    let chunk = &result.placed_chunks[0];
    assert_eq!((chunk.end_time - chunk.start_time).num_minutes(), 60);
    assert_eq!(chunk.start_time, utc(2026, 3, 16, 19, 0));
    assert!(result.warnings.is_empty());
}

#[test]
fn no_split_unschedulable_when_no_slot_large_enough() {
    let mut task = make_task("t1", "Big Task", "s1", 120);
    task.no_split = true;

    // Two 60-min slots — neither fits 120 min as a single chunk.
    let slots = two_day_18h_slots();

    let result = run(vec![task], slots);

    assert!(result.placed_chunks.is_empty());
    assert_single_unschedulable(&result);
}

/// A task whose `start_date` is past `horizon_end` (here 2027, vs the
/// `make_input` horizon of 2026-12-31) has no eligible slot. It must be
/// deferred silently — never placed, and warned about only when its deadline
/// falls within the horizon (a genuine "due before it can start" conflict).
#[test_case(None,                         0 ; "no deadline → deferred, no warning")]
#[test_case(Some(utc(2027, 3, 1, 0, 0)),  0 ; "deadline also beyond horizon → deferred")]
#[test_case(Some(utc(2026, 12, 1, 0, 0)), 1 ; "deadline within horizon → genuine conflict warns")]
fn future_start_beyond_horizon_deferred_unless_deadline_in_horizon(
    deadline: Option<DateTime<Utc>>,
    expected_warnings: usize,
) {
    let mut task = make_task("t1", "Future Task", "s1", 60);
    task.start_date = Some(utc(2027, 2, 1, 0, 0));
    task.deadline = deadline;
    let slots = vec![default_slot()];

    let result = run(vec![task], slots);

    assert!(result.placed_chunks.is_empty());
    assert_eq!(result.warnings.len(), expected_warnings);
}

#[test]
fn splittable_unschedulable_when_insufficient_total_capacity() {
    // Task needs 120 min; only 60 min available in total.
    let task = make_task("t1", "Long Task", "s1", 120);
    let slots = vec![default_slot()];

    let result = run(vec![task], slots);

    assert_eq!(result.placed_chunks.len(), 1);
    assert_single_unschedulable(&result);
}

#[test]
fn fixed_chunk_deduction_reduces_remaining_duration() {
    // Task needs 120 min. A 60-min fixed chunk already exists.
    // The engine should only place the remaining 60 min.
    let task = make_task("t1", "Task", "s1", 120);
    let fixed = make_fixed_chunk("t1", utc(2026, 3, 16, 17, 0), utc(2026, 3, 16, 18, 0));

    // Available slot: exactly 60 min (plus the fixed chunk area is blocked).
    let slots = vec![
        // This slot overlaps with the fixed chunk — will be subtracted.
        make_slot("s1", utc(2026, 3, 16, 17, 0), utc(2026, 3, 16, 18, 0)),
        // This slot is free.
        make_slot("s1", utc(2026, 3, 16, 19, 0), utc(2026, 3, 16, 20, 0)),
    ];

    let result = run_with_fixed(task, fixed, slots);

    assert_eq!(result.placed_chunks.len(), 1);
    let dur = (result.placed_chunks[0].end_time - result.placed_chunks[0].start_time).num_minutes();
    assert_eq!(dur, 60);
    assert!(result.warnings.is_empty());
}

#[test]
fn schedule_affinity_task_only_placed_in_matching_schedule_id_slots() {
    // Task belongs to schedule "s1"; slots from "s2" should be ignored.
    let task = make_task("t1", "Task", "s1", 60);
    let slots = vec![
        make_slot("s2", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0)),
        make_slot("s1", utc(2026, 3, 16, 20, 0), utc(2026, 3, 16, 21, 0)),
    ];

    assert_single_chunk_starts_at(vec![task], slots, utc(2026, 3, 16, 20, 0));
}

#[test]
fn empty_tasks_returns_empty_result() {
    let slots = vec![default_slot()];
    let result = run(vec![], slots);

    assert_no_placement_no_warnings(&result);
}

#[test]
fn deadline_violation_emitted_when_chunk_ends_after_deadline() {
    // Task with 60 min, deadline BEFORE the only available slot.
    let mut task = make_task("t1", "Overdue Task", "s1", 60);
    task.deadline = Some(utc(2026, 3, 16, 17, 0));

    let slots = vec![make_slot(
        "s1",
        utc(2026, 3, 16, 18, 0),
        utc(2026, 3, 16, 19, 0),
    )];

    let result = run(vec![task], slots);

    // The chunk is still placed (scheduler is optimistic).
    assert_eq!(result.placed_chunks.len(), 1);

    assert_eq!(result.warnings.len(), 1);
    match &result.warnings[0].kind {
        WarningKind::DeadlineViolation {
            deadline,
            earliest_completion,
        } => {
            assert_eq!(*deadline, utc(2026, 3, 16, 17, 0));
            assert_eq!(*earliest_completion, utc(2026, 3, 16, 19, 0));
        }
        other @ WarningKind::Unschedulable { .. } => {
            panic!("expected DeadlineViolation, got {other:?}")
        }
    }
}

/// Parametrize over chunk positions within a 10–20h slot.
/// (`chunk_start_h`, `chunk_end_h`, `expected_fragment_starts`, `expected_fragment_ends`)
#[test_case(10, 12, &[12], &[20] ; "chunk at start — only trailing fragment")]
#[test_case(18, 20, &[10], &[18] ; "chunk at end — only leading fragment")]
#[test_case(12, 18, &[10, 18], &[12, 20] ; "chunk in middle — two fragments")]
#[test_case(10, 20, &[], &[] ; "chunk fills slot — no fragments")]
fn consume_slot_splits_correctly(
    chunk_start_h: u32,
    chunk_end_h: u32,
    expected_starts_h: &[u32],
    expected_ends_h: &[u32],
) {
    let mut slots = vec![make_slot(
        "s1",
        utc(2026, 3, 16, 10, 0),
        utc(2026, 3, 16, 20, 0),
    )];

    consume_slot(
        &mut slots,
        0,
        utc(2026, 3, 16, chunk_start_h, 0),
        utc(2026, 3, 16, chunk_end_h, 0),
    );

    assert_eq!(
        slots.len(),
        expected_starts_h.len(),
        "wrong number of fragments"
    );
    for (i, (es, ee)) in expected_starts_h.iter().zip(expected_ends_h).enumerate() {
        assert_eq!(
            slots[i].start,
            utc(2026, 3, 16, *es, 0),
            "fragment {i} start"
        );
        assert_eq!(slots[i].end, utc(2026, 3, 16, *ee, 0), "fragment {i} end");
    }
}

#[test]
fn no_slots_all_tasks_unschedulable() {
    let task = make_task("t1", "Task", "s1", 60);
    let result = run(vec![task], vec![]);

    assert!(result.placed_chunks.is_empty());
    assert_single_unschedulable(&result);
}

#[test]
fn zero_duration_task_produces_no_chunks() {
    let task = make_task("t1", "Task", "s1", 0);
    let slots = vec![default_slot()];
    let result = run(vec![task], slots);
    assert_no_placement_no_warnings(&result);
}

/// Verify chunk fields: `is_fixed=false`, `status=Scheduled`, correct `task_id`.
#[test]
fn placed_chunk_fields_are_correct() {
    let task = make_task("t1", "Task", "s1", 30);
    let slots = vec![make_slot(
        "s1",
        utc(2026, 3, 16, 18, 0),
        utc(2026, 3, 16, 18, 30),
    )];

    let result = run(vec![task], slots);
    assert_eq!(result.placed_chunks.len(), 1);

    let chunk = &result.placed_chunks[0];
    assert_eq!(chunk.task_id, "t1");
    assert!(!chunk.is_fixed);
    assert_eq!(chunk.status, ChunkStatus::Scheduled);
    assert!(chunk.logged_minutes.is_none());
    assert!(chunk.completed_at.is_none());
    assert!(chunk.google_event_id.is_none());
    assert_eq!(chunk.start_time, utc(2026, 3, 16, 18, 0));
    assert_eq!(chunk.end_time, utc(2026, 3, 16, 18, 30));
}

/// Exercises the `None` branch of `if let Some(latest_fixed_end) = ...` inside
/// `warn_if_fixed_past_deadline` — no fixed chunks → helper finds nothing → no warning.
#[test_case(None ; "no_deadline")]
#[test_case(Some(utc(2026, 3, 16, 17, 0)) ; "with_deadline_no_fixed_chunks")]
fn overallocated_task_skipped(deadline: Option<DateTime<Utc>>) {
    let mut task = make_task("t1", "Task", "s1", 60);
    task.time_logged_minutes = 60;
    task.deadline = deadline;
    let slots = vec![default_slot()];
    let result = run(vec![task], slots);
    assert_no_placement_no_warnings(&result);
}

/// A task fully covered by a fixed chunk that ends past the task deadline must
/// emit a `DeadlineViolation` warning even though no new chunk is placed.
///
/// Fixed chunk: 18:00–19:00 (60 min), covers all 60 min of the task.
///
/// Case parametrization:
/// - deadline BEFORE fixed-end → warning emitted (boundary: strict >)
/// - deadline AFTER fixed-end  → no warning
/// - deadline == fixed-end     → no warning (strict >, not >=)
/// - no deadline               → no warning
#[test_case(Some(utc(2026, 3, 16, 17, 0)), true  ; "fixed_past_deadline_warns")]
#[test_case(Some(utc(2026, 3, 16, 20, 0)), false ; "fixed_before_deadline_no_warn")]
#[test_case(Some(utc(2026, 3, 16, 19, 0)), false ; "fixed_end_eq_deadline_no_warn")]
#[test_case(None, false ; "no_deadline_no_warn")]
fn fixed_chunk_fully_covers_task_deadline_violation(
    deadline: Option<DateTime<Utc>>,
    expect_warning: bool,
) {
    let mut task = make_task("t1", "Task", "s1", 60);
    task.deadline = deadline;
    let fixed = make_fixed_chunk("t1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0));
    // A free slot after the fixed chunk — ensures no new auto-chunk is placed.
    let slots = vec![make_slot(
        "s1",
        utc(2026, 3, 16, 20, 0),
        utc(2026, 3, 16, 21, 0),
    )];

    let result = run_with_fixed(task, fixed, slots);
    assert!(result.placed_chunks.is_empty());

    if expect_warning {
        assert_eq!(result.warnings.len(), 1, "expected one DeadlineViolation");
        let WarningKind::DeadlineViolation {
            deadline: w_dl,
            earliest_completion,
        } = &result.warnings[0].kind
        else {
            panic!(
                "expected DeadlineViolation, got {:?}",
                result.warnings[0].kind
            );
        };
        assert_eq!(
            *w_dl,
            deadline.expect("deadline must be Some when warning expected")
        );
        assert_eq!(*earliest_completion, utc(2026, 3, 16, 19, 0));
    } else {
        assert!(
            result.warnings.is_empty(),
            "unexpected warnings: {:?}",
            result.warnings
        );
    }
}

/// Regression guard for the C1 infinite-loop: with `min_break_minutes = 0`,
/// a continuous-work budget of zero produces `chunk_dur = 0`. Without the
/// `chunk_dur <= 0` guard the engine would place a zero-length chunk, leave the
/// slot unchanged, and loop forever. The engine must instead advance to the next
/// slot and eventually terminate.
///
/// Setup: one 120-min slot + one 30-min slot; policy `max_cont=60`, `min_break=0`.
/// Step 1: 60 min placed in slot 1 (budget hit).
/// Step 2: slot 1 remainder, budget still 0, `chunk_dur=0` → skip slot.
/// Step 3: slot 2 at 21:00 (gap from 19:00 → budget resets to 60) → place 30 min.
/// Expected: 2 chunks (60+30=90 min), no warnings.
#[test]
fn zero_min_break_budget_exhaustion_skips_slot_and_terminates() {
    let mut task = make_task("t1", "Task", "s1", 90);
    task.min_chunk_minutes = 30;

    let slots = vec![
        make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 20, 0)), // 120 min
        make_slot("s1", utc(2026, 3, 16, 21, 0), utc(2026, 3, 16, 21, 30)), // 30 min
    ];

    // min_break = 0: no mandatory break, but the cont budget caps individual chunks.
    let result = run_with_policy(vec![task], slots, 60, 0);

    assert_eq!(result.placed_chunks.len(), 2, "expected two placed chunks");
    assert_eq!(total_placed_minutes(&result), 90);
    assert!(result.warnings.is_empty());
}

#[test_case(
    task_with_start_date_march_17(),
    two_day_18h_slots(),
    utc(2026, 3, 17, 18, 0) ; "start_date_filters_early_slots"
)]
#[test_case(
    make_task("t1", "Task", "s1", 60),
    vec![
        make_slot("s1", utc(2026, 3, 1, 18, 0), utc(2026, 3, 1, 19, 0)),
        make_slot("s1", utc(2026, 3, 2, 18, 0), utc(2026, 3, 2, 19, 0)),
    ],
    utc(2026, 3, 1, 18, 0) ; "start_date_none_allows_all_slots"
)]
#[test_case(
    { let mut t = make_task("t1", "Task", "s1", 60); t.start_date = Some(utc(2026, 3, 17, 18, 0)); t },
    vec![make_slot("s1", utc(2026, 3, 17, 18, 0), utc(2026, 3, 17, 19, 0))],
    utc(2026, 3, 17, 18, 0) ; "start_date_exact_boundary"
)]
#[test_case(
    task_with_start_date_march_17(),
    vec![
        make_slot("s1", utc(2026, 3, 16, 20, 0), utc(2026, 3, 16, 23, 0)),
        make_slot("s1", utc(2026, 3, 17, 6, 0), utc(2026, 3, 17, 8, 0)),
    ],
    utc(2026, 3, 17, 6, 0) ; "start_date_mid_slot"
)]
#[test_case(
    task_with_start_date_march_17(),
    vec![
        make_slot("s1", utc(2026, 3, 16, 18, 0), utc(2026, 3, 16, 19, 0)),
        make_slot("s2", utc(2026, 3, 17, 18, 0), utc(2026, 3, 17, 19, 0)),
        make_slot("s1", utc(2026, 3, 17, 20, 0), utc(2026, 3, 17, 21, 0)),
    ],
    utc(2026, 3, 17, 20, 0) ; "start_date_with_schedule_affinity"
)]
fn start_date_constraint(
    task: Task,
    slots: Vec<AvailableSlot>,
    expected_chunk_start: DateTime<Utc>,
) {
    let result = assert_single_chunk_starts_at(vec![task], slots, expected_chunk_start);
    assert!(result.warnings.is_empty());
}
