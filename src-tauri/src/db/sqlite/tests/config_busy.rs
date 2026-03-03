// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `ConfigStore` implementation.

use chrono::{DateTime, TimeZone, Utc};
use test_case::test_case;

use crate::db::sqlite::SqliteStore;
use crate::domain::models::AppConfig;
use crate::traits::storage::ConfigStore;

#[test]
fn get_config_returns_seed_defaults() {
    let store = SqliteStore::new_in_memory();
    let config = store.get_config().expect("get_config");

    assert_eq!(config.planning_horizon_days, 30);
    assert_eq!(config.timezone, "UTC");
    assert_eq!(config.max_continuous_minutes, 120);
    assert_eq!(config.min_break_minutes, 5);
    assert!(config.last_reschedule.is_none());
    assert!(config.last_mutation.is_none());
    assert!(config.last_sync.is_none());
    assert!(config.last_busy_sync.is_none());
}

#[test]
fn update_config_roundtrip() {
    let store = SqliteStore::new_in_memory();

    let new_config = AppConfig {
        planning_horizon_days: 60,
        timezone: "Europe/Berlin".to_owned(),
        max_continuous_minutes: 90,
        min_break_minutes: 10,
        last_reschedule: None,
        last_mutation: None,
        last_sync: None,
        last_busy_sync: None,
    };

    store.update_config(&new_config).expect("update_config");
    let loaded = store.get_config().expect("get_config");

    assert_eq!(loaded.planning_horizon_days, 60);
    assert_eq!(loaded.timezone, "Europe/Berlin");
    assert_eq!(loaded.max_continuous_minutes, 90);
    assert_eq!(loaded.min_break_minutes, 10);
}

#[test_case(None ; "preserves_none")]
#[test_case(Some(Utc.with_ymd_and_hms(2026, 3, 13, 10, 30, 0).unwrap()) ; "preserves_some")]
fn update_config_timestamp_persistence(ts: Option<DateTime<Utc>>) {
    let store = SqliteStore::new_in_memory();

    let config = AppConfig {
        planning_horizon_days: 30,
        timezone: "UTC".to_owned(),
        max_continuous_minutes: 120,
        min_break_minutes: 5,
        last_reschedule: ts,
        last_mutation: ts,
        last_sync: ts,
        last_busy_sync: ts,
    };

    store.update_config(&config).expect("update_config");
    let loaded = store.get_config().expect("get_config");

    assert_eq!(loaded.last_reschedule, ts);
    assert_eq!(loaded.last_mutation, ts);
    assert_eq!(loaded.last_sync, ts);
    assert_eq!(loaded.last_busy_sync, ts);
}
