// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Hand-rolled migration runner for `SQLite`.
//!
//! A `schema_version` table tracks the current version. Each migration is a
//! plain Rust function compiled into the binary. [`run_migrations`] applies
//! pending migrations sequentially, wrapping each one in its own transaction.

use rusqlite::Connection;

use crate::error::AppError;

/// A migration is a function that receives an open connection (already inside
/// a transaction) and applies DDL/DML changes.
pub type Migration = fn(&Connection) -> Result<(), rusqlite::Error>;

/// Ordered list of all migrations. Index `n` corresponds to the migration that
/// brings the schema from version `n` to version `n + 1`.
///
/// The pre-release chain 001–008 was squashed into a single full-schema
/// `migration_001` before v0, so a fully migrated database sits at version 1.
/// Old-chain databases were flipped to version 1 by hand at the squash; their
/// schema is identical (verified structurally against the chain before it was
/// deleted). New migrations append here as version 2, 3, ….
pub const MIGRATIONS: &[Migration] = &[super::migration_001::migrate];

/// The schema version a fully migrated database ends up at.
///
/// Backup restore uses this as the ceiling: an archive whose `schema_version`
/// is above it was written by a newer binary and must be refused.
#[must_use]
#[allow(clippy::cast_possible_wrap)] // compile-time slice length, nowhere near i64::MAX
pub const fn current_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// Apply all pending migrations to `conn`.
///
/// 1. Creates the `schema_version` table if it does not exist (initial version 0).
/// 2. Reads the current version.
/// 3. Applies each migration whose index >= current version, in order.
/// 4. Each migration runs inside its own `BEGIN … COMMIT` transaction; on
///    failure the transaction is rolled back and the error is returned.
///
/// # Errors
///
/// Returns [`AppError::Database`] if any SQL operation or migration fails.
pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    run_migrations_with(conn, MIGRATIONS)
}

/// Internal entry point that accepts an explicit migration slice.
///
/// Separated from [`run_migrations`] so that tests can inject dummy migrations
/// without touching the production `MIGRATIONS` constant.
fn run_migrations_with(conn: &Connection, migrations: &[Migration]) -> Result<(), AppError> {
    // Wrap initial setup (CREATE TABLE + INSERT) in a transaction so the
    // schema_version table is never left in a half-initialized state.
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (\
             id INTEGER PRIMARY KEY CHECK(id = 1), \
             version INTEGER NOT NULL DEFAULT 0\
         );",
    )?;

    let count: i64 = tx.query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))?;
    if count == 0 {
        tx.execute("INSERT INTO schema_version (id, version) VALUES (1, 0)", [])?;
    }
    tx.commit()?;

    let current_version: i64 =
        conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;

    // Guard against corrupted negative version before casting to usize.
    if current_version < 0 {
        return Err(AppError::Database(
            "corrupted schema_version: negative version".into(),
        ));
    }

    // Precondition: version >= 0 enforced by the guard above; compile-time
    // migration count will never exceed usize::MAX.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let start = current_version as usize;

    for (i, migration) in migrations.iter().enumerate().skip(start) {
        let tx = conn.unchecked_transaction()?;
        migration(&tx)?;
        // Precondition: MIGRATIONS is a compile-time slice; its length is bounded
        // by binary size and will never approach i64::MAX.
        #[allow(clippy::cast_possible_wrap)]
        let new_version = (i + 1) as i64;
        tx.execute("UPDATE schema_version SET version = ?1", [new_version])?;
        tx.commit()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{run_migrations, run_migrations_with, Migration, MIGRATIONS};

    fn memory_db() -> Connection {
        Connection::open_in_memory().expect("open in-memory db")
    }

    fn schema_version(conn: &Connection) -> i64 {
        conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("read version")
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [name],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        count > 0
    }

    #[test]
    fn run_migrations_creates_schema_version_table() {
        let conn = memory_db();
        run_migrations(&conn).expect("run_migrations");

        assert!(table_exists(&conn, "schema_version"));
        let expected = i64::try_from(MIGRATIONS.len()).expect("migration count fits i64");
        assert_eq!(schema_version(&conn), expected);
    }

    #[test]
    fn skip_already_applied_migrations() {
        let conn = memory_db();
        run_migrations(&conn).expect("first run");
        let version_after_first = schema_version(&conn);

        run_migrations(&conn).expect("second run");
        assert_eq!(schema_version(&conn), version_after_first);
    }

    fn migration_create_fruits(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("CREATE TABLE fruits (id INTEGER PRIMARY KEY, name TEXT NOT NULL);")?;
        Ok(())
    }

    fn migration_add_color(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("ALTER TABLE fruits ADD COLUMN color TEXT;")?;
        Ok(())
    }

    #[test]
    fn sequential_ordering_applies_both() {
        let conn = memory_db();
        let migrations: &[Migration] = &[migration_create_fruits, migration_add_color];

        run_migrations_with(&conn, migrations).expect("run_migrations_with");

        assert_eq!(schema_version(&conn), 2);
        assert!(table_exists(&conn, "fruits"));

        // Verify the `color` column exists by inserting a row that uses it.
        conn.execute(
            "INSERT INTO fruits (name, color) VALUES ('apple', 'red')",
            [],
        )
        .expect("insert with color column");
    }

    #[test]
    fn run_twice_with_same_migrations_is_noop() {
        let conn = memory_db();
        let migrations: &[Migration] = &[migration_create_fruits, migration_add_color];

        run_migrations_with(&conn, migrations).expect("first run");
        assert_eq!(schema_version(&conn), 2);

        // Second run should be a no-op.
        run_migrations_with(&conn, migrations).expect("second run");
        assert_eq!(schema_version(&conn), 2);
    }

    fn migration_that_fails(conn: &Connection) -> Result<(), rusqlite::Error> {
        // Try to create a table that already exists — this would succeed with
        // IF NOT EXISTS, but without it SQLite raises an error.
        conn.execute_batch("CREATE TABLE fruits (x INTEGER);")?;
        Ok(())
    }

    #[test]
    fn migration_failure_rolls_back() {
        let conn = memory_db();

        let migrations: &[Migration] = &[migration_create_fruits, migration_that_fails];

        let result = run_migrations_with(&conn, migrations);
        assert!(result.is_err(), "second migration should fail");

        assert_eq!(schema_version(&conn), 1);
    }

    fn migration_add_weight(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch("ALTER TABLE fruits ADD COLUMN weight REAL;")?;
        Ok(())
    }

    #[test]
    fn resume_applies_only_pending_migrations() {
        let conn = memory_db();

        let first_batch: &[Migration] = &[migration_create_fruits];
        run_migrations_with(&conn, first_batch).expect("first batch");
        assert_eq!(schema_version(&conn), 1);

        let full_batch: &[Migration] = &[
            migration_create_fruits,
            migration_add_color,
            migration_add_weight,
        ];
        run_migrations_with(&conn, full_batch).expect("second batch");
        assert_eq!(schema_version(&conn), 3);

        conn.execute(
            "INSERT INTO fruits (name, color, weight) VALUES ('banana', 'yellow', 0.15)",
            [],
        )
        .expect("insert with color and weight columns");
    }

    #[test]
    fn negative_version_returns_error() {
        let conn = memory_db();

        conn.execute_batch(
            "CREATE TABLE schema_version (\
                 id INTEGER PRIMARY KEY CHECK(id = 1), \
                 version INTEGER NOT NULL DEFAULT 0\
             );",
        )
        .expect("create table");
        conn.execute(
            "INSERT INTO schema_version (id, version) VALUES (1, -1)",
            [],
        )
        .expect("insert negative version");

        let migrations: &[Migration] = &[migration_create_fruits];
        let result = run_migrations_with(&conn, migrations);
        assert!(result.is_err(), "should reject negative version");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("negative version"),
            "expected 'negative version' in: {err_msg}"
        );
    }

    #[test]
    fn schema_version_table_rejects_second_row() {
        let conn = memory_db();
        run_migrations(&conn).expect("run_migrations");

        let result = conn.execute("INSERT INTO schema_version (id, version) VALUES (2, 0)", []);
        assert!(
            result.is_err(),
            "second row should violate CHECK constraint"
        );
    }
}
