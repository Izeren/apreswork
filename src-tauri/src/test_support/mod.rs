// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared test fixtures for service-layer tests.
//!
//! The canonical test [`Store`](crate::traits::storage::Store) is the **real**
//! [`SqliteStore`](crate::db::sqlite::SqliteStore) running against a `:memory:`
//! database ([`test_store`]). It enforces the same schema, foreign keys, and
//! query semantics as production, so a service test can never pass because of a
//! divergent hand-rolled double. There is intentionally no in-memory fake to
//! maintain or contract-test.
//!
//! Helpers live in concern-scoped submodules and are re-exported flat:
//! - [`store`] — real in-memory store construction + the standard test config.
//! - [`seed`] — FK-aware seeding through the real write API.
//! - [`time`] — deterministic `DateTime` constructors.
//!
//! There is no `FailingStore` here: the audit found zero tests that inject a
//! storage-layer error — every error path asserts the service's own
//! `Validation` / `NotFound` logic, and `NotFound` already arises naturally
//! when a row is simply absent. Add a thin `FailingStore<S>` decorator (wrap
//! the real store, delegate everything, return `Err` for one targeted method)
//! the first time a real consumer appears.

mod asserts;
pub(crate) mod calendar;
mod seed;
mod store;
mod time;

pub(crate) use asserts::{assert_not_found, assert_validation, assert_validation_contains};
pub(crate) use seed::{schedule_with_window, seed_chunk, seed_schedule, seed_task, seed_template};
pub(crate) use store::{
    default_config, default_schedule_id, make_scheduler_stack, test_store, test_store_with_config,
};
pub(crate) use time::{fixture_base, test_instant_now, test_now, utc};
