// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! [`ConfigStore`] implementation.
//!
//! Query helpers take a plain `&Connection` and are shared by the
//! mutex-guarded [`SqliteStore`] impls and the transaction-scoped
//! [`TxStore`] impls.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use super::{format_optional_datetime, parse_optional_datetime, SqliteStore, TxStore};
use crate::domain::models::AppConfig;
use crate::error::AppError;
use crate::traits::storage::ConfigStore;

fn parse_config_int(value: &str, key: &str) -> Result<i64, AppError> {
    value
        .parse::<i64>()
        .map_err(|e| AppError::Database(format!("invalid {key}: {e}")))
}

// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
fn get_config(conn: &Connection) -> Result<AppConfig, AppError> {
    let mut stmt = conn.prepare("SELECT key, value FROM config")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut planning_horizon_days: Option<i64> = None;
    let mut timezone: Option<String> = None;
    let mut max_continuous_minutes: Option<i64> = None;
    let mut min_break_minutes: Option<i64> = None;
    let mut last_reschedule: Option<Option<DateTime<Utc>>> = None;
    let mut last_mutation: Option<Option<DateTime<Utc>>> = None;
    let mut last_sync: Option<Option<DateTime<Utc>>> = None;
    let mut last_busy_sync: Option<Option<DateTime<Utc>>> = None;

    for row in rows {
        let (key, value) = row?;
        match key.as_str() {
            "planning_horizon_days" => {
                planning_horizon_days = Some(parse_config_int(&value, "planning_horizon_days")?);
            }
            "timezone" => {
                timezone = Some(value);
            }
            "max_continuous_minutes" => {
                max_continuous_minutes = Some(parse_config_int(&value, "max_continuous_minutes")?);
            }
            "min_break_minutes" => {
                min_break_minutes = Some(parse_config_int(&value, "min_break_minutes")?);
            }
            "last_reschedule" => {
                last_reschedule = Some(parse_optional_datetime(&value)?);
            }
            "last_mutation" => {
                last_mutation = Some(parse_optional_datetime(&value)?);
            }
            "last_sync" => {
                last_sync = Some(parse_optional_datetime(&value)?);
            }
            "last_busy_sync" => {
                last_busy_sync = Some(parse_optional_datetime(&value)?);
            }
            _ => {}
        }
    }

    Ok(AppConfig {
        planning_horizon_days: planning_horizon_days.ok_or_else(|| {
            AppError::Database("missing config key: planning_horizon_days".into())
        })?,
        timezone: timezone
            .ok_or_else(|| AppError::Database("missing config key: timezone".into()))?,
        max_continuous_minutes: max_continuous_minutes.ok_or_else(|| {
            AppError::Database("missing config key: max_continuous_minutes".into())
        })?,
        min_break_minutes: min_break_minutes
            .ok_or_else(|| AppError::Database("missing config key: min_break_minutes".into()))?,
        last_reschedule: last_reschedule
            .ok_or_else(|| AppError::Database("missing config key: last_reschedule".into()))?,
        last_mutation: last_mutation
            .ok_or_else(|| AppError::Database("missing config key: last_mutation".into()))?,
        last_sync: last_sync
            .ok_or_else(|| AppError::Database("missing config key: last_sync".into()))?,
        last_busy_sync: last_busy_sync
            .ok_or_else(|| AppError::Database("missing config key: last_busy_sync".into()))?,
    })
}

fn update_config(conn: &Connection, config: &AppConfig) -> Result<(), AppError> {
    let pairs: &[(&str, String)] = &[
        (
            "planning_horizon_days",
            config.planning_horizon_days.to_string(),
        ),
        ("timezone", config.timezone.clone()),
        (
            "max_continuous_minutes",
            config.max_continuous_minutes.to_string(),
        ),
        ("min_break_minutes", config.min_break_minutes.to_string()),
        (
            "last_reschedule",
            format_optional_datetime(config.last_reschedule.as_ref()),
        ),
        (
            "last_mutation",
            format_optional_datetime(config.last_mutation.as_ref()),
        ),
        (
            "last_sync",
            format_optional_datetime(config.last_sync.as_ref()),
        ),
        (
            "last_busy_sync",
            format_optional_datetime(config.last_busy_sync.as_ref()),
        ),
    ];

    let mut stmt = conn.prepare("INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)")?;
    for (key, value) in pairs {
        stmt.execute(rusqlite::params![key, value])?;
    }
    Ok(())
}

fn get_config_value(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    conn.query_row(
        "SELECT value FROM config WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(AppError::from)
}

fn set_config_value(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

impl ConfigStore for SqliteStore {
    fn get_config(&self) -> Result<AppConfig, AppError> {
        get_config(&*self.lock()?)
    }

    fn update_config(&self, config: &AppConfig) -> Result<(), AppError> {
        self.in_tx(|conn| update_config(conn, config))
    }

    fn get_config_value(&self, key: &str) -> Result<Option<String>, AppError> {
        get_config_value(&*self.lock()?, key)
    }

    fn set_config_value(&self, key: &str, value: &str) -> Result<(), AppError> {
        set_config_value(&*self.lock()?, key, value)
    }
}

impl ConfigStore for TxStore<'_> {
    fn get_config(&self) -> Result<AppConfig, AppError> {
        get_config(self.conn)
    }

    fn update_config(&self, config: &AppConfig) -> Result<(), AppError> {
        update_config(self.conn, config)
    }

    fn get_config_value(&self, key: &str) -> Result<Option<String>, AppError> {
        get_config_value(self.conn, key)
    }

    fn set_config_value(&self, key: &str, value: &str) -> Result<(), AppError> {
        set_config_value(self.conn, key, value)
    }
}
