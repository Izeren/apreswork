// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for slot expansion, grid alignment, and interval subtraction.

use chrono::{DateTime, Duration, NaiveTime, Utc, Weekday};
use test_case::test_case;

use crate::domain::models::{Schedule, ScheduleWindow};
use crate::error::AppError;
use crate::test_support::{fixture_base, utc};
use crate::traits::scheduling::AvailableSlot;

use super::{align_slots_to_grid, expand_schedule_windows, subtract_intervals, OccupiedInterval};

fn make_schedule(id: &str, windows: Vec<ScheduleWindow>) -> Schedule {
    Schedule {
        id: id.to_owned(),
        name: id.to_owned(),
        is_default: false,
        windows,
        created_at: fixture_base(),
        updated_at: fixture_base(),
    }
}

fn make_window(
    id: &str,
    schedule_id: &str,
    day: Weekday,
    start_hm: (u32, u32),
    end_hm: (u32, u32),
) -> ScheduleWindow {
    ScheduleWindow {
        id: id.to_owned(),
        schedule_id: schedule_id.to_owned(),
        day_of_week: day,
        start_time: NaiveTime::from_hms_opt(start_hm.0, start_hm.1, 0).expect("valid time"),
        end_time: NaiveTime::from_hms_opt(end_hm.0, end_hm.1, 0).expect("valid time"),
    }
}

/// Single-window expansion: schedule "s1" holds one window on `day` spanning
/// `win_start`–`win_end` (local wall-clock), expanded over the `horizon` in
/// `tz`; it must yield exactly one slot at the `expected` UTC interval.
///
/// Covers plain UTC conversion, both DST transitions, horizon clipping, and the
/// horizon boundary days. DST rationale:
/// - spring-forward (`America/New_York`, 2026-03-08 02:00 EST → 03:00 EDT): an
///   evening window is wholly in EDT (UTC-4).
/// - fall-back (`America/New_York`, 2026-11-01 02:00 EDT → 01:00 EST): an
///   evening window is wholly in EST (UTC-5); a window touching the ambiguous
///   01:00 hour resolves its start to the earliest (EDT, UTC-4) instant.
#[test_case(
    "Europe/Berlin", Weekday::Mon, (18, 0), (20, 0),
    (utc(2026, 3, 16, 0, 0), utc(2026, 3, 16, 23, 59)),
    (utc(2026, 3, 16, 17, 0), utc(2026, 3, 16, 19, 0))
    ; "berlin winter Mon 18-20 local to 17-19 UTC")]
#[test_case(
    "America/New_York", Weekday::Sun, (18, 0), (23, 0),
    (utc(2026, 3, 8, 0, 0), utc(2026, 3, 9, 6, 0)),
    (utc(2026, 3, 8, 22, 0), utc(2026, 3, 9, 3, 0))
    ; "NY spring-forward evening window is EDT")]
#[test_case(
    "America/New_York", Weekday::Sun, (18, 0), (22, 0),
    (utc(2026, 11, 1, 0, 0), utc(2026, 11, 2, 6, 0)),
    (utc(2026, 11, 1, 23, 0), utc(2026, 11, 2, 3, 0))
    ; "NY fall-back evening window is EST")]
#[test_case(
    "America/New_York", Weekday::Sun, (1, 0), (2, 0),
    (utc(2026, 11, 1, 0, 0), utc(2026, 11, 2, 0, 0)),
    (utc(2026, 11, 1, 5, 0), utc(2026, 11, 1, 7, 0))
    ; "NY fall-back ambiguous 01:00 picks earliest UTC")]
#[test_case(
    "Europe/Berlin", Weekday::Mon, (17, 0), (20, 0),
    (utc(2026, 3, 16, 17, 0), utc(2026, 3, 16, 23, 0)),
    (utc(2026, 3, 16, 17, 0), utc(2026, 3, 16, 19, 0))
    ; "horizon clips start of window")]
#[test_case(
    "Europe/Berlin", Weekday::Mon, (18, 0), (22, 0),
    (utc(2026, 3, 16, 0, 0), utc(2026, 3, 16, 20, 0)),
    (utc(2026, 3, 16, 17, 0), utc(2026, 3, 16, 20, 0))
    ; "horizon clips end of window")]
#[test_case(
    "Europe/Berlin", Weekday::Mon, (0, 0), (1, 0),
    (utc(2026, 3, 15, 23, 0), utc(2026, 3, 16, 1, 0)),
    (utc(2026, 3, 15, 23, 0), utc(2026, 3, 16, 0, 0))
    ; "window on first horizon day")]
#[test_case(
    "Europe/Berlin", Weekday::Wed, (18, 0), (20, 0),
    (utc(2026, 3, 18, 0, 0), utc(2026, 3, 18, 23, 0)),
    (utc(2026, 3, 18, 17, 0), utc(2026, 3, 18, 19, 0))
    ; "window on last horizon day")]
fn single_window_expands_to_expected_slot(
    tz: &str,
    day: Weekday,
    win_start: (u32, u32),
    win_end: (u32, u32),
    horizon: (DateTime<Utc>, DateTime<Utc>),
    expected: (DateTime<Utc>, DateTime<Utc>),
) {
    let window = make_window("w1", "s1", day, win_start, win_end);
    let schedule = make_schedule("s1", vec![window]);

    let slots = expand_schedule_windows(&[schedule], tz, horizon.0, horizon.1).expect("no error");

    assert_eq!(slots.len(), 1, "expected exactly one slot");
    assert_eq!(slots[0].start, expected.0);
    assert_eq!(slots[0].end, expected.1);
    assert_eq!(slots[0].schedule_id, "s1");
}

/// Multiple schedules on the same day produce a slot per matching window,
/// sorted by start time.
#[test]
fn multiple_schedules_same_day() {
    // 2026-03-16 Monday, Europe/Berlin (UTC+1 winter).
    let start = utc(2026, 3, 16, 0, 0);
    let end = utc(2026, 3, 16, 23, 59);

    // Schedule A: 18:00–19:00 local → 17:00–18:00 UTC
    let w_a = make_window("w_a", "sA", Weekday::Mon, (18, 0), (19, 0));
    let sched_a = make_schedule("sA", vec![w_a]);

    // Schedule B: 20:00–22:00 local → 19:00–21:00 UTC
    let w_b = make_window("w_b", "sB", Weekday::Mon, (20, 0), (22, 0));
    let sched_b = make_schedule("sB", vec![w_b]);

    let slots = expand_schedule_windows(&[sched_a, sched_b], "Europe/Berlin", start, end)
        .expect("no error");

    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].schedule_id, "sA");
    assert_eq!(slots[0].start, utc(2026, 3, 16, 17, 0));
    assert_eq!(slots[1].schedule_id, "sB");
    assert_eq!(slots[1].start, utc(2026, 3, 16, 19, 0));
}

#[test]
fn no_matching_windows_returns_empty() {
    // Horizon is a single Tuesday. Window is on Wednesday.
    let start = utc(2026, 3, 17, 0, 0); // Tuesday
    let end = utc(2026, 3, 17, 23, 59);

    let window = make_window("w1", "s1", Weekday::Wed, (18, 0), (20, 0));
    let schedule = make_schedule("s1", vec![window]);

    let slots =
        expand_schedule_windows(&[schedule], "Europe/Berlin", start, end).expect("no error");

    assert!(slots.is_empty());
}

#[test]
fn empty_schedules_returns_empty() {
    let start = utc(2026, 3, 16, 0, 0);
    let end = utc(2026, 3, 16, 23, 0);

    let slots = expand_schedule_windows(&[], "Europe/Berlin", start, end).expect("no error");
    assert!(slots.is_empty());
}

/// Invalid timezone string returns `AppError::Validation`.
#[test_case("Not/ATimezone" ; "slash but invalid")]
#[test_case("garbage" ; "no slash at all")]
#[test_case("" ; "empty string")]
fn invalid_timezone_returns_validation_error(tz: &str) {
    let result = expand_schedule_windows(&[], tz, utc(2026, 3, 16, 0, 0), utc(2026, 3, 17, 0, 0));
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "expected Validation error for tz={tz:?}, got {result:?}"
    );
}

/// Sorted output: multiple windows on different days must come back sorted
/// by start time regardless of schedule/window ordering in the input.
#[test]
fn results_sorted_by_start_time() {
    // Horizon: Mon–Wed 2026-03-16..=2026-03-18, Europe/Berlin UTC+1.
    let start = utc(2026, 3, 16, 0, 0);
    let end = utc(2026, 3, 18, 23, 0);

    // Wed window earlier in UTC than Mon window intentionally placed first.
    let w_wed = make_window("w_wed", "s1", Weekday::Wed, (8, 0), (9, 0));
    let w_mon = make_window("w_mon", "s1", Weekday::Mon, (18, 0), (20, 0));
    let schedule = make_schedule("s1", vec![w_wed, w_mon]);

    let slots =
        expand_schedule_windows(&[schedule], "Europe/Berlin", start, end).expect("no error");

    assert_eq!(slots.len(), 2);
    assert!(
        slots[0].start <= slots[1].start,
        "slots not sorted: {slots:?}"
    );
    // Monday slot should be first (earlier in the week).
    assert_eq!(slots[0].start, utc(2026, 3, 16, 17, 0));
    assert_eq!(slots[1].start, utc(2026, 3, 18, 7, 0));
}

#[test]
fn schedule_with_no_windows_returns_empty() {
    let start = utc(2026, 3, 16, 0, 0);
    let end = utc(2026, 3, 17, 0, 0);
    let schedule = make_schedule("s1", vec![]);

    let slots =
        expand_schedule_windows(&[schedule], "Europe/Berlin", start, end).expect("no error");
    assert!(slots.is_empty());
}

#[test]
fn slot_fields_populated() {
    let start = utc(2026, 3, 16, 0, 0);
    let end = utc(2026, 3, 16, 23, 0);
    let window = make_window("w1", "sched-42", Weekday::Mon, (18, 0), (20, 0));
    let schedule = make_schedule("sched-42", vec![window]);

    let slots =
        expand_schedule_windows(&[schedule], "Europe/Berlin", start, end).expect("no error");

    assert_eq!(slots.len(), 1);
    let AvailableSlot {
        start: s,
        end: e,
        schedule_id,
    } = &slots[0];
    assert_eq!(schedule_id, "sched-42");
    assert!(s < e, "start must be before end");
}

/// Horizon with start == end produces no slots (degenerate range).
#[test]
fn zero_length_horizon_returns_empty() {
    let t = utc(2026, 3, 16, 12, 0);
    let window = make_window("w1", "s1", Weekday::Mon, (11, 0), (13, 0));
    let schedule = make_schedule("s1", vec![window]);

    let slots = expand_schedule_windows(&[schedule], "Europe/Berlin", t, t).expect("no error");
    assert!(slots.is_empty());
}

fn make_slot(schedule_id: &str, start_h: u32, end_h: u32) -> AvailableSlot {
    AvailableSlot {
        start: utc(2026, 3, 16, start_h, 0),
        end: utc(2026, 3, 16, end_h, 0),
        schedule_id: schedule_id.to_owned(),
    }
}

fn make_occ(start_h: u32, end_h: u32) -> OccupiedInterval {
    OccupiedInterval {
        start: utc(2026, 3, 16, start_h, 0),
        end: utc(2026, 3, 16, end_h, 0),
    }
}

/// Assert `slot` spans `[start_h:00, end_h:00)` on the fixed 2026-03-16 test day.
fn assert_fragment(slot: &AvailableSlot, start_h: u32, end_h: u32) {
    assert_eq!(slot.start, utc(2026, 3, 16, start_h, 0));
    assert_eq!(slot.end, utc(2026, 3, 16, end_h, 0));
}

/// Like [`assert_fragment`], additionally asserting the fragment's `schedule_id`.
fn assert_slot(slot: &AvailableSlot, schedule_id: &str, start_h: u32, end_h: u32) {
    assert_eq!(slot.schedule_id, schedule_id);
    assert_fragment(slot, start_h, end_h);
}

#[test]
fn subtract_both_empty() {
    let result = subtract_intervals(&[], &[]);
    assert!(result.is_empty());
}

#[test]
fn subtract_empty_slots() {
    let occ = [make_occ(10, 11)];
    let result = subtract_intervals(&[], &occ);
    assert!(result.is_empty());
}

#[test]
fn subtract_empty_occupied() {
    let slots = [make_slot("s1", 10, 12)];
    let result = subtract_intervals(&slots, &[]);
    assert_eq!(result.len(), 1);
    assert_slot(&result[0], "s1", 10, 12);
}

#[test]
fn subtract_non_overlapping_occupied() {
    let slots = [make_slot("s1", 10, 12)];
    let occ = [make_occ(13, 14)]; // after the slot
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 1);
    assert_fragment(&result[0], 10, 12);
}

#[test]
fn subtract_adjacent_boundary_occ_end_equals_slot_start() {
    let slots = [make_slot("s1", 12, 14)];
    let occ = [make_occ(10, 12)]; // ends exactly at slot start
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 1);
    assert_fragment(&result[0], 12, 14);
}

#[test_case(10, 11, 11, 13 ; "occupied covers first hour of three-hour slot")]
#[test_case(10, 12, 12, 13 ; "occupied covers first two hours of three-hour slot")]
fn subtract_partial_overlap_start(occ_s: u32, occ_e: u32, expected_s: u32, expected_e: u32) {
    let slots = [make_slot("s1", 10, 13)];
    let occ = [OccupiedInterval {
        start: utc(2026, 3, 16, occ_s, 0),
        end: utc(2026, 3, 16, occ_e, 0),
    }];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 1);
    assert_slot(&result[0], "s1", expected_s, expected_e);
}

#[test]
fn subtract_partial_overlap_end() {
    // slot: 10–13, occupied: 12–14 → free: 10–12
    let slots = [make_slot("s1", 10, 13)];
    let occ = [make_occ(12, 14)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 1);
    assert_slot(&result[0], "s1", 10, 12);
}

#[test]
fn subtract_full_containment() {
    let slots = [make_slot("s1", 11, 12)];
    let occ = [make_occ(10, 13)];
    let result = subtract_intervals(&slots, &occ);
    assert!(result.is_empty());
}

#[test]
fn subtract_slot_contains_occupied() {
    // slot: 10–14, occupied: 11–12 → free: [10–11, 12–14]
    let slots = [make_slot("s1", 10, 14)];
    let occ = [make_occ(11, 12)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 2);
    assert_slot(&result[0], "s1", 10, 11);
    assert_slot(&result[1], "s1", 12, 14);
}

#[test]
fn subtract_multiple_occupied_three_fragments() {
    // slot: 10–16, occupied: [11–12, 13–14] → free: [10–11, 12–13, 14–16]
    let slots = [make_slot("s1", 10, 16)];
    let occ = [make_occ(11, 12), make_occ(13, 14)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 3);
    assert_fragment(&result[0], 10, 11);
    assert_fragment(&result[1], 12, 13);
    assert_fragment(&result[2], 14, 16);
}

#[test]
fn subtract_nested_overlapping_occupied() {
    // slot: 10–16, occupied: [11–14, 12–15] (overlap 12–14)
    // merged occupied span: 11–15 → free: [10–11, 15–16]
    let slots = [make_slot("s1", 10, 16)];
    let occ = [make_occ(11, 14), make_occ(12, 15)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 2);
    assert_fragment(&result[0], 10, 11);
    assert_fragment(&result[1], 15, 16);
}

#[test]
fn subtract_multiple_slots_schedule_id_preserved() {
    // sA: 10–12 (no occupied)
    // sB: 14–18, occupied: 15–16 → [14–15, 16–18]
    let slots = [make_slot("sA", 10, 12), make_slot("sB", 14, 18)];
    let occ = [make_occ(15, 16)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 3);
    assert_slot(&result[0], "sA", 10, 12);
    assert_slot(&result[1], "sB", 14, 15);
    assert_slot(&result[2], "sB", 16, 18);
}

#[test]
fn subtract_overlapping_schedules_both_survive() {
    // sA: 10–14, sB: 12–16 (overlap 12–14), occupied: 12–13
    // sA → [10–12, 13–14]; sB → [13–16]
    let slots = [make_slot("sA", 10, 14), make_slot("sB", 12, 16)];
    let occ = [make_occ(12, 13)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 3, "got: {result:?}");
    assert_slot(&result[0], "sA", 10, 12);
    assert_slot(&result[1], "sA", 13, 14);
    assert_slot(&result[2], "sB", 13, 16);
}

#[test]
fn subtract_identical_windows_two_schedules() {
    // sA and sB: 10–13, occupied: 11–12 → each gets [10–11, 12–13]
    let slots = [make_slot("sA", 10, 13), make_slot("sB", 10, 13)];
    let occ = [make_occ(11, 12)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 4, "got: {result:?}");
    // Sorted by (start, schedule_id): (10 sA), (10 sB), (12 sA), (12 sB)
    assert_slot(&result[0], "sA", 10, 11);
    assert_slot(&result[1], "sB", 10, 11);
    assert_slot(&result[2], "sA", 12, 13);
    assert_slot(&result[3], "sB", 12, 13);
}

#[test]
fn subtract_adjacent_slots_same_schedule_both_survive() {
    // s1: 18–20 and 20–22, no occupied → both slots returned as-is
    let slots = [make_slot("s1", 18, 20), make_slot("s1", 20, 22)];
    let result = subtract_intervals(&slots, &[]);
    assert_eq!(result.len(), 2, "got: {result:?}");
    assert_fragment(&result[0], 18, 20);
    assert_fragment(&result[1], 20, 22);
}

#[test]
fn subtract_adjacent_slots_busy_straddles_boundary() {
    // s1: 18–20 and 20–22, occupied: 19–21 → [18–19, 21–22]
    let slots = [make_slot("s1", 18, 20), make_slot("s1", 20, 22)];
    let occ = [make_occ(19, 21)];
    let result = subtract_intervals(&slots, &occ);
    assert_eq!(result.len(), 2, "got: {result:?}");
    assert_fragment(&result[0], 18, 19);
    assert_fragment(&result[1], 21, 22);
}

fn ragged_slot(start: DateTime<Utc>, end: DateTime<Utc>, schedule_id: &str) -> AvailableSlot {
    AvailableSlot {
        start,
        end,
        schedule_id: schedule_id.to_owned(),
    }
}

#[test_case(
    Duration::nanoseconds(47_606_881_160), Duration::seconds(30),
    (10, 1), (12, 0)
    ; "nanosecond start and second end")]
#[test_case(
    Duration::seconds(30), Duration::milliseconds(59_999),
    (10, 1), (12, 0)
    ; "second start and millisecond end")]
#[test_case(Duration::zero(), Duration::zero(), (10, 0), (12, 0)
    ; "aligned input unchanged")]
fn align_snaps_boundaries_inward(
    start_offset: Duration,
    end_offset: Duration,
    expected_start_hm: (u32, u32),
    expected_end_hm: (u32, u32),
) {
    let slot = ragged_slot(
        utc(2026, 3, 16, 10, 0) + start_offset,
        utc(2026, 3, 16, 12, 0) + end_offset,
        "s1",
    );

    let result = align_slots_to_grid(vec![slot]).expect("align ok");

    assert_eq!(result.len(), 1, "got: {result:?}");
    let (sh, sm) = expected_start_hm;
    let (eh, em) = expected_end_hm;
    assert_eq!(result[0].start, utc(2026, 3, 16, sh, sm));
    assert_eq!(result[0].end, utc(2026, 3, 16, eh, em));
    assert_eq!(result[0].schedule_id, "s1");
}

#[test]
fn align_drops_sub_minute_slot() {
    // 10:00:20 – 10:00:50 → start ceils to 10:01, end floors to 10:00 → gone.
    let slot = ragged_slot(
        utc(2026, 3, 16, 10, 0) + Duration::seconds(20),
        utc(2026, 3, 16, 10, 0) + Duration::seconds(50),
        "s1",
    );

    let result = align_slots_to_grid(vec![slot]).expect("align ok");
    assert!(result.is_empty(), "got: {result:?}");
}

#[test]
fn align_preserves_order_and_schedule_ids() {
    let slots = vec![
        ragged_slot(
            utc(2026, 3, 16, 10, 0) + Duration::milliseconds(500),
            utc(2026, 3, 16, 11, 0),
            "sA",
        ),
        ragged_slot(utc(2026, 3, 16, 12, 0), utc(2026, 3, 16, 13, 0), "sB"),
    ];

    let result = align_slots_to_grid(slots).expect("align ok");

    assert_eq!(result.len(), 2, "got: {result:?}");
    assert_eq!(result[0].schedule_id, "sA");
    assert_eq!(result[0].start, utc(2026, 3, 16, 10, 1));
    assert_eq!(result[1].schedule_id, "sB");
    assert_eq!(result[1].start, utc(2026, 3, 16, 12, 0));
}
