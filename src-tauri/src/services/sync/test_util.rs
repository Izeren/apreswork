// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared test helpers for the `sync` service test modules (`tests`,
//! `sync_cycle_tests`, `sync_now_tests`, `user_event_tests`).

use std::sync::Arc;

use crate::db::sqlite::SqliteStore;
use crate::scheduler::engine::DefaultScheduler;
use crate::services::trigger::{DefaultExecutor, RescheduleTrigger};

/// Build a [`RescheduleTrigger`] over the default scheduler and executor.
pub(super) fn make_trigger(store: &Arc<SqliteStore>) -> RescheduleTrigger {
    let scheduler = Arc::new(DefaultScheduler);
    let executor = Arc::new(DefaultExecutor::new(scheduler));
    RescheduleTrigger::new(store.clone(), executor)
}

/// Count rows in `table`. `table` is always a fixed literal from the tests,
/// never runtime data — SQL identifiers cannot be bind parameters.
pub(super) fn count_rows(store: &SqliteStore, table: &str) -> i64 {
    store.with_conn_for_test(|conn| {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |row: &rusqlite::Row<'_>| row.get(0),
        )
        .expect("count")
    })
}
