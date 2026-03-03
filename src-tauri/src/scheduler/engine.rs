// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Default scheduling engine — priority-sorted greedy placement.
//!
//! Implements [`Scheduler`] using a greedy algorithm that:
//! 1. Computes remaining duration per task (total minus logged and fixed chunks).
//! 2. Sorts tasks by `(priority DESC, deadline ASC, remaining ASC, title ASC)`.
//! 3. Subtracts fixed/completed chunks from available slots.
//! 4. Places each task greedily into free slots, respecting `no_split`,
//!    `min_chunk_minutes`, and break-enforcement constraints.
//! 5. Emits [`ScheduleWarning`]s for deadline violations and unschedulable tasks.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Utc};

use crate::domain::enums::ChunkStatus;
use crate::domain::models::{Chunk, Task};
use crate::error::AppError;
use crate::scheduler::slot_finder::{subtract_intervals, OccupiedInterval};
use crate::traits::scheduling::{
    scheduling_order, AvailableSlot, ScheduleInput, ScheduleResult, ScheduleWarning, Scheduler,
    WarningKind,
};

/// Stateless default scheduler implementing the greedy placement algorithm.
pub struct DefaultScheduler;

impl Scheduler for DefaultScheduler {
    fn schedule(&self, input: ScheduleInput) -> Result<ScheduleResult, AppError> {
        Ok(schedule_impl(input))
    }
}

/// Whether a task should be deferred because it cannot start within the planning
/// horizon. With `start_date > horizon_end` no slot is eligible (`is_eligible`
/// requires `slot.start >= start_date`), but that is not an "unschedulable"
/// failure — the task simply belongs to a later window, so it is skipped without a
/// warning. A deadline that itself falls within the horizon is the exception: the
/// task is due before it can start, a genuine conflict that must surface normally.
fn deferred_beyond_horizon(task: &Task, horizon_end: DateTime<Utc>) -> bool {
    task.start_date.is_some_and(|start| {
        start > horizon_end && task.deadline.is_none_or(|deadline| deadline > horizon_end)
    })
}

/// If a task is fully covered by fixed/completed chunks but those chunks extend past
/// the task's deadline, emit a `DeadlineViolation` warning so it surfaces in the
/// Status view.
fn warn_if_fixed_past_deadline(
    task: &Task,
    fixed_by_task: &HashMap<String, Vec<&Chunk>>,
    warnings: &mut Vec<ScheduleWarning>,
) {
    if let Some(deadline) = task.deadline {
        if let Some(latest_fixed_end) = fixed_by_task
            .get(&task.id)
            .and_then(|chunks| chunks.iter().map(|c| c.end_time).max())
        {
            if latest_fixed_end > deadline {
                warnings.push(ScheduleWarning {
                    task_id: task.id.clone(),
                    task_title: task.title.clone(),
                    kind: WarningKind::DeadlineViolation {
                        deadline,
                        earliest_completion: latest_fixed_end,
                    },
                });
            }
        }
    }
}

// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
fn schedule_impl(input: ScheduleInput) -> ScheduleResult {
    let ScheduleInput {
        tasks,
        existing_fixed_chunks,
        available_slots,
        horizon_end,
        now,
        max_continuous_minutes,
        min_break_minutes,
    } = input;

    // Step 1 — compute remaining duration per task (SCHEDULER_ALGORITHM.md §5).
    let fixed_by_task = group_fixed_by_task(&existing_fixed_chunks);
    let remaining_by_task: HashMap<String, i64> = tasks
        .iter()
        .map(|task| {
            let fixed_minutes: i64 = fixed_by_task.get(&task.id).map_or(0, |chunks| {
                chunks
                    .iter()
                    .map(|c| (c.end_time - c.start_time).num_minutes())
                    .sum()
            });
            let remaining = task.duration_minutes - task.time_logged_minutes - fixed_minutes;
            (task.id.clone(), remaining.max(0))
        })
        .collect();

    let mut tasks = tasks;
    tasks.sort_by(|a, b| {
        scheduling_order(a, b, |id| remaining_by_task.get(id).copied().unwrap_or(0))
    });

    let occupied: Vec<OccupiedInterval> = existing_fixed_chunks
        .iter()
        .map(|c| OccupiedInterval {
            start: c.start_time,
            end: c.end_time,
        })
        .collect();
    let mut free_slots = subtract_intervals(&available_slots, &occupied);

    let mut placed: Vec<Chunk> = Vec::new();
    let mut warnings: Vec<ScheduleWarning> = Vec::new();

    // Key = chunk end time, value = cumulative continuous minutes up to that end.
    let mut timeline: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();

    for task in &tasks {
        let remaining = remaining_by_task.get(&task.id).copied().unwrap_or(0);

        if remaining <= 0 {
            warn_if_fixed_past_deadline(task, &fixed_by_task, &mut warnings);
            continue;
        }

        if deferred_beyond_horizon(task, horizon_end) {
            continue;
        }

        let task_placed_before = placed.len();
        let mut state = PlacementState {
            free_slots: &mut free_slots,
            placed: &mut placed,
            warnings: &mut warnings,
            timeline: &mut timeline,
            max_cont: max_continuous_minutes,
            min_break: min_break_minutes,
            now,
        };
        if task.no_split {
            place_no_split(task, remaining, &mut state);
        } else {
            place_splittable(task, remaining, &mut state);
        }

        // Deadline violation check: if any chunk was placed, check the latest end.
        // TODO(M1): for partially-covered tasks (remaining > 0 with fixed chunks),
        // this check has two gaps: (a) if a fixed chunk ends after the deadline but
        // the latest *placed* chunk does not, no warning fires at all; (b) if no
        // chunk is placed (Unschedulable) but fixed chunks extend past the deadline,
        // the deadline check is skipped entirely.  Track as a dedicated follow-up.
        if placed.len() > task_placed_before {
            if let Some(deadline) = task.deadline {
                // Safety: the guard `placed.len() > task_placed_before` ensures
                // the slice is non-empty, so `.max()` returns `Some`.
                if let Some(latest_end) = placed[task_placed_before..]
                    .iter()
                    .map(|c| c.end_time)
                    .max()
                {
                    if latest_end > deadline {
                        warnings.push(ScheduleWarning {
                            task_id: task.id.clone(),
                            task_title: task.title.clone(),
                            kind: WarningKind::DeadlineViolation {
                                deadline,
                                earliest_completion: latest_end,
                            },
                        });
                    }
                }
            }
        }
    }

    ScheduleResult {
        placed_chunks: placed,
        warnings,
    }
}

/// Compute the effective start time for a chunk, inserting a break gap if the
/// cumulative continuous work at `slot_start` has reached `max_cont`.
///
/// Returns `slot_start` unchanged when there is a natural gap before the slot,
/// or when the cumulative budget has not yet been exhausted.
/// Returns `slot_start + min_break` when the preceding block is adjacent and
/// has accumulated exactly `max_cont` minutes of continuous work.
fn apply_break(
    slot_start: DateTime<Utc>,
    timeline: &BTreeMap<DateTime<Utc>, i64>,
    max_cont: i64,
    min_break: i64,
) -> DateTime<Utc> {
    // Look up the most recent entry whose end time is ≤ slot_start.
    if let Some((&prev_end, &prev_cumulative)) = timeline.range(..=slot_start).next_back() {
        if prev_end == slot_start && prev_cumulative >= max_cont {
            // Adjacent and budget exhausted — force a break.
            return slot_start + Duration::minutes(min_break);
        }
    }
    // Either no predecessor, a gap exists, or budget not exhausted.
    slot_start
}

/// Record a placed chunk's contribution to the cumulative continuous work
/// timeline.
///
/// If the chunk is adjacent to an existing timeline entry (i.e. a previous
/// chunk ended exactly at `chunk.start_time`), the cumulative value is carried
/// forward. The old entry is preserved as-is; only the new end-time key is
/// inserted (or updated if it already exists, which can happen when two tasks
/// produce chunks ending at the same time).
fn update_timeline(
    timeline: &mut BTreeMap<DateTime<Utc>, i64>,
    chunk_start: DateTime<Utc>,
    chunk_end: DateTime<Utc>,
    chunk_duration: i64,
    max_cont: i64,
) {
    let mut cumulative = chunk_duration;

    // Carry forward from an adjacent predecessor.
    if let Some((&prev_end, &prev_cumulative)) = timeline.range(..=chunk_start).next_back() {
        if prev_end == chunk_start {
            cumulative = prev_cumulative + chunk_duration;
        }
    }

    // Cap at max_cont to avoid unbounded growth (no_split tasks that exceed
    // the budget are capped here for timeline accounting purposes).
    cumulative = cumulative.min(max_cont);

    timeline.insert(chunk_end, cumulative);
}

/// Mutable scheduling state threaded through task placement: the free slots
/// to consume, chunks placed so far, warnings collected, and the continuous-
/// work timeline. Bundled into one struct so `place_no_split` and
/// `place_splittable` share a single parameter instead of duplicating the
/// same six-argument list.
struct PlacementState<'a> {
    free_slots: &'a mut Vec<AvailableSlot>,
    placed: &'a mut Vec<Chunk>,
    warnings: &'a mut Vec<ScheduleWarning>,
    timeline: &'a mut BTreeMap<DateTime<Utc>, i64>,
    max_cont: i64,
    min_break: i64,
    now: DateTime<Utc>,
}

/// Place a `no_split` task as a single chunk into the first eligible slot that
/// can accommodate the entire remaining duration.
///
/// Per spec §5.3 exception: a `no_split` task longer than `max_cont` is placed
/// as a single block (violating the continuous cap), but `update_timeline` is
/// called so that a break is enforced after it for subsequent tasks.
fn place_no_split(task: &Task, remaining: i64, state: &mut PlacementState) {
    let duration = remaining;

    for slot_idx in 0..state.free_slots.len() {
        let slot = state.free_slots[slot_idx].clone();
        if !is_eligible(&slot, task) {
            continue;
        }

        let eff_start = apply_break(slot.start, state.timeline, state.max_cont, state.min_break);
        let available = (slot.end - eff_start).num_minutes();
        if available < duration {
            continue;
        }

        // Found a slot — place the chunk.
        let chunk = make_chunk(task, eff_start, duration, state.now);
        let chunk_end = eff_start + Duration::minutes(duration);
        consume_slot(state.free_slots, slot_idx, eff_start, chunk_end);
        update_timeline(
            state.timeline,
            eff_start,
            chunk_end,
            duration,
            state.max_cont,
        );
        state.placed.push(chunk);
        return;
    }

    // No eligible slot found.
    state.warnings.push(ScheduleWarning {
        task_id: task.id.clone(),
        task_title: task.title.clone(),
        kind: WarningKind::Unschedulable {
            reason: format!("no single slot large enough for {duration} minutes (no_split=true)"),
        },
    });
}

/// Place a splittable task greedily across multiple slots, respecting
/// `min_chunk_minutes`, break-enforcement, and the continuous work cap.
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
fn place_splittable(task: &Task, remaining: i64, state: &mut PlacementState) {
    let mut remaining = remaining;
    let mut slot_idx = 0usize;

    while remaining > 0 && slot_idx < state.free_slots.len() {
        let slot = state.free_slots[slot_idx].clone();

        if !is_eligible(&slot, task) {
            slot_idx += 1;
            continue;
        }

        let eff_start = apply_break(slot.start, state.timeline, state.max_cont, state.min_break);

        // If the break pushes eff_start past the slot end, skip this slot.
        if eff_start >= slot.end {
            slot_idx += 1;
            continue;
        }

        let available_in_slot = (slot.end - eff_start).num_minutes();

        let mut chunk_dur = remaining.min(available_in_slot);

        // Cap at remaining continuous budget.
        let cont_budget = {
            let prev_cumulative = state
                .timeline
                .range(..=eff_start)
                .next_back()
                .and_then(|(&prev_end, &prev_cum)| {
                    if prev_end == eff_start {
                        Some(prev_cum)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            state.max_cont - prev_cumulative
        };
        chunk_dur = chunk_dur.min(cont_budget);

        // Enforce minimum chunk size.
        if chunk_dur < task.min_chunk_minutes {
            if remaining <= task.min_chunk_minutes {
                // Final-chunk exception: the task has a sub-floor remainder. Allow
                // placement only if the slot is at least `min_chunk_minutes` wide AND
                // the continuous-work budget is not exhausted (`chunk_dur > 0`). Without
                // the budget guard a `min_break = 0` config produces zero-length chunks
                // and an infinite loop because `consume_slot` leaves the slot unchanged.
                if available_in_slot < task.min_chunk_minutes || chunk_dur <= 0 {
                    slot_idx += 1;
                    continue;
                }
                // Slot is floor-sized and budget allows; place the sub-floor remainder.
            } else {
                // Not the final chunk and slot is too small — skip.
                slot_idx += 1;
                continue;
            }
        }

        let chunk = make_chunk(task, eff_start, chunk_dur, state.now);
        let chunk_end = eff_start + Duration::minutes(chunk_dur);
        consume_slot(state.free_slots, slot_idx, eff_start, chunk_end);
        update_timeline(
            state.timeline,
            eff_start,
            chunk_end,
            chunk_dur,
            state.max_cont,
        );
        state.placed.push(chunk);
        remaining -= chunk_dur;
        // Do NOT increment slot_idx — consume_slot may have left a remainder
        // fragment at the same index.
    }

    if remaining > 0 {
        state.warnings.push(ScheduleWarning {
            task_id: task.id.clone(),
            task_title: task.title.clone(),
            kind: WarningKind::Unschedulable {
                reason: format!(
                    "{remaining} minutes could not be scheduled (insufficient slot capacity)"
                ),
            },
        });
    }
}

/// Remove the slot at `slot_idx` and insert up to two fragments for the
/// portions of the slot that fall before and after `[chunk_start, chunk_end)`.
/// Fragments are inserted in sorted order at `slot_idx`.
fn consume_slot(
    free_slots: &mut Vec<AvailableSlot>,
    slot_idx: usize,
    chunk_start: DateTime<Utc>,
    chunk_end: DateTime<Utc>,
) {
    let AvailableSlot {
        start: slot_start,
        end: slot_end,
        schedule_id,
    } = free_slots.remove(slot_idx);

    // Fragment before the chunk.
    let mut insert_pos = slot_idx;
    if slot_start < chunk_start {
        free_slots.insert(
            insert_pos,
            AvailableSlot {
                start: slot_start,
                end: chunk_start,
                schedule_id: schedule_id.clone(),
            },
        );
        insert_pos += 1;
    }

    // Fragment after the chunk.
    if chunk_end < slot_end {
        free_slots.insert(
            insert_pos,
            AvailableSlot {
                start: chunk_end,
                end: slot_end,
                schedule_id,
            },
        );
    }
}

/// A slot is eligible for a task if:
/// 1. Their `schedule_id`s match (schedule affinity), and
/// 2. The slot starts on or after the task's `start_date` (if set).
#[inline]
fn is_eligible(slot: &AvailableSlot, task: &Task) -> bool {
    slot.schedule_id == task.schedule_id
        && task
            .start_date
            .is_none_or(|start_date| slot.start >= start_date)
}

fn make_chunk(
    task: &Task,
    start: DateTime<Utc>,
    duration_minutes: i64,
    now: DateTime<Utc>,
) -> Chunk {
    Chunk {
        id: uuid::Uuid::now_v7().to_string(),
        task_id: task.id.clone(),
        start_time: start,
        end_time: start + Duration::minutes(duration_minutes),
        status: ChunkStatus::Scheduled,
        is_fixed: false,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    }
}

fn group_fixed_by_task(chunks: &[Chunk]) -> HashMap<String, Vec<&Chunk>> {
    let mut map: HashMap<String, Vec<&Chunk>> = HashMap::new();
    for chunk in chunks {
        map.entry(chunk.task_id.clone()).or_default().push(chunk);
    }
    map
}

#[cfg(test)]
mod tests;
