// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Chunk placement — creating fixed chunks, moving, resizing, and (un)pinning.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::enums::{ChunkStatus, TaskStatus};
use crate::domain::models::{Chunk, Task};
use crate::error::AppError;
use crate::traits::storage::Store;

use super::get_task;

/// Fetch a chunk by ID or fail with [`AppError::NotFound`].
///
/// The chunk counterpart of [`get_task`] — the single definition of the
/// "load this chunk or 404" step shared by every chunk mutation.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if no chunk with the given ID exists.
/// Returns [`AppError::Database`] on storage failure.
pub fn require_chunk(store: &dyn Store, chunk_id: &str) -> Result<Chunk, AppError> {
    store
        .get_chunk(chunk_id)?
        .ok_or_else(|| AppError::NotFound {
            entity: "Chunk".into(),
            id: chunk_id.to_owned(),
        })
}

fn validate_time_range(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    msg: &'static str,
) -> Result<(), AppError> {
    if start >= end {
        Err(AppError::Validation(msg.into()))
    } else {
        Ok(())
    }
}

/// Create a fixed (manually-placed) chunk for a task.
///
/// Fixed chunks are not moved by the auto-scheduler. They count towards the
/// task's allocated time when checking for over-allocation, but only other
/// fixed chunks are included in the allocation sum (auto-scheduled chunks
/// are recomputed each reschedule and thus excluded).
///
/// # Status transition
///
/// If the task is in `Pending` status, it is automatically transitioned to
/// `Scheduled` when the first chunk is created.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the task does not exist.
/// Returns [`AppError::Validation`] if:
/// - the task is `Completed` or `Cancelled`
/// - `start_time >= end_time`
/// - the chunk duration would exceed the remaining allocatable time
pub fn create_fixed_chunk(
    store: &dyn Store,
    task_id: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(Chunk, Task), AppError> {
    validate_time_range(start_time, end_time, "start_time must be before end_time")?;

    let mut task = get_task(store, task_id)?;

    if matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled) {
        return Err(AppError::Validation(format!(
            "cannot create chunk for task with status {:?}",
            task.status
        )));
    }

    let existing_chunks = store.get_chunks_for_task(task_id)?;
    let allocated: i64 = existing_chunks
        .iter()
        .filter(|c| c.is_fixed)
        .map(|c| (c.end_time - c.start_time).num_minutes())
        .sum();

    let remaining = task.duration_minutes - task.time_logged_minutes - allocated;
    let chunk_duration = (end_time - start_time).num_minutes();

    if chunk_duration > remaining {
        return Err(AppError::Validation(format!(
            "chunk duration ({chunk_duration} min) exceeds remaining allocatable time ({remaining} min)"
        )));
    }

    let chunk = Chunk {
        id: Uuid::now_v7().to_string(),
        task_id: task_id.to_owned(),
        start_time,
        end_time,
        status: ChunkStatus::Scheduled,
        is_fixed: true,
        logged_minutes: None,
        completed_at: None,
        google_event_id: None,
        created_at: now,
        updated_at: now,
    };

    let task_dirty = task.status == TaskStatus::Pending;
    if task_dirty {
        task.status = TaskStatus::Scheduled;
        task.updated_at = now;
    }

    store.with_tx(&mut |tx| {
        tx.create_chunk(&chunk)?;
        if task_dirty {
            tx.update_task(&task)?;
        }
        Ok(())
    })?;

    // TODO(1.8.1): set config.last_mutation

    Ok((chunk, task))
}

/// Recompute and persist a task's `is_pinned` flag from its chunks.
///
/// Invariant: `is_pinned` ⇔ the task has at least one fixed chunk. Call after
/// any change to a chunk's fixed status. Skips the write when already in sync.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the task does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub(crate) fn sync_task_pinned(
    store: &dyn Store,
    task_id: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let pinned = store
        .get_chunks_for_task(task_id)?
        .iter()
        .any(|c| c.is_fixed);
    let mut task = get_task(store, task_id)?;
    if task.is_pinned != pinned {
        task.is_pinned = pinned;
        task.updated_at = now;
        store.update_task(&task)?;
    }
    Ok(())
}

/// Move a chunk to a new time range, marking it as fixed.
///
/// No overlap validation is performed — fixed chunks can overlap and will be
/// displaced on the next reschedule.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `new_start >= new_end`.
/// Returns [`AppError::NotFound`] if the chunk does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn move_chunk(
    store: &dyn Store,
    chunk_id: &str,
    new_start: DateTime<Utc>,
    new_end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<Chunk, AppError> {
    validate_time_range(new_start, new_end, "new_start must be before new_end")?;

    let mut chunk = require_chunk(store, chunk_id)?;

    chunk.start_time = new_start;
    chunk.end_time = new_end;
    chunk.is_fixed = true;
    chunk.updated_at = now;

    store.with_tx(&mut |tx| {
        tx.update_chunk(&chunk)?;
        sync_task_pinned(tx, &chunk.task_id, now)
    })?;

    // TODO(1.8.1): set config.last_mutation

    Ok(chunk)
}

/// Resize a chunk by changing its end time, marking it as fixed.
///
/// If the chunk is completed, the task's `time_logged_minutes` is adjusted by
/// the delta between the new and old logged durations. The duration invariant
/// is **eventually consistent** — the incremental reschedule fixes it within
/// seconds.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the chunk does not exist.
/// Returns [`AppError::Validation`] if `new_end <= chunk.start_time`.
/// Returns [`AppError::NotFound`] if the associated task does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn resize_chunk(
    store: &dyn Store,
    chunk_id: &str,
    new_end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(Chunk, Task), AppError> {
    let mut chunk = require_chunk(store, chunk_id)?;

    if chunk.start_time >= new_end {
        return Err(AppError::Validation(
            "new_end must be after chunk start_time".into(),
        ));
    }

    chunk.end_time = new_end;
    chunk.is_fixed = true;

    let mut task = get_task(store, &chunk.task_id)?;
    let mut task_dirty = false;

    if chunk.status == ChunkStatus::Completed {
        let new_duration = (new_end - chunk.start_time).num_minutes();
        let old_logged = chunk.logged_minutes.unwrap_or(0);
        let delta = new_duration - old_logged;
        task.time_logged_minutes += delta;
        chunk.logged_minutes = Some(new_duration);
        task_dirty = true;
    }

    if task_dirty {
        task.updated_at = now;
    }
    chunk.updated_at = now;

    store.with_tx(&mut |tx| {
        // Order is load-bearing: update_chunk first so sync_task_pinned sees is_fixed=true;
        // update_task before sync_task_pinned so the completed-path time_logged delta
        // is not silently overwritten by sync_task_pinned's own task write.
        tx.update_chunk(&chunk)?;
        if task_dirty {
            tx.update_task(&task)?;
        }
        sync_task_pinned(tx, &chunk.task_id, now)
    })?;

    // TODO(1.8.1): set config.last_mutation

    let task = get_task(store, &chunk.task_id)?;
    Ok((chunk, task))
}

/// Lock a chunk in place, marking it as fixed without changing its times.
///
/// The honest counterpart of [`unlock_chunk`] — pins the chunk where the
/// scheduler put it instead of round-tripping a `move_chunk` to its own
/// times. Only scheduled chunks can be locked. Completed chunks are
/// immutable with respect to their fixed status.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the chunk does not exist.
/// Returns [`AppError::Validation`] if the chunk is completed.
/// Returns [`AppError::Database`] on storage failure.
pub fn lock_chunk(
    store: &dyn Store,
    chunk_id: &str,
    now: DateTime<Utc>,
) -> Result<Chunk, AppError> {
    set_chunk_fixed(store, chunk_id, true, "locked", now)
}

/// Unlock a fixed chunk, allowing it to be moved by the auto-scheduler.
///
/// Only scheduled chunks can be unlocked. Completed chunks are immutable
/// with respect to their fixed status.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the chunk does not exist.
/// Returns [`AppError::Validation`] if the chunk is completed.
/// Returns [`AppError::Database`] on storage failure.
pub fn unlock_chunk(
    store: &dyn Store,
    chunk_id: &str,
    now: DateTime<Utc>,
) -> Result<Chunk, AppError> {
    set_chunk_fixed(store, chunk_id, false, "unlocked", now)
}

/// Set a chunk's `is_fixed` flag and resync the owning task's derived pinned
/// status. Shared by [`lock_chunk`] (`is_fixed = true`) and [`unlock_chunk`]
/// (`is_fixed = false`); `verb` names the rejected action in the
/// completed-chunk validation message.
fn set_chunk_fixed(
    store: &dyn Store,
    chunk_id: &str,
    is_fixed: bool,
    verb: &str,
    now: DateTime<Utc>,
) -> Result<Chunk, AppError> {
    let mut chunk = require_chunk(store, chunk_id)?;

    if chunk.status == ChunkStatus::Completed {
        return Err(AppError::Validation(format!(
            "completed chunks cannot be {verb}"
        )));
    }

    chunk.is_fixed = is_fixed;
    chunk.updated_at = now;

    store.with_tx(&mut |tx| {
        tx.update_chunk(&chunk)?;
        sync_task_pinned(tx, &chunk.task_id, now)
    })?;

    Ok(chunk)
}

/// Delete a fixed (manually-placed) chunk.
///
/// Only fixed chunks can be deleted directly — auto-scheduled chunks are
/// owned by the scheduler and would simply be re-placed on the next
/// reschedule. Completed chunks are immutable history (their logged minutes
/// are folded into the task); unlock or reopen them instead.
///
/// Returns the deleted chunk so callers can name its task in the reschedule
/// trigger. No task status transition happens here — the incremental
/// reschedule reconciles the task's remaining time.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the chunk does not exist.
/// Returns [`AppError::Validation`] if the chunk is not fixed or is completed.
/// Returns [`AppError::Database`] on storage failure.
pub fn delete_fixed_chunk(
    store: &dyn Store,
    chunk_id: &str,
    now: DateTime<Utc>,
) -> Result<Chunk, AppError> {
    let chunk = require_chunk(store, chunk_id)?;

    if !chunk.is_fixed {
        return Err(AppError::Validation(
            "only fixed chunks can be deleted; the scheduler owns auto-placed chunks".into(),
        ));
    }

    if chunk.status == ChunkStatus::Completed {
        return Err(AppError::Validation(
            "completed chunks cannot be deleted".into(),
        ));
    }

    store.with_tx(&mut |tx| {
        tx.delete_chunk(chunk_id)?;
        sync_task_pinned(tx, &chunk.task_id, now)
    })?;

    Ok(chunk)
}

#[cfg(test)]
mod tests;
