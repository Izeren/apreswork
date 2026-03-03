// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! SQLite-backed implementation of all storage traits.
//!
//! [`SqliteStore`] wraps a `Mutex<Connection>` and serializes all access
//! through it. Entity-specific mappers and query helpers live in the sibling
//! modules ([`task`], [`chunk`], [`schedule`], [`template`], [`comment`],
//! [`config_busy`], [`sync_state`]);
//! each helper takes a plain `&Connection` so the same code path serves both
//! the mutex-guarded [`SqliteStore`] methods and the transaction-scoped
//! [`TxStore`] view handed to [`Store::with_tx`] closures.

mod chunk;
mod comment;
mod config_busy;
mod schedule;
mod sync_state;
mod task;
mod template;

#[cfg(test)]
mod tests;

use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::domain::enums::Priority;
use crate::error::AppError;
use crate::traits::storage::Store;

/// SQLite-backed storage backend.
///
/// All access is serialized through a [`Mutex`]. The connection is configured
/// with WAL mode and foreign keys enabled at construction time.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (or create) a `SQLite` database at `path`, enable WAL mode and
    /// foreign keys, and run pending migrations.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] if the connection cannot be opened,
    /// pragmas fail, or migrations fail.
    pub fn new(path: &str) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        crate::db::migrations::run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create an in-memory store for testing.
    ///
    /// # Panics
    ///
    /// Panics if the in-memory connection cannot be opened, foreign keys
    /// cannot be enabled, or migrations fail.
    #[cfg(test)]
    #[must_use]
    pub fn new_in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .expect("enable foreign keys");
        crate::db::migrations::run_migrations(&conn).expect("run migrations");
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Lock the connection mutex, mapping poisoning to [`AppError::Internal`].
    fn lock(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        self.conn
            .lock()
            .map_err(|e| AppError::Internal(format!("mutex poisoned: {e}")))
    }

    /// Execute `f` against the raw connection for test-only seeding and row
    /// counting. Only available in `#[cfg(test)]` builds; never called from
    /// production paths.
    ///
    /// # Panics
    ///
    /// Panics if the connection mutex is poisoned.
    #[cfg(test)]
    pub fn with_conn_for_test<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        let conn = self.conn.lock().expect("lock");
        f(&conn)
    }

    /// Run `f` on the locked connection inside a fresh transaction.
    ///
    /// Commits on `Ok`; on `Err` the transaction is dropped un-committed,
    /// which rolls it back. This is the single per-method transaction wrapper
    /// for multi-statement [`SqliteStore`] mutations — [`TxStore`] methods
    /// never call it because they already run inside a [`Store::with_tx`]
    /// transaction.
    fn in_tx(&self, f: impl FnOnce(&Connection) -> Result<(), AppError>) -> Result<(), AppError> {
        let conn = self.lock()?;
        let tx = conn.unchecked_transaction()?;
        f(&conn)?;
        tx.commit()?;
        Ok(())
    }
}

/// Transaction-scoped storage view handed to [`Store::with_tx`] closures.
///
/// Borrows the already-locked connection, so every trait method executes its
/// statements inside the transaction open on that connection without touching
/// the [`SqliteStore`] mutex (which the enclosing `with_tx` call still holds).
struct TxStore<'a> {
    conn: &'a Connection,
}

impl Store for SqliteStore {
    fn with_tx(
        &self,
        f: &mut dyn FnMut(&dyn Store) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let conn = self.lock()?;
        let tx = conn.unchecked_transaction()?;
        f(&TxStore { conn: &conn })?;
        tx.commit()?;
        Ok(())
    }

    fn vacuum_into(&self, dest: &std::path::Path) -> Result<(), AppError> {
        let dest_str = dest.to_str().ok_or_else(|| {
            AppError::Database("snapshot destination path contains invalid UTF-8".into())
        })?;
        let conn = self.lock()?;
        // VACUUM INTO writes a compact, consistent snapshot even while the
        // source runs in WAL mode; it fails if `dest` already exists.
        conn.execute("VACUUM INTO ?1", [dest_str])?;
        Ok(())
    }
}

impl Store for TxStore<'_> {
    fn with_tx(
        &self,
        f: &mut dyn FnMut(&dyn Store) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        // Already inside a transaction — a nested call joins it.
        f(self)
    }
}

// ── Shared conversion helpers (used across entity modules) ──────────────

/// Parse a required RFC 3339 datetime column, labelling errors with the field name.
fn parse_datetime(value: &str, field: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AppError::Database(format!("invalid {field}: {e}")))
}

/// Parse a config value as `Option<DateTime<Utc>>`. Empty string maps to `None`.
fn parse_optional_datetime(value: &str) -> Result<Option<DateTime<Utc>>, AppError> {
    if value.is_empty() {
        return Ok(None);
    }
    DateTime::parse_from_rfc3339(value)
        .map(|dt| Some(dt.with_timezone(&Utc)))
        .map_err(|e| AppError::Database(format!("invalid datetime in config: {e}")))
}

/// Serialize an `Option<DateTime<Utc>>` to a config value string.
fn format_optional_datetime(dt: Option<&DateTime<Utc>>) -> String {
    match dt {
        Some(d) => d.to_rfc3339(),
        None => String::new(),
    }
}

/// Convert a [`Priority`] variant to its integer representation.
const fn priority_to_i64(p: Priority) -> i64 {
    match p {
        Priority::Low => 0,
        Priority::Medium => 1,
        Priority::High => 2,
        Priority::Critical => 3,
    }
}

/// Convert an integer back to a [`Priority`], returning an error for unknown values.
fn priority_from_i64(v: i64) -> Result<Priority, AppError> {
    match v {
        0 => Ok(Priority::Low),
        1 => Ok(Priority::Medium),
        2 => Ok(Priority::High),
        3 => Ok(Priority::Critical),
        other => Err(AppError::Database(format!(
            "unknown priority value: {other}"
        ))),
    }
}
