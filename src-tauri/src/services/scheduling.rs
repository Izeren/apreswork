// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Full reschedule orchestration service.
//!
//! The [`reschedule`] function implements the full scheduling pipeline:
//! reconcile recurring instances → auto-cancel overdue → shared prep
//! (`ReschedulePrep`: config, horizon, fixed chunks, free slots) →
//! run scheduler → apply diff → sync statuses → persist config.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};

use crate::domain::enums::TaskStatus;
use crate::domain::models::{AppConfig, Chunk, Task};
use crate::error::AppError;
use crate::scheduler::slot_finder::{
    align_slots_to_grid, expand_schedule_windows, subtract_intervals, OccupiedInterval,
};
use crate::services::recurring::{auto_cancel_overdue, reconcile};
use crate::services::task::sync_task_pinned;
use crate::traits::scheduling::{
    scheduling_order, AvailableSlot, ScheduleInput, ScheduleResult, ScheduleWarning, Scheduler,
    WarningKind,
};
use crate::traits::storage::Store;

/// Fixed chunks older than this many hours are considered stale: the scheduled
/// slot was missed, so the lock is dead weight and must be released before the
/// next reschedule so the task can be placed into a future slot.
const STALE_FIXED_LOCK_HOURS: i64 = 4;

/// Release stale fixed-chunk locks before a reschedule pass.
///
/// Any fixed+Scheduled chunk whose `end_time` is more than
/// `STALE_FIXED_LOCK_HOURS` hours before `now` is unlocked (`is_fixed = false`)
/// so the scheduler can reclaim that slot and re-place the owning task.
///
/// Completed chunks are never touched: a completed chunk represents finished
/// work and must not be reverted.
///
/// # Errors
///
/// Returns [`AppError::Database`] on any storage failure.
pub(crate) fn release_stale_fixed_locks(
    store: &dyn Store,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let cutoff = now - Duration::hours(STALE_FIXED_LOCK_HOURS);
    store.with_tx(&mut |tx| {
        let mut affected: HashSet<String> = HashSet::new();
        for mut chunk in tx.get_fixed_scheduled_chunks()? {
            if chunk.end_time < cutoff {
                affected.insert(chunk.task_id.clone());
                chunk.is_fixed = false;
                chunk.updated_at = now;
                tx.update_chunk(&chunk)?;
            }
        }
        for task_id in &affected {
            sync_task_pinned(tx, task_id, now)?;
        }
        Ok(())
    })
}

#[derive(Debug, Clone)]
pub enum DiffOp {
    Keep {
        chunk_id: String,
    },
    Update {
        chunk_id: String,
        new_start: DateTime<Utc>,
        new_end: DateTime<Utc>,
        google_event_id: Option<String>,
    },
    Delete {
        chunk_id: String,
    },
    Create {
        chunk: Chunk,
    },
}

/// Compute the diff between old auto-chunks and new placed chunks.
///
/// Groups by `task_id`, pairs by closest start time (greedy), and classifies
/// each pair as KEEP, UPDATE, DELETE, or CREATE. Preserves `google_event_id`
/// on KEEP and UPDATE ops.
#[must_use]
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
pub fn diff_chunks(old_chunks: &[Chunk], new_chunks: &[Chunk]) -> Vec<DiffOp> {
    let mut old_by_task: HashMap<&str, Vec<&Chunk>> = HashMap::new();
    for chunk in old_chunks {
        old_by_task
            .entry(chunk.task_id.as_str())
            .or_default()
            .push(chunk);
    }
    let mut new_by_task: HashMap<&str, Vec<&Chunk>> = HashMap::new();
    for chunk in new_chunks {
        new_by_task
            .entry(chunk.task_id.as_str())
            .or_default()
            .push(chunk);
    }

    let mut all_task_ids: Vec<&str> = old_by_task
        .keys()
        .chain(new_by_task.keys())
        .copied()
        .collect::<HashSet<&str>>()
        .into_iter()
        .collect();
    all_task_ids.sort_unstable();

    let mut ops: Vec<DiffOp> = Vec::new();

    for task_id in all_task_ids {
        let empty: Vec<&Chunk> = Vec::new();
        let mut old: Vec<&Chunk> = old_by_task.get(task_id).unwrap_or(&empty).clone();
        let mut new: Vec<&Chunk> = new_by_task.get(task_id).unwrap_or(&empty).clone();
        old.sort_unstable_by_key(|c| c.start_time);
        new.sort_unstable_by_key(|c| c.start_time);

        let mut paired_old: HashSet<usize> = HashSet::new();
        let mut paired_new: HashSet<usize> = HashSet::new();

        // Greedy pairing: for each new chunk, find the closest old chunk.
        for (ni, new_chunk) in new.iter().enumerate() {
            let mut best_match: Option<usize> = None;
            let mut best_dist = i64::MAX;

            for (oj, old_chunk) in old.iter().enumerate() {
                if paired_old.contains(&oj) {
                    continue;
                }
                let dist = (new_chunk.start_time - old_chunk.start_time)
                    .num_seconds()
                    .abs();
                if dist < best_dist {
                    best_dist = dist;
                    best_match = Some(oj);
                } else if dist > best_dist {
                    // Sorted → distance only increases from here.
                    break;
                }
            }

            if let Some(oj) = best_match {
                paired_old.insert(oj);
                paired_new.insert(ni);

                let old_chunk = old[oj];
                if old_chunk.start_time == new_chunk.start_time
                    && old_chunk.end_time == new_chunk.end_time
                {
                    ops.push(DiffOp::Keep {
                        chunk_id: old_chunk.id.clone(),
                    });
                } else {
                    ops.push(DiffOp::Update {
                        chunk_id: old_chunk.id.clone(),
                        new_start: new_chunk.start_time,
                        new_end: new_chunk.end_time,
                        google_event_id: old_chunk.google_event_id.clone(),
                    });
                }
            }
        }

        for (oi, old_chunk) in old.iter().enumerate() {
            if !paired_old.contains(&oi) {
                ops.push(DiffOp::Delete {
                    chunk_id: old_chunk.id.clone(),
                });
            }
        }

        for (ni, new_chunk) in new.iter().enumerate() {
            if !paired_new.contains(&ni) {
                ops.push(DiffOp::Create {
                    chunk: (*new_chunk).clone(),
                });
            }
        }
    }

    ops
}

/// Apply a list of [`DiffOp`]s to the store.
fn apply_diff_ops(store: &dyn Store, ops: &[DiffOp], now: DateTime<Utc>) -> Result<(), AppError> {
    for op in ops {
        match op {
            DiffOp::Keep { .. } => {}
            DiffOp::Update {
                chunk_id,
                new_start,
                new_end,
                google_event_id,
            } => {
                if let Some(mut chunk) = store.get_chunk(chunk_id)? {
                    chunk.start_time = *new_start;
                    chunk.end_time = *new_end;
                    chunk.google_event_id.clone_from(google_event_id);
                    chunk.updated_at = now;
                    store.update_chunk(&chunk)?;
                }
            }
            DiffOp::Delete { chunk_id } => {
                store.delete_chunk(chunk_id)?;
            }
            DiffOp::Create { chunk } => {
                store.create_chunk(chunk)?;
            }
        }
    }
    Ok(())
}

/// Preparation state loaded identically by both scheduling pipelines: config
/// and horizon, the immovable chunks, and the free-slot pool with busy external
/// events and fixed chunks already subtracted.
struct ReschedulePrep {
    config: AppConfig,
    horizon_end: DateTime<Utc>,
    /// Fixed/completed (immovable) chunks.
    fixed: Vec<Chunk>,
    /// Schedule windows minus busy external events and fixed chunks.
    free_slots: Vec<AvailableSlot>,
    /// Task ids owning at least one fixed chunk (status sync needs this).
    fixed_task_ids: HashSet<String>,
}

impl ReschedulePrep {
    /// Load the shared prep state. Call AFTER any step that mutates tasks or
    /// chunks (recurring reconcile, overdue auto-cancel, duration fixes) so
    /// the snapshot reflects those changes.
    fn load(store: &dyn Store, now: DateTime<Utc>) -> Result<Self, AppError> {
        let config = store.get_config()?;
        let horizon_end = now + Duration::days(config.planning_horizon_days);

        let fixed = store.get_all_fixed_and_completed()?;

        let all_schedules = store.list_schedules()?;
        let raw_slots =
            expand_schedule_windows(&all_schedules, &config.timezone, now, horizon_end)?;

        // Step 7a: busy intervals come from the mirrored external events
        // (Decision 5). Transparent and declined events carry busy=false and
        // never subtract from slots.
        let external_events = store.get_external_events_in_range(now, horizon_end)?;
        let mut all_occupied: Vec<OccupiedInterval> = external_events
            .iter()
            .filter(|ev| ev.busy)
            .map(|ev| OccupiedInterval {
                start: ev.start_time,
                end: ev.end_time,
            })
            .collect();
        // Fixed chunks are immovable — treat them as occupied time.
        for chunk in &fixed {
            all_occupied.push(OccupiedInterval {
                start: chunk.start_time,
                end: chunk.end_time,
            });
        }
        // Minute-grid alignment keeps every generated chunk minute-precise
        // (see `SLOT_GRID_MINUTES`); ragged edges come from the `now` clip
        // and from busy-interval boundaries.
        let free_slots = align_slots_to_grid(subtract_intervals(&raw_slots, &all_occupied))?;

        let fixed_task_ids: HashSet<String> = fixed.iter().map(|c| c.task_id.clone()).collect();

        Ok(Self {
            config,
            horizon_end,
            fixed,
            free_slots,
            fixed_task_ids,
        })
    }
}

/// Sync one task's status with its chunk placement: `Pending` with chunks
/// becomes `Scheduled`; `Scheduled` without chunks reverts to `Pending`.
///
/// Fetches the task fresh from the store so the write is based on the current
/// row; a task deleted since the pipeline started is skipped.
fn sync_task_status(
    store: &dyn Store,
    task_id: &str,
    has_chunks: bool,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let Some(task) = store.get_task(task_id)? else {
        return Ok(());
    };
    if task.status == TaskStatus::Pending && has_chunks {
        let mut updated = task;
        updated.status = TaskStatus::Scheduled;
        updated.updated_at = now;
        store.update_task(&updated)?;
    } else if task.status == TaskStatus::Scheduled && !has_chunks {
        let mut updated = task;
        updated.status = TaskStatus::Pending;
        updated.updated_at = now;
        store.update_task(&updated)?;
    }
    Ok(())
}

/// Horizon-aware warning filter (DESIGN.md § "Warning semantics").
///
/// The engine reports every placement shortfall; this keeps only warnings that
/// are actionable inside the active horizon:
/// - `DeadlineViolation` is kept only when the violated deadline is on or
///   before `horizon_end` (an in-horizon deadline that cannot be met).
/// - `Unschedulable` is kept only when the task has a deadline on or before
///   `horizon_end` — without one, unplaced work is normal backlog that may
///   become schedulable in a later horizon and stays silently `Pending`.
fn retain_horizon_warnings(
    warnings: &mut Vec<ScheduleWarning>,
    tasks: &[Task],
    horizon_end: DateTime<Utc>,
) {
    let deadline_map: HashMap<&str, DateTime<Utc>> = tasks
        .iter()
        .filter_map(|t| t.deadline.map(|d| (t.id.as_str(), d)))
        .collect();
    warnings.retain(|w| match &w.kind {
        WarningKind::DeadlineViolation { deadline, .. } => *deadline <= horizon_end,
        WarningKind::Unschedulable { .. } => deadline_map
            .get(w.task_id.as_str())
            .is_some_and(|&d| d <= horizon_end),
    });
}

/// Orchestrate a full reschedule of all pending/scheduled tasks.
///
/// Runs the full pipeline: recurring instance generation, overdue
/// cancellation, slot expansion, scheduling, diff application, status updates,
/// and config timestamp update.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the configured timezone is invalid.
/// Returns [`AppError::Database`] on any storage failure.
/// Returns [`AppError::Internal`] if the scheduling algorithm fails.
pub fn reschedule(
    store: &dyn Store,
    scheduler: &dyn Scheduler,
    now: DateTime<Utc>,
) -> Result<ScheduleResult, AppError> {
    // ReschedulePrep re-reads config after the mutating steps below; this read
    // only feeds reconcile's horizon and timezone.
    let pre_config = store.get_config()?;
    let pre_horizon_end = now + Duration::days(pre_config.planning_horizon_days);
    let tz = pre_config.timezone_tz()?;

    // Repositions existing instances in place. Conflicts a moved deadline may
    // introduce are resolved by the scheduler pass below.
    let templates = store.list_templates()?;
    for template in &templates {
        reconcile(store, template, now, pre_horizon_end, &tz)?;
    }

    auto_cancel_overdue(store, now)?;

    release_stale_fixed_locks(store, now)?;

    let ReschedulePrep {
        mut config,
        horizon_end,
        fixed,
        free_slots,
        fixed_task_ids,
    } = ReschedulePrep::load(store, now)?;

    let tasks = store.get_schedulable_tasks()?;

    let old_auto = store.get_auto_chunks()?;

    let mut result = scheduler.schedule(ScheduleInput {
        tasks: tasks.clone(),
        existing_fixed_chunks: fixed,
        available_slots: free_slots,
        horizon_end,
        now,
        max_continuous_minutes: config.max_continuous_minutes,
        min_break_minutes: config.min_break_minutes,
    })?;
    retain_horizon_warnings(&mut result.warnings, &tasks, horizon_end);

    // Steps 9–11 (ARCHITECTURE.md §6) run in a single transaction so concurrent
    // readers see either the old or new state, never a partial update.
    let ops = diff_chunks(&old_auto, &result.placed_chunks);

    let tasks_with_auto_chunks: HashSet<String> = result
        .placed_chunks
        .iter()
        .map(|c| c.task_id.clone())
        .collect();

    config.last_reschedule = Some(now);
    config.last_mutation = Some(now);

    store.with_tx(&mut |tx| {
        apply_diff_ops(tx, &ops, now)?;

        for task in &tasks {
            let has_chunks =
                tasks_with_auto_chunks.contains(&task.id) || fixed_task_ids.contains(&task.id);
            sync_task_status(tx, &task.id, has_chunks, now)?;
        }

        tx.update_config(&config)
    })?;

    Ok(result)
}

fn chunks_to_occupied(chunks: &[Chunk]) -> Vec<OccupiedInterval> {
    chunks
        .iter()
        .map(|c| OccupiedInterval {
            start: c.start_time,
            end: c.end_time,
        })
        .collect()
}

/// Orchestrate an incremental reschedule for a set of initially-affected tasks.
///
/// Unlike [`reschedule`], this function skips recurring-instance generation,
/// overdue cancellation, and the `last_reschedule` timestamp update. It targets
/// only the cascade of tasks whose auto-chunks may be displaced by the changed
/// initial tasks.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the configured timezone is invalid.
/// Returns [`AppError::Database`] on any storage failure.
/// Returns [`AppError::Internal`] if the scheduling algorithm fails.
// The function implements a sequential multi-step pipeline whose steps share
// intermediate state (config, fixed chunks, slot pool, affected set). Splitting
// it into smaller functions would require threading this shared state through
// multiple function signatures, adding more complexity than the current flat
// layout. The length is intentional and well-commented.
#[allow(clippy::too_many_lines)]
pub fn reschedule_incremental(
    store: &dyn Store,
    scheduler: &dyn Scheduler,
    initial_task_ids: &[String],
    now: DateTime<Utc>,
) -> Result<ScheduleResult, AppError> {
    // Timezone parse is validation only — fail fast even when there is nothing
    // to do. ReschedulePrep below re-reads config after the duration fixes.
    store.get_config()?.timezone_tz()?;

    if initial_task_ids.is_empty() {
        return Ok(ScheduleResult {
            placed_chunks: Vec::new(),
            warnings: Vec::new(),
        });
    }

    release_stale_fixed_locks(store, now)?;

    for task_id in initial_task_ids {
        // task may have been deleted between trigger and execution — skip.
        let Some(task) = store.get_task(task_id)? else {
            continue;
        };
        let all_chunks = store.get_chunks_for_task(task_id)?;
        let fixed_duration: i64 = all_chunks
            .iter()
            .filter(|c| c.is_fixed)
            .map(|c| (c.end_time - c.start_time).num_minutes())
            .sum();
        let total_committed = task.time_logged_minutes + fixed_duration;
        if total_committed > task.duration_minutes {
            let mut updated = task;
            updated.duration_minutes = total_committed;
            updated.updated_at = now;
            store.update_task(&updated)?;
        }
    }

    let ReschedulePrep {
        mut config,
        horizon_end,
        fixed,
        free_slots: initial_free_slots,
        fixed_task_ids,
    } = ReschedulePrep::load(store, now)?;

    // Compute fixed chunk duration per task for the sort tiebreaker
    // (remaining = duration − logged − fixed, consistent with DefaultScheduler).
    let fixed_dur_by_task: HashMap<String, i64> = {
        let mut m: HashMap<String, i64> = HashMap::new();
        for c in &fixed {
            *m.entry(c.task_id.clone()).or_default() += (c.end_time - c.start_time).num_minutes();
        }
        m
    };

    // Remaining = duration − logged − fixed, floored at 0 — the same formula
    // DefaultScheduler uses, so both pipelines sort identically.
    let mut all_tasks = store.get_schedulable_tasks()?;
    let remaining_by_task: HashMap<String, i64> = all_tasks
        .iter()
        .map(|task| {
            let fixed_minutes = fixed_dur_by_task.get(&task.id).copied().unwrap_or(0);
            let remaining = task.duration_minutes - task.time_logged_minutes - fixed_minutes;
            (task.id.clone(), remaining.max(0))
        })
        .collect();
    all_tasks.sort_by(|a, b| {
        scheduling_order(a, b, |id| remaining_by_task.get(id).copied().unwrap_or(0))
    });

    let all_auto_chunks = store.get_auto_chunks()?;
    let mut existing_auto: HashMap<String, Vec<Chunk>> = HashMap::new();
    for chunk in all_auto_chunks {
        existing_auto
            .entry(chunk.task_id.clone())
            .or_default()
            .push(chunk);
    }

    let mut affected: HashSet<String> = initial_task_ids.iter().cloned().collect();
    // Snapshots: old auto-chunks before any changes (for diff in step 7).
    let mut old_auto_by_task: HashMap<String, Vec<Chunk>> = HashMap::new();
    let mut new_auto_by_task: HashMap<String, Vec<Chunk>> = HashMap::new();
    let mut processed: HashSet<String> = HashSet::new();
    let mut free_slots = initial_free_slots;
    let mut all_warnings: Vec<ScheduleWarning> = Vec::new();

    for task in &all_tasks {
        if affected.contains(&task.id) {
            old_auto_by_task
                .entry(task.id.clone())
                .or_insert_with(|| existing_auto.get(&task.id).cloned().unwrap_or_default());

            let result = scheduler.schedule(ScheduleInput {
                tasks: vec![task.clone()],
                existing_fixed_chunks: fixed.clone(),
                available_slots: free_slots.clone(),
                horizon_end,
                now,
                max_continuous_minutes: config.max_continuous_minutes,
                min_break_minutes: config.min_break_minutes,
            })?;

            all_warnings.extend(result.warnings);
            let placed_chunks = result.placed_chunks;

            // Check whether any of the newly placed chunks overlap existing
            // auto-chunks of tasks that have not yet been processed.
            for placed in &placed_chunks {
                for (other_id, other_chunks) in &existing_auto {
                    if processed.contains(other_id) || other_id == &task.id {
                        continue;
                    }
                    let overlaps = other_chunks.iter().any(|oc| {
                        placed.start_time < oc.end_time && oc.start_time < placed.end_time
                    });
                    if overlaps {
                        affected.insert(other_id.clone());
                    }
                }
            }

            free_slots = subtract_intervals(&free_slots, &chunks_to_occupied(&placed_chunks));
            new_auto_by_task.insert(task.id.clone(), placed_chunks);
        } else if let Some(chunks) = existing_auto.get(&task.id) {
            free_slots = subtract_intervals(&free_slots, &chunks_to_occupied(chunks));
        }

        processed.insert(task.id.clone());
    }

    // Steps 7–9 run in a single transaction so concurrent readers see
    // either the old or new state, never a partial update.
    let empty_chunks: Vec<Chunk> = Vec::new();
    let mut all_placed: Vec<Chunk> = Vec::new();
    for task_id in &affected {
        let new = new_auto_by_task.get(task_id).unwrap_or(&empty_chunks);
        all_placed.extend(new.iter().cloned());
    }

    let tasks_with_new_auto: HashSet<String> = new_auto_by_task
        .iter()
        .filter(|(_, chunks)| !chunks.is_empty())
        .map(|(id, _)| id.clone())
        .collect();

    config.last_mutation = Some(now);

    store.with_tx(&mut |tx| {
        for task_id in &affected {
            let old = old_auto_by_task.get(task_id).unwrap_or(&empty_chunks);
            let new = new_auto_by_task.get(task_id).unwrap_or(&empty_chunks);
            let ops = diff_chunks(old, new);
            apply_diff_ops(tx, &ops, now)?;
        }

        for task_id in &affected {
            let has_chunks =
                tasks_with_new_auto.contains(task_id) || fixed_task_ids.contains(task_id);
            sync_task_status(tx, task_id, has_chunks, now)?;
        }

        // Step 9: Update config (last_mutation — NOT last_reschedule)
        tx.update_config(&config)
    })?;

    retain_horizon_warnings(&mut all_warnings, &all_tasks, horizon_end);
    Ok(ScheduleResult {
        placed_chunks: all_placed,
        warnings: all_warnings,
    })
}

#[cfg(test)]
mod tests;
