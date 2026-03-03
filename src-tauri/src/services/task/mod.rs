// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Task service — stateless functions implementing task business logic.
//!
//! The implementation is split by concern across submodules; the public API is
//! re-exported here so callers keep using `crate::services::task::*`.

use chrono::{DateTime, Utc};

use crate::domain::models::Task;
use crate::error::AppError;
use crate::traits::storage::Store;

mod agenda;
mod chunks;
mod crud;
mod lifecycle;

#[cfg(test)]
mod test_helpers;

pub use agenda::get_agenda;
pub(crate) use chunks::sync_task_pinned;
pub use chunks::{
    create_fixed_chunk, delete_fixed_chunk, lock_chunk, move_chunk, require_chunk, resize_chunk,
    unlock_chunk,
};
pub use crud::{create_task, delete_task, get_task, list_labels, list_tasks, update_task};
pub use lifecycle::{cancel_task, complete_chunk, complete_task, reopen_chunk};

/// Stamp `task.updated_at`, delete the given chunks, and persist the task
/// inside a single transaction. Shared by `update_task` and `cancel_task`.
fn stamp_and_persist(
    store: &dyn Store,
    task: &mut Task,
    chunks_to_delete: &[String],
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    task.updated_at = now;
    store.with_tx(&mut |tx| {
        for chunk_id in chunks_to_delete {
            tx.delete_chunk(chunk_id)?;
        }
        tx.update_task(task)
    })
}
