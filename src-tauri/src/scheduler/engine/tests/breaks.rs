// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Break-enforcement tests: the `apply_break` helper plus `max_continuous` /
//! `min_break` behavior across chunks, tasks, and slots.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use crate::domain::enums::Priority;
use crate::domain::models::{Chunk, Task};
use crate::scheduler::engine::apply_break;
use crate::test_support::utc;

use super::{
    assert_no_warnings, default_slot, find_chunk, make_slot, make_task,
    run_two_tasks_expect_two_chunks, run_with_policy, slot_18_to, total_placed_minutes,
};

/// Task "t-b" (Beta): low priority, 30 min, 10-min minimum chunk — the standard
/// second task for the break-ordering tests.
fn low_priority_task_b() -> Task {
    let mut task_b = make_task("t-b", "Beta", "s1", 30);
    task_b.priority = Priority::Low;
    task_b.min_chunk_minutes = 10;
    task_b
}

/// Helper that arranges two tasks in a single [18:00, end) slot and returns their placed chunks (a, b).
fn run_two_tasks_in_one_slot(task_a: Task, task_b: Task, end: DateTime<Utc>) -> (Chunk, Chunk) {
    let slots = vec![slot_18_to(end)];
    let result = run_two_tasks_expect_two_chunks(task_a, task_b, slots);
    let chunk_a = find_chunk(&result, "t-a").clone();
    let chunk_b = find_chunk(&result, "t-b").clone();
    (chunk_a, chunk_b)
}

/// `apply_break`: parametrized over gap vs. adjacent vs. adjacent-but-within-budget
/// cases for the low-level helper.
///
/// Parameters: `(prev_cumulative, max_cont, prev_is_adjacent, expected_delay_minutes)`
/// where `expected_delay_minutes` is the offset added to `slot_start` (0 = no break,
/// `min_break=10` = break forced).
#[test_case(
    60, 60, true, 10 ;
    "adjacent and budget exhausted — break forced by min_break=10"
)]
#[test_case(
    40, 60, true, 0 ;
    "adjacent but within budget — no break"
)]
#[test_case(
    60, 60, false, 0 ;
    "gap before slot — no break regardless of prev cumulative"
)]
#[test_case(
    0, 60, false, 0 ;
    "empty timeline — no break"
)]
fn apply_break_unit(
    prev_cumulative: i64,
    max_cont: i64,
    prev_is_adjacent: bool,
    expected_delay_minutes: i64,
) {
    let base = utc(2026, 3, 16, 18, 0);
    let slot_start = base;
    let min_break = 10_i64;

    let mut timeline: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
    if prev_is_adjacent {
        timeline.insert(slot_start, prev_cumulative);
    } else if prev_cumulative > 0 {
        timeline.insert(slot_start - Duration::minutes(30), prev_cumulative);
    }

    let eff = apply_break(slot_start, &timeline, max_cont, min_break);
    assert_eq!(
        eff,
        slot_start + Duration::minutes(expected_delay_minutes),
        "expected delay {expected_delay_minutes} min"
    );
}

/// A splittable task of 180 min with `max_continuous=60`, `min_break=10`.
/// Each individual chunk must be at most 60 min.
/// The total slot must be large enough to fit 3×60 min work + 2×10 min breaks = 200 min.
#[test]
fn max_continuous_caps_chunk_duration() {
    let mut task = make_task("t1", "Task", "s1", 180);
    task.min_chunk_minutes = 10;

    // Slot of 200 min: 18:00–21:20 (fits 3×60 + 2×10 = 200 exactly).
    let slots = vec![slot_18_to(utc(2026, 3, 16, 21, 20))];

    let result = run_with_policy(vec![task], slots, 60, 10);

    assert_no_warnings(&result);

    for chunk in &result.placed_chunks {
        let dur = (chunk.end_time - chunk.start_time).num_minutes();
        assert!(dur <= 60, "chunk duration {dur} exceeds max_continuous=60");
    }

    assert_eq!(
        total_placed_minutes(&result),
        180,
        "expected 180 min placed"
    );

    assert_eq!(result.placed_chunks.len(), 3, "expected 3 chunks");
}

/// After the first task fills `max_continuous`, the next task's chunk must
/// start after the mandatory break gap.
///
/// Both cases assert identical placement: `chunk_a` 18:00–19:00, `chunk_b` at 19:10.
/// The second case additionally sets `min_chunk_minutes=10` to exercise the
/// "cumulative == `max_cont`" boundary explicitly.
#[test_case(None ; "break inserted between chunks")]
#[test_case(Some(10_i64) ; "zero remaining budget forces break")]
fn break_after_critical_60min_task(min_chunk: Option<i64>) {
    let mut task_a = make_task("t-a", "Alpha", "s1", 60);
    task_a.priority = Priority::Critical;
    if let Some(mc) = min_chunk {
        task_a.min_chunk_minutes = mc;
    }
    let task_b = low_priority_task_b();
    let (chunk_a, chunk_b) = run_two_tasks_in_one_slot(task_a, task_b, utc(2026, 3, 16, 20, 0));

    assert_eq!(chunk_a.start_time, utc(2026, 3, 16, 18, 0));
    assert_eq!(chunk_a.end_time, utc(2026, 3, 16, 19, 0));
    assert_eq!(chunk_b.start_time, utc(2026, 3, 16, 19, 10));
}

/// A `no_split` task longer than `max_continuous` is placed as a single block
/// (exception), but the next task must observe a break after it.
#[test]
fn no_split_exceeding_max_continuous() {
    // Task A: Critical priority, no_split=true, 90 min > max_cont=60.
    let mut task_a = make_task("t-a", "Alpha", "s1", 90);
    task_a.no_split = true;
    task_a.priority = Priority::Critical;

    // Task B: Low priority, 30 min — placed after task A.
    let task_b = low_priority_task_b();

    let slots = vec![slot_18_to(utc(2026, 3, 16, 21, 0))];

    let result = run_two_tasks_expect_two_chunks(task_a, task_b, slots);

    let chunk_a = find_chunk(&result, "t-a");
    let chunk_b = find_chunk(&result, "t-b");

    assert_eq!(chunk_a.start_time, utc(2026, 3, 16, 18, 0));
    assert_eq!(
        (chunk_a.end_time - chunk_a.start_time).num_minutes(),
        90,
        "task A must be a single 90-min chunk"
    );

    // Task B must start at 19:30 + 10 min break = 19:40.
    let expected_b_start = chunk_a.end_time + Duration::minutes(10);
    assert_eq!(
        chunk_b.start_time, expected_b_start,
        "task B must start after break"
    );
}

/// Cross-task continuous time: task A places 40 min, task B can only place
/// 20 min before needing a break (cumulative 40+20 = `max_cont=60`).
#[test]
fn cross_task_continuous_time() {
    let mut task_a = make_task("t-a", "Alpha", "s1", 40);
    task_a.min_chunk_minutes = 10;

    let mut task_b = make_task("t-b", "Beta", "s1", 60);
    task_b.min_chunk_minutes = 10;

    // One long slot for both tasks.
    let slots = vec![slot_18_to(utc(2026, 3, 16, 21, 0))];

    let result = run_with_policy(vec![task_a, task_b], slots, 60, 10);

    assert_no_warnings(&result);

    // Verify cross-task continuous cap: no adjacent chunks sum to > 60 min
    // without a gap.
    let mut sorted = result.placed_chunks.clone();
    sorted.sort_by_key(|c| c.start_time);

    for window in sorted.windows(2) {
        let gap_minutes = (window[1].start_time - window[0].end_time).num_minutes();
        if gap_minutes == 0 {
            let dur0 = (window[0].end_time - window[0].start_time).num_minutes();
            let dur1 = (window[1].end_time - window[1].start_time).num_minutes();
            assert!(
                dur0 + dur1 <= 60,
                "adjacent chunks sum to {} min, exceeds max_cont=60",
                dur0 + dur1
            );
        }
    }

    // Specifically: task A ends at 18:40, task B starts at 18:40 and gets
    // only 20 min before break (18:40–19:00), then break 19:00–19:10,
    // then remaining 40 min (19:10–19:50).
    let chunks_b: Vec<_> = result
        .placed_chunks
        .iter()
        .filter(|c| c.task_id == "t-b")
        .collect();
    assert_eq!(chunks_b.len(), 2, "task B should split into 2 chunks");
    let total_b: i64 = chunks_b
        .iter()
        .map(|c| (c.end_time - c.start_time).num_minutes())
        .sum();
    assert_eq!(total_b, 60, "task B total must be 60 min");
}

/// A natural gap between two slots resets cumulative work time, so the
/// second slot starts with a full `max_continuous` budget.
#[test]
fn gap_resets_cumulative() {
    let mut task = make_task("t1", "Task", "s1", 120);
    task.min_chunk_minutes = 10;

    // Two 60-min slots with a 30-min gap between them.
    let slots = vec![
        default_slot(), // 18:00–19:00, 60 min
        make_slot("s1", utc(2026, 3, 16, 19, 30), utc(2026, 3, 16, 20, 30)), // 60 min, gap of 30 min
    ];

    let result = run_with_policy(vec![task], slots, 60, 10);

    // Should place all 120 min without warnings (gap resets the budget).
    assert_no_warnings(&result);

    assert_eq!(total_placed_minutes(&result), 120);

    // The second slot chunk should start at 19:30 (natural slot boundary),
    // NOT at 19:30+10 (no break needed since there's already a gap).
    let mut sorted = result.placed_chunks.clone();
    sorted.sort_by_key(|c| c.start_time);

    assert_eq!(
        sorted[1].start_time,
        utc(2026, 3, 16, 19, 30),
        "second chunk should start at slot boundary, not after extra break"
    );
}

/// When `apply_break` shifts the effective start by `min_break` such that there
/// is not enough room left in the slot, the slot must be skipped.
#[test]
fn break_pushes_past_slot_end() {
    let mut task = make_task("t1", "Task", "s1", 90);
    task.min_chunk_minutes = 10;

    let slots = vec![
        default_slot(), // 18:00–19:00, 60 min — fills budget
        make_slot("s1", utc(2026, 3, 16, 19, 0), utc(2026, 3, 16, 19, 5)), // 5 min — break pushes past end
        make_slot("s1", utc(2026, 3, 16, 19, 10), utc(2026, 3, 16, 20, 10)), // 60 min — break gap covered
    ];

    let result = run_with_policy(vec![task], slots, 60, 10);

    assert_no_warnings(&result);
    assert_eq!(result.placed_chunks.len(), 2, "expected 2 chunks placed");

    let mut sorted = result.placed_chunks.clone();
    sorted.sort_by_key(|c| c.start_time);
    assert_eq!(sorted[0].start_time, utc(2026, 3, 16, 18, 0));
    assert_eq!(sorted[0].end_time, utc(2026, 3, 16, 19, 0));
    assert_eq!(sorted[1].start_time, utc(2026, 3, 16, 19, 10));
    assert_eq!(sorted[1].end_time, utc(2026, 3, 16, 19, 40));
}
