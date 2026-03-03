// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Agenda assembly — chunks in a time range enriched with task metadata.

use chrono::{DateTime, Utc};

use crate::domain::inputs::AgendaItem;
use crate::error::AppError;
use crate::traits::storage::Store;

/// Build an agenda of chunks in a time range, enriched with task metadata.
///
/// When a `label_filter` is provided, only chunks whose task has at least one
/// matching label are included (OR semantics).
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn get_agenda(
    store: &dyn Store,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    label_filter: Option<&[String]>,
) -> Result<Vec<AgendaItem>, AppError> {
    let items = store.get_agenda_in_range(start, end)?;

    let Some(filter) = label_filter else {
        return Ok(items);
    };
    Ok(items
        .into_iter()
        .filter(|item| item.task_labels.iter().any(|l| filter.contains(l)))
        .collect())
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, TimeZone, Utc};

    use super::get_agenda;
    use crate::db::sqlite::SqliteStore;
    use crate::domain::enums::Priority;
    use crate::domain::inputs::AgendaItem;
    use crate::domain::models::Task;
    use crate::services::task::test_helpers::{make_scheduled_chunk, make_task};
    use crate::test_support::{fixture_base, seed_chunk, seed_task, test_store};

    /// Standard agenda query window for these tests: 1 hour before to 3 hours after `fixture_base()`.
    fn standard_range() -> (DateTime<Utc>, DateTime<Utc>) {
        let base = fixture_base();
        (base - Duration::hours(1), base + Duration::hours(3))
    }

    /// Seed `task` plus one 30-minute scheduled chunk (`chunk_id`) owned by it.
    fn seed_task_and_chunk(store: &SqliteStore, task: &Task, chunk_id: &str) {
        seed_task(store, task);
        seed_chunk(store, &make_scheduled_chunk(chunk_id, &task.id, 30));
    }

    fn agenda(store: &SqliteStore, label_filter: Option<&[String]>) -> Vec<AgendaItem> {
        let (start, end) = standard_range();
        get_agenda(store, start, end, label_filter).expect("should succeed")
    }

    #[test]
    fn get_agenda_happy_path() {
        let store = test_store();
        let mut task = make_task("task-1");
        task.title = "Study Rust".to_owned();
        task.priority = Priority::High;
        task.labels = vec!["programming".to_owned()];
        task.deadline = Some(Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap());
        seed_task_and_chunk(&store, &task, "chunk-1");

        let items = agenda(&store, None);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].task_title, "Study Rust");
        assert_eq!(items[0].task_priority, Priority::High);
        assert_eq!(items[0].task_labels, vec!["programming"]);
        assert_eq!(items[0].chunk.id, "chunk-1");
        assert_eq!(items[0].task_recurring_template_id, None);
        assert_eq!(
            items[0].task_deadline,
            Some(Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn get_agenda_task_deadline_none_when_task_has_no_deadline() {
        let store = test_store();
        let mut task = make_task("task-1");
        task.deadline = None;
        seed_task_and_chunk(&store, &task, "chunk-1");

        let items = agenda(&store, None);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].task_deadline, None);
    }

    #[test]
    fn get_agenda_recurring_instance_carries_template_id() {
        let store = test_store();
        let mut task = make_task("task-1");
        task.recurring_template_id = Some("tpl-1".to_owned());
        seed_task_and_chunk(&store, &task, "chunk-1");

        let items = agenda(&store, None);

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].task_recurring_template_id,
            Some("tpl-1".to_owned())
        );
    }

    #[test]
    fn get_agenda_empty_range() {
        let store = test_store();
        seed_task_and_chunk(&store, &make_task("task-1"), "chunk-1");

        let range_start = fixture_base() + Duration::days(1);
        let range_end = fixture_base() + Duration::days(1) + Duration::hours(12);

        let items = get_agenda(&store, range_start, range_end, None).expect("should succeed");
        assert!(items.is_empty());
    }

    #[test]
    fn get_agenda_with_label_filter() {
        let store = test_store();

        let mut task1 = make_task("task-1");
        task1.labels = vec!["reading".to_owned()];
        seed_task_and_chunk(&store, &task1, "chunk-1");

        let mut task2 = make_task("task-2");
        task2.labels = vec!["programming".to_owned()];
        seed_task_and_chunk(&store, &task2, "chunk-2");

        let filter = vec!["reading".to_owned()];
        let items = agenda(&store, Some(&filter));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].chunk.task_id, "task-1");
    }

    #[test]
    fn get_agenda_label_filter_no_matches() {
        let store = test_store();

        let mut task = make_task("task-1");
        task.labels = vec!["reading".to_owned()];
        seed_task_and_chunk(&store, &task, "chunk-1");

        let filter = vec!["sports".to_owned()];
        let items = agenda(&store, Some(&filter));
        assert!(items.is_empty());
    }
}
