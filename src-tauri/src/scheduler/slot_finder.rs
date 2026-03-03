// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Slot expansion: converts abstract schedule windows into concrete UTC intervals.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, DurationRound, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::domain::models::Schedule;
use crate::error::AppError;
use crate::traits::scheduling::AvailableSlot;

/// Grid to which free-slot boundaries snap, in minutes.
///
/// Single definition of the "chunk times are never finer than one minute"
/// policy: the engine only adds whole minutes to slot starts, so aligning the
/// slot pool makes every generated chunk minute-aligned. Sub-minute
/// boundaries enter from horizon clipping at `now` and from busy-interval
/// edges (external events can carry second precision).
pub const SLOT_GRID_MINUTES: i64 = 1;

/// A time interval to subtract from available slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OccupiedInterval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Expand schedule windows into concrete UTC time slots for the planning horizon.
///
/// Iterates each calendar day (in the given `tz`) that overlaps with
/// `[start, end)`, matches schedule windows by weekday, and converts local
/// times to UTC (DST-aware). Slots are clipped to the horizon boundaries and
/// results are sorted by start time.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `tz` is not a valid IANA timezone string.
pub fn expand_schedule_windows(
    schedules: &[Schedule],
    tz: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<AvailableSlot>, AppError> {
    let timezone: Tz = tz
        .parse()
        .map_err(|_| AppError::Validation(format!("invalid timezone: {tz}")))?;

    let mut slots: Vec<AvailableSlot> = Vec::new();

    // We start one day before the UTC start to be safe across any UTC offset.
    let local_start = start.with_timezone(&timezone);
    let local_end = end.with_timezone(&timezone);

    let first_date = local_start.date_naive();
    // Add one day buffer so a window whose local day ends past midnight in UTC
    // is not accidentally dropped.
    let last_date = local_end.date_naive() + Duration::days(1);

    let mut current_date = first_date;
    while current_date < last_date {
        let weekday = current_date.weekday();

        for schedule in schedules {
            for window in &schedule.windows {
                if window.day_of_week != weekday {
                    continue;
                }

                let slot_start_utc = local_to_utc(current_date, window.start_time, timezone)?;
                let slot_end_utc = local_to_utc(current_date, window.end_time, timezone)?;

                if slot_end_utc <= slot_start_utc {
                    continue;
                }

                if slot_end_utc <= start || slot_start_utc >= end {
                    continue;
                }

                let clipped_start = slot_start_utc.max(start);
                let clipped_end = slot_end_utc.min(end);

                slots.push(AvailableSlot {
                    start: clipped_start,
                    end: clipped_end,
                    schedule_id: schedule.id.clone(),
                });
            }
        }

        current_date += Duration::days(1);
    }

    slots.sort_by_key(|s| s.start);
    Ok(slots)
}

/// Snap free slots to the [`SLOT_GRID_MINUTES`] grid: starts round up, ends
/// round down, slots shorter than the grid are dropped. Call this once, after
/// the free-slot pool is fully built (window expansion + busy subtraction);
/// ordering and `schedule_id`s are preserved.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if grid rounding fails (timestamp outside
/// chrono's nanosecond range — unreachable for horizon-bounded slots).
pub fn align_slots_to_grid(slots: Vec<AvailableSlot>) -> Result<Vec<AvailableSlot>, AppError> {
    let grid = Duration::minutes(SLOT_GRID_MINUTES);
    let mut aligned = Vec::with_capacity(slots.len());
    for slot in slots {
        let start = ceil_to_grid(slot.start, grid)?;
        let end = floor_to_grid(slot.end, grid)?;
        if start < end {
            aligned.push(AvailableSlot {
                start,
                end,
                schedule_id: slot.schedule_id,
            });
        }
    }
    Ok(aligned)
}

fn floor_to_grid(dt: DateTime<Utc>, grid: Duration) -> Result<DateTime<Utc>, AppError> {
    dt.duration_trunc(grid)
        .map_err(|e| AppError::Internal(format!("slot grid rounding failed for {dt}: {e}")))
}

fn ceil_to_grid(dt: DateTime<Utc>, grid: Duration) -> Result<DateTime<Utc>, AppError> {
    let floored = floor_to_grid(dt, grid)?;
    if floored == dt {
        Ok(dt)
    } else {
        Ok(floored + grid)
    }
}

/// Convert a local date + time to UTC using the given timezone.
///
/// When the local time falls in a DST gap (spring-forward, the time does not
/// exist), `chrono` returns `LocalResult::None`; we advance by 1 hour to the
/// post-gap time so the window is shortened rather than dropped entirely.
///
/// When the local time is ambiguous (fall-back, the time exists twice), we
/// pick the earliest UTC interpretation (i.e. the pre-transition / summer-
/// time offset) via `LocalResult::Ambiguous` — the `earliest` value.
fn local_to_utc(
    date: NaiveDate,
    time: chrono::NaiveTime,
    tz: Tz,
) -> Result<DateTime<Utc>, AppError> {
    let naive_dt = date.and_time(time);

    let to_utc = |result: chrono::LocalResult<DateTime<Tz>>| -> Option<DateTime<Utc>> {
        match result {
            chrono::LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
            // Ambiguous (fall-back): pick earliest UTC (pre-transition = summer offset).
            chrono::LocalResult::Ambiguous(earliest, _latest) => Some(earliest.with_timezone(&Utc)),
            chrono::LocalResult::None => None,
        }
    };

    if let Some(dt) = to_utc(tz.from_local_datetime(&naive_dt)) {
        Ok(dt)
    } else {
        // Non-existent (spring-forward gap): advance 1 hour to post-gap time.
        let advanced = naive_dt + Duration::hours(1);
        // Extremely unlikely: two consecutive hours in a gap.
        to_utc(tz.from_local_datetime(&advanced)).ok_or_else(|| {
            AppError::Internal(format!(
                "could not resolve local time {advanced} in timezone {tz}"
            ))
        })
    }
}

/// Remove occupied time intervals from available slots using a sweep-line algorithm.
///
/// Slots belonging to different schedules may overlap in wall-clock time (each
/// schedule contributes its own independent availability), so the sweep runs
/// per `schedule_id` group: every `occupied` interval is subtracted from every
/// group, and one schedule's windows never affect another's. Within one
/// schedule, validation guarantees windows do not overlap (touching is legal),
/// which the single-group sweep relies on.
///
/// Each resulting slot inherits the `schedule_id` from its parent slot.
/// Results are sorted by start time, ties broken by `schedule_id` for
/// determinism.
///
/// Complexity: O((S + G·F) log(S + F)) time, O(S + F) space, where G is the
/// number of schedules (occupied intervals are replicated per group; G is a
/// small constant in practice).
#[must_use]
pub fn subtract_intervals(
    slots: &[AvailableSlot],
    occupied: &[OccupiedInterval],
) -> Vec<AvailableSlot> {
    let mut groups: BTreeMap<&str, Vec<&AvailableSlot>> = BTreeMap::new();
    for slot in slots {
        groups
            .entry(slot.schedule_id.as_str())
            .or_default()
            .push(slot);
    }

    let mut result: Vec<AvailableSlot> = Vec::new();
    for group in groups.values() {
        result.extend(subtract_single_schedule(group, occupied));
    }
    result.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| a.schedule_id.cmp(&b.schedule_id))
    });
    result
}

/// Sweep-line subtraction for the slots of a single schedule.
///
/// Precondition: all `slots` share one `schedule_id` and do not overlap each
/// other (schedule-window validation forbids same-day overlap; touching is
/// legal). The sweep keeps a single open-slot cursor, which is only correct
/// under that precondition — [`subtract_intervals`] is the multi-schedule
/// wrapper that establishes it.
// The function is long because the sweep-line algorithm requires inline types
// (`EventKind`, `Event`, `impl Event`) that do not have a natural separate home.
// The logic itself is a straightforward linear sweep and cannot be split without
// introducing unnecessary indirection. All helper types are local and invisible
// outside this function.
#[allow(clippy::too_many_lines)]
fn subtract_single_schedule(
    slots: &[&AvailableSlot],
    occupied: &[OccupiedInterval],
) -> Vec<AvailableSlot> {
    // Event type ordering for tie-breaking at the same timestamp.
    // Lower value = processed first.
    // OccStart(0) < SlotEnd(1) < SlotStart(2) < OccEnd(3)
    //
    // SlotEnd before SlotStart: adjacent slots (touching windows are legal per
    // schedule validation) must close the earlier slot — emitting its final
    // fragment — before the next slot opens; the reverse order clobbers the
    // open `free_start` and silently drops both slots. OccStart first and
    // OccEnd last keep boundary-touching busy intervals non-subtracting.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum EventKind {
        OccStart,
        SlotEnd,
        SlotStart,
        OccEnd,
    }

    // SlotStart and SlotEnd carry only time; all slots in the group share one
    // schedule_id, cached once below. OccStart/OccEnd track depth only.
    #[derive(Debug, Clone, Copy)]
    enum Event {
        SlotStart { time: DateTime<Utc> },
        SlotEnd { time: DateTime<Utc> },
        OccStart { time: DateTime<Utc> },
        OccEnd { time: DateTime<Utc> },
    }

    impl Event {
        fn time(self) -> DateTime<Utc> {
            match self {
                Self::SlotStart { time }
                | Self::SlotEnd { time }
                | Self::OccStart { time }
                | Self::OccEnd { time } => time,
            }
        }

        fn kind(self) -> EventKind {
            match self {
                Self::OccStart { .. } => EventKind::OccStart,
                Self::SlotStart { .. } => EventKind::SlotStart,
                Self::SlotEnd { .. } => EventKind::SlotEnd,
                Self::OccEnd { .. } => EventKind::OccEnd,
            }
        }
    }

    let mut events: Vec<Event> = Vec::with_capacity(2 * slots.len() + 2 * occupied.len());

    for s in slots {
        events.push(Event::SlotStart { time: s.start });
        events.push(Event::SlotEnd { time: s.end });
    }

    // All slots share the same schedule_id (callers group by id); cache it once.
    // Caller (`subtract_intervals`) only pushes non-empty groups, so first() always yields.
    let Some(first) = slots.first() else {
        return Vec::new();
    };
    let schedule_id = &first.schedule_id;

    for o in occupied {
        events.push(Event::OccStart { time: o.start });
        events.push(Event::OccEnd { time: o.end });
    }

    events.sort_by(|a, b| a.time().cmp(&b.time()).then(a.kind().cmp(&b.kind())));

    let mut result: Vec<AvailableSlot> = Vec::new();
    let mut occ_depth: u32 = 0;
    let mut slot_open: bool = false;
    let mut free_start: Option<DateTime<Utc>> = None;

    for event in events {
        match event {
            Event::SlotStart { time } => {
                slot_open = true;
                if occ_depth == 0 {
                    free_start = Some(time);
                }
            }
            Event::SlotEnd { time } => {
                if occ_depth == 0 {
                    if let Some(fs) = free_start {
                        if fs < time {
                            result.push(AvailableSlot {
                                start: fs,
                                end: time,
                                schedule_id: schedule_id.clone(),
                            });
                        }
                    }
                }
                slot_open = false;
                free_start = None;
            }
            Event::OccStart { time } => {
                if occ_depth == 0 && slot_open {
                    if let Some(fs) = free_start {
                        if fs < time {
                            result.push(AvailableSlot {
                                start: fs,
                                end: time,
                                schedule_id: schedule_id.clone(),
                            });
                        }
                        free_start = None;
                    }
                }
                occ_depth += 1;
            }
            Event::OccEnd { time } => {
                // occ_depth > 0 here: each OccEnd is paired with a preceding OccStart
                // that incremented occ_depth, so the subtraction cannot underflow.
                occ_depth -= 1;
                if occ_depth == 0 && slot_open {
                    free_start = Some(time);
                }
            }
        }
    }

    // Events were processed in chronological order, so the group's fragments
    // are already sorted; `subtract_intervals` applies the final cross-group
    // ordering.
    result
}

#[cfg(test)]
mod tests;
