// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Migration 001: full schema.
//!
//! The pre-release chain 001–008 was squashed into this single migration
//! before v0: it creates the final schema and seeds the default config and
//! schedule (weekday 07:00–09:00 + 18:00–23:00, weekend 08:00–22:00) in one
//! step. Databases migrated by the old chain were set to version 1 by hand
//! when the squash landed.
//!
//! `external_busy_times` is dead (superseded by the `external_events`
//! mirror) but kept for schema parity with pre-squash databases; dropping
//! it rides a future cleanup migration.

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

/// Apply the full-schema migration.
///
/// Creates every table and index of the final schema — `config`, `schedules`,
/// `schedule_windows`, `recurring_templates`, `tasks`, `task_labels`,
/// `template_labels`, `chunks`, `external_busy_times`, `google_auth`,
/// `external_events`, `chunk_sync_state`, `comments` — and seeds the default
/// config and schedule.
///
/// # Errors
///
/// Returns `rusqlite::Error` if any DDL or DML statement fails.
pub fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    create_config_and_schedule_tables(conn)?;
    create_task_tables(conn)?;
    create_chunk_and_auxiliary_tables(conn)?;
    create_sync_tables(conn)?;
    create_comments_table(conn)?;
    seed_config(conn)?;
    seed_default_schedule(conn)?;
    Ok(())
}

fn create_config_and_schedule_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "\
        CREATE TABLE config (\
            key TEXT PRIMARY KEY, \
            value TEXT NOT NULL\
        );\
        \
        CREATE TABLE schedules (\
            id TEXT PRIMARY KEY, \
            name TEXT NOT NULL UNIQUE, \
            is_default INTEGER NOT NULL DEFAULT 0, \
            created_at TEXT NOT NULL, \
            updated_at TEXT NOT NULL\
        );\
        \
        CREATE TABLE schedule_windows (\
            id TEXT PRIMARY KEY, \
            schedule_id TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE, \
            day_of_week INTEGER NOT NULL, \
            start_time TEXT NOT NULL, \
            end_time TEXT NOT NULL\
        );\
        CREATE INDEX idx_schedule_windows_schedule ON schedule_windows(schedule_id);\
        \
        CREATE TABLE recurring_templates (\
            id TEXT PRIMARY KEY, \
            title TEXT NOT NULL, \
            description TEXT, \
            duration_minutes INTEGER NOT NULL, \
            priority INTEGER NOT NULL DEFAULT 1, \
            schedule_id TEXT NOT NULL REFERENCES schedules(id), \
            cadence_type TEXT NOT NULL, \
            cadence_data TEXT NOT NULL, \
            is_active INTEGER NOT NULL DEFAULT 1, \
            created_at TEXT NOT NULL, \
            updated_at TEXT NOT NULL, \
            start_date TEXT\
        );\
        ",
    )
}

fn create_task_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "\
        CREATE TABLE tasks (\
            id TEXT PRIMARY KEY, \
            title TEXT NOT NULL, \
            description TEXT, \
            duration_minutes INTEGER NOT NULL, \
            time_logged_minutes INTEGER NOT NULL DEFAULT 0, \
            priority INTEGER NOT NULL DEFAULT 1, \
            status TEXT NOT NULL DEFAULT 'pending', \
            start_date TEXT, \
            deadline TEXT, \
            schedule_id TEXT NOT NULL REFERENCES schedules(id), \
            min_chunk_minutes INTEGER NOT NULL DEFAULT 30, \
            no_split INTEGER NOT NULL DEFAULT 0, \
            recurring_template_id TEXT REFERENCES recurring_templates(id) ON DELETE SET NULL, \
            created_at TEXT NOT NULL, \
            updated_at TEXT NOT NULL, \
            expire_at TEXT, \
            is_pinned INTEGER NOT NULL DEFAULT 0\
        );\
        CREATE INDEX idx_tasks_status ON tasks(status);\
        CREATE INDEX idx_tasks_deadline ON tasks(deadline);\
        CREATE INDEX idx_tasks_priority ON tasks(priority);\
        CREATE INDEX idx_tasks_recurring ON tasks(recurring_template_id);\
        CREATE INDEX idx_tasks_schedule ON tasks(schedule_id);\
        \
        CREATE TABLE task_labels (\
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE, \
            label TEXT NOT NULL, \
            PRIMARY KEY (task_id, label)\
        );\
        CREATE INDEX idx_task_labels_label ON task_labels(label);\
        \
        CREATE TABLE template_labels (\
            template_id TEXT NOT NULL REFERENCES recurring_templates(id) ON DELETE CASCADE, \
            label TEXT NOT NULL, \
            PRIMARY KEY (template_id, label)\
        );\
        ",
    )
}

fn create_chunk_and_auxiliary_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "\
        CREATE TABLE chunks (\
            id TEXT PRIMARY KEY, \
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE, \
            start_time TEXT NOT NULL, \
            end_time TEXT NOT NULL, \
            status TEXT NOT NULL DEFAULT 'scheduled', \
            is_fixed INTEGER NOT NULL DEFAULT 0, \
            logged_minutes INTEGER, \
            completed_at TEXT, \
            google_event_id TEXT, \
            created_at TEXT NOT NULL, \
            updated_at TEXT NOT NULL\
        );\
        CREATE INDEX idx_chunks_task ON chunks(task_id);\
        CREATE INDEX idx_chunks_time ON chunks(start_time, end_time);\
        CREATE INDEX idx_chunks_status ON chunks(status);\
        CREATE INDEX idx_chunks_google ON chunks(google_event_id);\
        \
        CREATE TABLE external_busy_times (\
            id TEXT PRIMARY KEY, \
            start_time TEXT NOT NULL, \
            end_time TEXT NOT NULL, \
            source TEXT NOT NULL DEFAULT 'google'\
        );\
        CREATE INDEX idx_busy_times_range ON external_busy_times(start_time, end_time);\
        \
        CREATE TABLE google_auth (\
            id INTEGER PRIMARY KEY CHECK (id = 1), \
            calendar_id TEXT, \
            connected_at TEXT\
        );\
        ",
    )
}

/// `external_events` uniqueness is per `(calendar_id, event_id)` — the same
/// provider event id may legitimately appear on several calendars.
fn create_sync_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE external_events (\
            id          TEXT PRIMARY KEY,\
            calendar_id TEXT NOT NULL,\
            event_id    TEXT NOT NULL,\
            title       TEXT NOT NULL,\
            description TEXT,\
            start_utc   TEXT NOT NULL,\
            end_utc     TEXT NOT NULL,\
            busy        INTEGER NOT NULL DEFAULT 1,\
            declined    INTEGER NOT NULL DEFAULT 0,\
            updated_at  TEXT NOT NULL,\
            all_day     INTEGER NOT NULL DEFAULT 0,\
            UNIQUE(calendar_id, event_id)\
        );\
        CREATE INDEX idx_external_events_range ON external_events(start_utc, end_utc);\
        \
        CREATE TABLE chunk_sync_state (\
            chunk_id            TEXT PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,\
            event_id            TEXT NOT NULL,\
            etag                TEXT,\
            synced_start        TEXT NOT NULL,\
            synced_end          TEXT NOT NULL,\
            synced_title        TEXT NOT NULL,\
            synced_description  TEXT NOT NULL DEFAULT '',\
            updated_at          TEXT NOT NULL\
        );\
        CREATE INDEX idx_chunk_sync_event ON chunk_sync_state(event_id);",
    )
}

fn create_comments_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE comments (\
            id         TEXT PRIMARY KEY,\
            task_id    TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,\
            author     TEXT NOT NULL,\
            content    TEXT NOT NULL,\
            created_at TEXT NOT NULL,\
            updated_at TEXT NOT NULL\
        );\
        CREATE INDEX idx_comments_task ON comments(task_id, created_at);",
    )
}

fn seed_config(conn: &Connection) -> Result<(), rusqlite::Error> {
    let defaults: &[(&str, &str)] = &[
        ("planning_horizon_days", "30"),
        ("timezone", "UTC"),
        ("max_continuous_minutes", "120"),
        ("min_break_minutes", "5"),
        ("last_reschedule", ""),
        ("last_mutation", ""),
        ("last_sync", ""),
        ("last_busy_sync", ""),
        ("sync_provider", "google"),
        ("sync_debounce_minutes", "2"),
        ("sync_poll_minutes", "60"),
        ("last_sync_error", ""),
        ("pull_calendar_ids", ""),
    ];

    for (key, value) in defaults {
        conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
    }

    Ok(())
}

fn seed_default_schedule(conn: &Connection) -> Result<(), rusqlite::Error> {
    let schedule_id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO schedules (id, name, is_default, created_at, updated_at) \
         VALUES (?1, ?2, 1, ?3, ?4)",
        [&schedule_id, &String::from("Default"), &now, &now],
    )?;

    // Weekday windows: Monday(0)–Friday(4) get two windows each.
    for day in 0..5 {
        insert_window(conn, &schedule_id, day, "07:00", "09:00")?;
        insert_window(conn, &schedule_id, day, "18:00", "23:00")?;
    }

    // Weekend windows: Saturday(5) and Sunday(6) get one window each.
    for day in 5..7 {
        insert_window(conn, &schedule_id, day, "08:00", "22:00")?;
    }

    Ok(())
}

fn insert_window(
    conn: &Connection,
    schedule_id: &str,
    day_of_week: i32,
    start_time: &str,
    end_time: &str,
) -> Result<(), rusqlite::Error> {
    let window_id = Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO schedule_windows (id, schedule_id, day_of_week, start_time, end_time) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![window_id, schedule_id, day_of_week, start_time, end_time],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use test_case::test_case;

    use crate::db::migrations::{current_version, run_migrations};

    fn migrated_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        run_migrations(&conn).expect("run_migrations");
        conn
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).expect("count")
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        // SQLite PRAGMA does not support bind parameters; table name is test-controlled.
        let sql = format!("PRAGMA table_info({table})");
        let mut stmt = conn.prepare(&sql).expect("prepare pragma");
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        names.iter().any(|n| n == column)
    }

    #[test]
    fn migration_creates_all_tables() {
        let conn = migrated_db();
        let expected = [
            "config",
            "schedules",
            "schedule_windows",
            "recurring_templates",
            "tasks",
            "task_labels",
            "template_labels",
            "chunks",
            "external_busy_times",
            "google_auth",
            "external_events",
            "chunk_sync_state",
            "comments",
        ];
        for table in expected {
            let sql = format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
            );
            assert_eq!(count(&conn, &sql), 1, "table '{table}' should exist");
        }
    }

    #[test]
    fn migration_creates_all_indexes() {
        let conn = migrated_db();
        let expected = [
            "idx_schedule_windows_schedule",
            "idx_tasks_status",
            "idx_tasks_deadline",
            "idx_tasks_priority",
            "idx_tasks_recurring",
            "idx_tasks_schedule",
            "idx_task_labels_label",
            "idx_chunks_task",
            "idx_chunks_time",
            "idx_chunks_status",
            "idx_chunks_google",
            "idx_busy_times_range",
            "idx_external_events_range",
            "idx_chunk_sync_event",
            "idx_comments_task",
        ];
        for index in expected {
            let sql = format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
            );
            assert_eq!(count(&conn, &sql), 1, "index '{index}' should exist");
        }
    }

    // Spot-check columns the old chain added late (squash regression guards).
    #[test_case("recurring_templates", "start_date")]
    #[test_case("tasks", "expire_at")]
    #[test_case("tasks", "is_pinned")]
    #[test_case("google_auth", "connected_at")]
    #[test_case("external_events", "all_day")]
    fn late_chain_column_present(table: &str, column: &str) {
        let conn = migrated_db();
        assert!(column_exists(&conn, table, column));
    }

    /// The old chain dropped `cadence_period_key` and its unique index; the
    /// squashed schema must not resurrect them (the unique index forbade two
    /// recurring instances on one calendar day).
    #[test]
    fn cadence_period_key_absent() {
        let conn = migrated_db();
        assert!(!column_exists(&conn, "tasks", "cadence_period_key"));
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type = 'index' AND name = 'idx_tasks_cadence_period'"
            ),
            0
        );
    }

    #[test]
    fn migration_seeds_config() {
        let conn = migrated_db();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM config"), 13);

        let read = |key: &str| -> String {
            conn.query_row("SELECT value FROM config WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .expect("read config value")
        };
        assert_eq!(read("planning_horizon_days"), "30");
        assert_eq!(read("sync_provider"), "google");
    }

    #[test]
    fn migration_seeds_default_schedule() {
        let conn = migrated_db();
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM schedules WHERE is_default = 1 AND name = 'Default'"
            ),
            1
        );
        // 5 weekdays × 2 windows + 2 weekend days × 1 window = 12.
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM schedule_windows"), 12);
    }

    /// Same `(calendar_id, event_id)` must be rejected; the same `event_id`
    /// on a different calendar must be allowed.
    #[test]
    fn external_events_unique_per_calendar() {
        let conn = migrated_db();
        let ts = "2026-03-13T10:00:00+00:00";
        let insert = |id: &str, calendar: &str| {
            conn.execute(
                "INSERT INTO external_events \
                 (id, calendar_id, event_id, title, start_utc, end_utc, updated_at) \
                 VALUES (?1, ?2, 'ev-1', 'Event', ?3, ?3, ?3)",
                rusqlite::params![id, calendar, ts],
            )
        };
        insert("id-1", "cal-a").expect("first insert");
        assert!(
            insert("id-2", "cal-a").is_err(),
            "same (calendar_id, event_id) must fail UNIQUE"
        );
        insert("id-3", "cal-b").expect("same event_id on another calendar is allowed");
    }

    /// Comments are cascade-deleted with their task (M12.8).
    #[test]
    fn comments_cascade_with_task() {
        let conn = migrated_db();
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .expect("enable foreign keys");

        let schedule_id: String = conn
            .query_row("SELECT id FROM schedules WHERE is_default = 1", [], |row| {
                row.get(0)
            })
            .expect("default schedule id");
        let ts = "2026-07-13T10:00:00+00:00";
        conn.execute(
            "INSERT INTO tasks (id, title, duration_minutes, schedule_id, created_at, updated_at) \
             VALUES ('t-1', 'Parent task', 60, ?1, ?2, ?2)",
            rusqlite::params![schedule_id, ts],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO comments (id, task_id, author, content, created_at, updated_at) \
             VALUES ('c-1', 't-1', 'User', 'First comment', ?1, ?1)",
            [ts],
        )
        .expect("insert comment");

        conn.execute("DELETE FROM tasks WHERE id = 't-1'", [])
            .expect("delete task");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comments"), 0);
    }

    #[test]
    fn migrated_schema_at_version_one() {
        let conn = migrated_db();
        let version: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("read version");
        assert_eq!(version, current_version());
        assert_eq!(version, 1);
    }
}
