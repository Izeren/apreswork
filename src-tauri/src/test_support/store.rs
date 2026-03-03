// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Real in-memory [`SqliteStore`] construction plus associated test-fixture
//! helpers used by service- and command-layer tests.
//!
//! [`test_store`] opens a fresh `:memory:` database with migrations already
//! applied, so the default schedule and config row exist and foreign keys are
//! enforced — every query behaves like production.

use std::sync::Arc;

use crate::db::sqlite::SqliteStore;
use crate::domain::models::AppConfig;
use crate::scheduler::engine::DefaultScheduler;
use crate::services::trigger::{DefaultExecutor, RescheduleTrigger};
use crate::traits::storage::{ConfigStore, ScheduleStore};

#[must_use]
pub(crate) fn test_store() -> SqliteStore {
    SqliteStore::new_in_memory()
}

/// A [`test_store`] with the config row replaced by `config`.
// Owned `AppConfig` keeps call sites ergonomic: `test_store_with_config(default_config())`.
#[allow(clippy::needless_pass_by_value)]
#[must_use]
pub(crate) fn test_store_with_config(config: AppConfig) -> SqliteStore {
    let store = test_store();
    store
        .update_config(&config)
        .expect("seed config into test store");
    store
}

#[must_use]
pub(crate) fn default_config() -> AppConfig {
    AppConfig {
        planning_horizon_days: 30,
        timezone: "UTC".to_owned(),
        max_continuous_minutes: 120,
        min_break_minutes: 5,
        last_reschedule: None,
        last_mutation: None,
        last_sync: None,
        last_busy_sync: None,
    }
}

/// Build the standard `(scheduler, trigger)` triple on top of `store`.
///
/// Shared by `backup_commands` and `state` test fixtures to avoid repeating
/// the same three-line composition chain.
#[must_use]
pub(crate) fn make_scheduler_stack(
    store: Arc<SqliteStore>,
) -> (Arc<DefaultScheduler>, Arc<RescheduleTrigger>) {
    let scheduler = Arc::new(DefaultScheduler);
    let executor = Arc::new(DefaultExecutor::new(scheduler.clone()));
    let trigger = Arc::new(RescheduleTrigger::new(store, executor));
    (scheduler, trigger)
}

/// The id of the migration-seeded default schedule (a fresh UUID per store).
#[must_use]
pub(crate) fn default_schedule_id(store: &SqliteStore) -> String {
    store
        .get_default_schedule()
        .expect("default schedule exists")
        .id
}

#[cfg(test)]
mod tests {
    use super::{default_config, default_schedule_id, test_store, test_store_with_config};
    use crate::domain::models::AppConfig;
    use crate::traits::storage::{ConfigStore, ScheduleStore};

    #[test]
    fn test_store_has_seeded_default_schedule() {
        let store = test_store();
        let id = default_schedule_id(&store);
        assert!(!id.is_empty());
        assert!(store.get_schedule(&id).expect("get").unwrap().is_default);
    }

    #[test]
    fn test_store_with_config_overrides_config() {
        let config = AppConfig {
            timezone: "Europe/Berlin".to_owned(),
            ..default_config()
        };
        let store = test_store_with_config(config);
        assert_eq!(
            store.get_config().expect("config").timezone,
            "Europe/Berlin"
        );
    }
}
