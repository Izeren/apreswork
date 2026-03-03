// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the `RecurringTemplateStore` implementation and cadence helpers.

use chrono::{TimeZone, Utc, Weekday};
use test_case::test_case;

use super::make_test_template;
use crate::db::sqlite::template::{cadence_from_db, cadence_to_db};
use crate::db::sqlite::SqliteStore;
use crate::domain::cadence::Cadence;
use crate::domain::enums::Priority;
use crate::traits::storage::RecurringTemplateStore;

#[test]
fn create_and_get_template_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let template = make_test_template(&store);

    store.create_template(&template).expect("create_template");
    let loaded = store
        .get_template(&template.id)
        .expect("get_template")
        .expect("template should exist");

    assert_eq!(loaded.id, template.id);
    assert_eq!(loaded.title, template.title);
    assert_eq!(loaded.description, template.description);
    assert_eq!(loaded.duration_minutes, template.duration_minutes);
    assert_eq!(loaded.priority, template.priority);
    assert_eq!(loaded.schedule_id, template.schedule_id);
    assert_eq!(loaded.cadence, template.cadence);
    assert_eq!(loaded.is_active, template.is_active);
    assert_eq!(loaded.start_date, template.start_date);
    assert_eq!(loaded.created_at, template.created_at);
    assert_eq!(loaded.updated_at, template.updated_at);
    assert!(loaded.labels.is_empty());
}

#[test]
fn get_template_not_found() {
    let store = SqliteStore::new_in_memory();
    let result = store.get_template("nonexistent-id").expect("get_template");
    assert!(result.is_none());
}

#[test]
fn create_template_with_labels() {
    let store = SqliteStore::new_in_memory();
    let mut template = make_test_template(&store);
    template.labels = vec!["fitness".to_owned(), "weekly".to_owned()];

    store.create_template(&template).expect("create_template");
    let loaded = store
        .get_template(&template.id)
        .expect("get_template")
        .expect("template should exist");

    let mut labels = loaded.labels.clone();
    labels.sort();
    assert_eq!(labels, vec!["fitness", "weekly"]);
}

#[test]
fn update_template_roundtrip() {
    let store = SqliteStore::new_in_memory();
    let mut template = make_test_template(&store);
    store.create_template(&template).expect("create_template");

    template.title = "Updated Template".to_owned();
    template.description = None;
    template.duration_minutes = 60;
    template.priority = Priority::High;
    template.cadence = Cadence::monthly(15);
    template.is_active = false;
    template.start_date = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
    store.update_template(&template).expect("update_template");

    let loaded = store
        .get_template(&template.id)
        .expect("get_template")
        .expect("template should exist");
    assert_eq!(loaded.title, "Updated Template");
    assert!(loaded.description.is_none());
    assert_eq!(loaded.duration_minutes, 60);
    assert_eq!(loaded.priority, Priority::High);
    assert_eq!(loaded.cadence, Cadence::monthly(15));
    assert!(!loaded.is_active);
    assert_eq!(loaded.start_date, template.start_date);
}

#[test]
fn update_template_replaces_labels() {
    let store = SqliteStore::new_in_memory();
    let mut template = make_test_template(&store);
    template.labels = vec!["a".to_owned(), "b".to_owned()];
    store.create_template(&template).expect("create_template");

    template.labels = vec!["c".to_owned()];
    store.update_template(&template).expect("update_template");

    let loaded = store
        .get_template(&template.id)
        .expect("get_template")
        .expect("template should exist");
    assert_eq!(loaded.labels, vec!["c"]);
}

#[test]
fn delete_template_removes_template_and_labels() {
    let store = SqliteStore::new_in_memory();
    let mut template = make_test_template(&store);
    template.labels = vec!["label1".to_owned()];
    store.create_template(&template).expect("create_template");

    store
        .delete_template(&template.id)
        .expect("delete_template");

    let loaded = store.get_template(&template.id).expect("get_template");
    assert!(loaded.is_none());

    let conn = store.conn.lock().expect("lock");
    let label_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM template_labels WHERE template_id = ?1",
            [&template.id],
            |row| row.get(0),
        )
        .expect("count labels");
    assert_eq!(label_count, 0);
}

#[test]
fn list_templates_includes_all() {
    let store = SqliteStore::new_in_memory();
    let mut t1 = make_test_template(&store);
    t1.title = "Template 1".to_owned();
    t1.labels = vec!["label-a".to_owned()];
    store.create_template(&t1).expect("create t1");

    let mut t2 = make_test_template(&store);
    t2.title = "Template 2".to_owned();
    t2.labels = vec!["label-b".to_owned()];
    store.create_template(&t2).expect("create t2");

    let all = store.list_templates().expect("list_templates");
    assert_eq!(all.len(), 2);

    let titles: Vec<&str> = all.iter().map(|t| t.title.as_str()).collect();
    assert!(titles.contains(&"Template 1"));
    assert!(titles.contains(&"Template 2"));

    for t in &all {
        assert_eq!(t.labels.len(), 1);
    }
}

#[test_case(Cadence::weekly(vec![Weekday::Tue]), "Weekly" ; "weekly single day")]
#[test_case(Cadence::weekly(vec![Weekday::Mon, Weekday::Wed, Weekday::Fri]), "Weekly" ; "weekly multi day")]
#[test_case(Cadence::weekly_every(2, vec![Weekday::Tue]), "Weekly" ; "biweekly")]
#[test_case(Cadence::monthly(1), "Monthly" ; "monthly cadence roundtrips")]
#[test_case(Cadence::monthly_every(3, 1), "Monthly" ; "quarterly cadence roundtrips")]
// test_case macro generates callers that pass by value.
#[allow(clippy::needless_pass_by_value)]
fn cadence_helpers_roundtrip(cadence: Cadence, expected_type: &str) {
    let (ctype, cdata) = cadence_to_db(&cadence).expect("to_db");
    assert_eq!(ctype, expected_type);
    let restored = cadence_from_db(&ctype, &cdata).expect("from_db");
    assert_eq!(restored, cadence);
}

#[test_case("Weekly", "not valid json{{{"  ; "rejects invalid json")]
#[test_case("Yearly", r#"{"day_of_year": 100}"# ; "rejects unknown type")]
fn cadence_from_db_error_cases(ctype: &str, cdata: &str) {
    assert!(cadence_from_db(ctype, cdata).is_err());
}

#[test]
fn create_template_with_no_labels() {
    let store = SqliteStore::new_in_memory();
    let template = make_test_template(&store);
    assert!(template.labels.is_empty());

    store.create_template(&template).expect("create_template");
    let loaded = store
        .get_template(&template.id)
        .expect("get_template")
        .expect("template should exist");

    assert!(loaded.labels.is_empty());
}

#[test]
fn list_templates_empty() {
    let store = SqliteStore::new_in_memory();
    let all = store.list_templates().expect("list_templates");
    assert!(all.is_empty());
}

#[test]
fn template_null_start_date_coalesces_to_created_at() {
    let store = SqliteStore::new_in_memory();
    let template = make_test_template(&store);
    store.create_template(&template).expect("create");

    let conn = store.conn.lock().expect("lock");
    conn.execute(
        "UPDATE recurring_templates SET start_date = NULL WHERE id = ?1",
        rusqlite::params![template.id],
    )
    .expect("null out start_date");
    drop(conn);

    let loaded = store
        .get_template(&template.id)
        .expect("get")
        .expect("exists");
    assert_eq!(loaded.start_date, loaded.created_at);
}
