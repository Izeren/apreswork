// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

use chrono::{DateTime, Duration, NaiveTime, Utc, Weekday};
use test_case::test_case;

use super::{create_template, delete_template, get_orphaned_template_instances, update_template};
use crate::db::sqlite::SqliteStore;
use crate::domain::cadence::Cadence;
use crate::domain::enums::{ChunkStatus, Priority, TaskStatus};
use crate::domain::inputs::{CreateTemplateInput, UpdateTemplateInput};
use crate::domain::models::{AppConfig, Chunk, RecurringTemplate, Schedule, ScheduleWindow, Task};
use crate::error::AppError;
use crate::test_support::{
    default_config, default_schedule_id, schedule_with_window, seed_chunk, seed_task,
    seed_template, test_now, test_store_with_config, utc,
};
use crate::traits::storage::{
    ChunkStore, ConfigStore, RecurringTemplateStore, ScheduleStore, TaskStore,
};

#[test_case("UTC", utc(2026, 6, 1, 14, 30) + Duration::seconds(45) => utc(2026, 6, 1, 0, 0) ; "utc strips the time of day")]
#[test_case("Europe/Berlin", utc(2026, 6, 1, 0, 30) => utc(2026, 5, 31, 22, 0) ; "berlin floors to local midnight (prev-day utc)")]
fn create_template_truncates_anchor(tz: &str, anchor: DateTime<Utc>) -> DateTime<Utc> {
    let store = test_store_with_config(AppConfig {
        timezone: tz.to_owned(),
        ..default_config()
    });
    let mut input = CreateTemplateInput::test_default();
    input.start_date = Some(anchor);
    create_template(&store, input, test_now())
        .expect("should succeed")
        .start_date
}

#[test_case(
    "Weekly review", 45, Cadence::weekly(vec![Weekday::Mon, Weekday::Wed, Weekday::Fri]),
    None, None, None, None
    ; "weekly, optionals defaulted"
)]
#[test_case(
    "Monthly report", 90, Cadence::monthly(15),
    None, None, None, None
    ; "monthly, optionals defaulted"
)]
#[test_case(
    "Critical review", 30, Cadence::weekly(vec![Weekday::Tue]),
    Some(Priority::Critical), Some(vec!["fitness".to_owned(), "weekly".to_owned()]),
    Some(utc(2026, 6, 1, 0, 0)), None
    ; "explicit priority, labels, start_date"
)]
#[test_case(
    "Custom scheduled", 75, Cadence::monthly(3),
    None, None, None, Some("custom-schedule".to_owned())
    ; "explicit schedule_id"
)]
#[allow(clippy::needless_pass_by_value)]
fn create_template_persists_fields_and_applies_defaults(
    title: &str,
    duration: i64,
    cadence: Cadence,
    priority: Option<Priority>,
    labels: Option<Vec<String>>,
    start_date: Option<DateTime<Utc>>,
    schedule_id: Option<String>,
) {
    let store = test_store_with_config(default_config());
    // Seed any explicit schedule with a 120-min window so capacity validation
    // passes (all test durations in this table are ≤ 120 min).
    if let Some(id) = &schedule_id {
        store
            .create_schedule(&schedule_with_window(id, 120))
            .expect("seed schedule");
    }

    let input = CreateTemplateInput {
        title: title.to_owned(),
        duration_minutes: duration,
        cadence: cadence.clone(),
        priority,
        labels: labels.clone(),
        start_date,
        schedule_id: schedule_id.clone(),
        ..CreateTemplateInput::test_default()
    };
    let template = create_template(&store, input, test_now()).expect("should succeed");

    assert_eq!(template.title, title);
    assert_eq!(template.duration_minutes, duration);
    assert_eq!(template.cadence, cadence);
    // Optional fields: the explicit value when given, else the default.
    assert_eq!(template.priority, priority.unwrap_or(Priority::Medium));
    assert_eq!(template.labels, labels.unwrap_or_default());
    assert_eq!(
        template.schedule_id,
        schedule_id.unwrap_or_else(|| default_schedule_id(&store))
    );
    assert!(template.is_active);
    if let Some(anchor) = start_date {
        // An explicit anchor is stored as given (already at local midnight).
        assert_eq!(template.start_date, anchor);
    } else {
        // Default anchor = injected now floored to local midnight (UTC = same value).
        assert_eq!(template.start_date, test_now());
    }
    assert!(store.get_template(&template.id).unwrap().is_some());
}

#[test]
fn create_template_zero_duration_rejected() {
    let store = test_store_with_config(default_config());
    let mut input = CreateTemplateInput::test_default();
    input.duration_minutes = 0;
    assert!(matches!(
        create_template(&store, input, test_now()),
        Err(AppError::Validation(_))
    ));
}

#[test]
fn create_template_rejects_duration_exceeding_largest_window() {
    let store = test_store_with_config(default_config());
    // Schedule with 50-min window; template duration=60 → 60 > 50 → fail
    store
        .create_schedule(&schedule_with_window("tiny-sched-tmpl", 50))
        .expect("seed schedule");
    let mut input = CreateTemplateInput::test_default();
    input.duration_minutes = 60;
    input.schedule_id = Some("tiny-sched-tmpl".to_owned());
    let result = create_template(&store, input, test_now());
    assert!(
        matches!(result, Err(crate::error::AppError::Validation(_))),
        "expected Validation error, got: {result:?}"
    );
}

#[test]
fn create_template_bogus_explicit_schedule_id_returns_not_found() {
    let store = test_store_with_config(default_config());
    let mut input = CreateTemplateInput::test_default();
    input.schedule_id = Some("nonexistent-schedule".to_owned());
    let result = create_template(&store, input, test_now());
    assert!(
        matches!(result, Err(crate::error::AppError::NotFound { .. })),
        "expected NotFound for bogus schedule_id, got: {result:?}"
    );
}

#[test_case("UTC", utc(2026, 7, 1, 9, 15) + Duration::seconds(30) => utc(2026, 7, 1, 0, 0) ; "utc strips the time of day")]
#[test_case("Europe/Berlin", utc(2026, 6, 1, 0, 30) => utc(2026, 5, 31, 22, 0) ; "berlin floors to local midnight (prev-day utc)")]
fn update_template_truncates_anchor(tz: &str, anchor: DateTime<Utc>) -> DateTime<Utc> {
    let store = test_store_with_config(AppConfig {
        timezone: tz.to_owned(),
        ..default_config()
    });
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-anchor")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    let input = UpdateTemplateInput {
        start_date: Some(anchor),
        ..UpdateTemplateInput::default()
    };
    update_template(&store, "tmpl-anchor", input, test_now())
        .expect("should succeed")
        .start_date
}

#[test]
fn update_template_not_found() {
    let store = test_store_with_config(default_config());
    let result = update_template(
        &store,
        "nonexistent",
        UpdateTemplateInput::default(),
        test_now(),
    );
    assert!(matches!(
        result,
        Err(AppError::NotFound { ref entity, ref id })
        if entity == "RecurringTemplate" && id == "nonexistent"
    ));
}

#[test]
fn update_template_zero_duration_rejected() {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-3")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    let input = UpdateTemplateInput {
        duration_minutes: Some(0),
        ..UpdateTemplateInput::default()
    };
    assert!(matches!(
        update_template(&store, "tmpl-3", input, test_now()),
        Err(AppError::Validation(_))
    ));
}

// Each case sets one or more fields; `patch` applies the same change to a
// clone of the seeded template to build the expected result. Comparing the
// whole struct (minus the always-bumped `updated_at`) proves both that the
// patched field(s) took effect *and* that every other field stayed
// untouched — patch independence — and that the patch bumps `updated_at`.
// The last case patches several fields at once to catch any cross-field
// interference.
#[test_case(
    UpdateTemplateInput { title: Some("New title".to_owned()), ..UpdateTemplateInput::default() },
    (|t: &mut RecurringTemplate| t.title = "New title".to_owned()) as fn(&mut RecurringTemplate)
    ; "title"
)]
#[test_case(
    UpdateTemplateInput {
        description: Some(Some("New desc".to_owned())),
        ..UpdateTemplateInput::default()
    },
    (|t| t.description = Some("New desc".to_owned())) as fn(&mut RecurringTemplate)
    ; "description"
)]
#[test_case(
    UpdateTemplateInput { duration_minutes: Some(120), ..UpdateTemplateInput::default() },
    (|t| t.duration_minutes = 120) as fn(&mut RecurringTemplate)
    ; "duration"
)]
#[test_case(
    UpdateTemplateInput { priority: Some(Priority::Critical), ..UpdateTemplateInput::default() },
    (|t| t.priority = Priority::Critical) as fn(&mut RecurringTemplate)
    ; "priority"
)]
#[test_case(
    UpdateTemplateInput { schedule_id: Some("sched-x".to_owned()), ..UpdateTemplateInput::default() },
    (|t| t.schedule_id = "sched-x".to_owned()) as fn(&mut RecurringTemplate)
    ; "schedule_id"
)]
#[test_case(
    UpdateTemplateInput { cadence: Some(Cadence::monthly(20)), ..UpdateTemplateInput::default() },
    (|t| t.cadence = Cadence::monthly(20)) as fn(&mut RecurringTemplate)
    ; "cadence"
)]
#[test_case(
    UpdateTemplateInput {
        labels: Some(vec!["a".to_owned(), "b".to_owned()]),
        ..UpdateTemplateInput::default()
    },
    (|t| t.labels = vec!["a".to_owned(), "b".to_owned()]) as fn(&mut RecurringTemplate)
    ; "labels"
)]
#[test_case(
    UpdateTemplateInput { is_active: Some(false), ..UpdateTemplateInput::default() },
    (|t| t.is_active = false) as fn(&mut RecurringTemplate)
    ; "is_active deactivates"
)]
#[test_case(
    UpdateTemplateInput { start_date: Some(utc(2026, 8, 1, 0, 0)), ..UpdateTemplateInput::default() },
    (|t| t.start_date = utc(2026, 8, 1, 0, 0)) as fn(&mut RecurringTemplate)
    ; "start_date"
)]
#[test_case(
    UpdateTemplateInput {
        title: Some("Multi".to_owned()),
        duration_minutes: Some(15),
        start_date: Some(utc(2026, 9, 1, 0, 0)),
        ..UpdateTemplateInput::default()
    },
    (|t| {
        t.title = "Multi".to_owned();
        t.duration_minutes = 15;
        t.start_date = utc(2026, 9, 1, 0, 0);
    }) as fn(&mut RecurringTemplate)
    ; "multiple fields at once"
)]
#[allow(clippy::needless_pass_by_value)]
fn update_template_patches_field(input: UpdateTemplateInput, patch: fn(&mut RecurringTemplate)) {
    let store = test_store_with_config(default_config());
    // Seed the FK target for the schedule_id case.
    // Needs a window ≥ template duration (60 min default) to pass capacity check.
    store
        .create_schedule(&Schedule {
            name: "Schedule X".to_owned(),
            windows: vec![ScheduleWindow {
                id: "sched-x-win".to_owned(),
                schedule_id: "sched-x".to_owned(),
                day_of_week: Weekday::Mon,
                start_time: NaiveTime::from_hms_opt(18, 0, 0).expect("valid time"),
                end_time: NaiveTime::from_hms_opt(20, 0, 0).expect("valid time"),
            }],
            ..Schedule::test_default().with_id("sched-x")
        })
        .expect("seed sched-x");
    let seeded = RecurringTemplate::test_default()
        .with_id("tmpl-all")
        .with_cadence(Cadence::weekly(vec![Weekday::Mon]));
    seed_template(&store, &seeded);

    // Expected = the seeded template with exactly the patched field changed.
    let mut expected = seeded.clone();
    patch(&mut expected);

    let result = update_template(&store, "tmpl-all", input, test_now()).expect("should succeed");

    assert_eq!(result.updated_at, test_now());
    expected.updated_at = test_now();
    assert_eq!(result, expected);
}

fn check_sets_last_mutation(service_call: impl FnOnce(&SqliteStore, DateTime<Utc>)) {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("t")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    let now_ts = test_now();
    service_call(&store, now_ts);
    let last_mut = store
        .get_config()
        .expect("config")
        .last_mutation
        .expect("last_mutation should be Some");
    assert_eq!(last_mut, now_ts);
}

#[test]
fn update_template_sets_last_mutation() {
    check_sets_last_mutation(|store, now| {
        update_template(store, "t", UpdateTemplateInput::default(), now).expect("should succeed");
    });
}

#[test]
fn update_template_rejects_schedule_reassignment_to_smaller_schedule() {
    let store = test_store_with_config(default_config());
    // Big schedule (template currently fits: duration=60 ≤ 120)
    store
        .create_schedule(&schedule_with_window("big-sched-tmpl", 120))
        .expect("seed big schedule");
    // Small schedule (duration=60 won't fit: 60 > 50)
    store
        .create_schedule(&schedule_with_window("small-sched-tmpl", 50))
        .expect("seed small schedule");

    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-reassign")
            .with_schedule("big-sched-tmpl")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );

    let input = UpdateTemplateInput {
        schedule_id: Some("small-sched-tmpl".to_owned()),
        ..UpdateTemplateInput::default()
    };
    let result = update_template(&store, "tmpl-reassign", input, test_now());
    assert!(
        matches!(result, Err(crate::error::AppError::Validation(_))),
        "expected Validation error for reassignment to smaller schedule, got: {result:?}"
    );
}

#[test]
fn update_template_accepts_cadence_change_when_duration_still_fits() {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-cadence")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    // default-schedule-id is auto-created with 300-min window → duration=60 fits
    let input = UpdateTemplateInput {
        cadence: Some(Cadence::monthly(15)),
        ..UpdateTemplateInput::default()
    };
    let result = update_template(&store, "tmpl-cadence", input, test_now());
    assert!(
        result.is_ok(),
        "cadence change on fitting template must succeed: {result:?}"
    );
    assert_eq!(result.unwrap().cadence, Cadence::monthly(15));
}

#[test]
fn update_template_rejects_reactivation_when_oversized() {
    let store = test_store_with_config(default_config());
    // Seed a schedule with a 50-min window
    store
        .create_schedule(&schedule_with_window("tight-sched", 50))
        .expect("seed schedule");
    // Seed an inactive template with duration=60 on the tight schedule.
    // Must bypass service capacity check (duration=60 > 50 would be rejected
    // by create_template), so we seed directly at store level.
    let tmpl = RecurringTemplate {
        id: "tmpl-inactive".to_owned(),
        duration_minutes: 60,
        schedule_id: "tight-sched".to_owned(),
        is_active: false,
        ..RecurringTemplate::test_default()
    };
    store
        .create_template(&tmpl)
        .expect("seed inactive template");

    // Attempt reactivation → capacity re-check fires: 60 > 50 → fail
    let input = UpdateTemplateInput {
        is_active: Some(true),
        ..UpdateTemplateInput::default()
    };
    let result = update_template(&store, "tmpl-inactive", input, test_now());
    assert!(
        matches!(result, Err(crate::error::AppError::Validation(_))),
        "expected Validation error on reactivation of oversized template, got: {result:?}"
    );
}

#[test]
fn update_template_bogus_schedule_reassignment_returns_not_found() {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-bogus-sched")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    let input = UpdateTemplateInput {
        schedule_id: Some("no-such-schedule".to_owned()),
        ..UpdateTemplateInput::default()
    };
    let result = update_template(&store, "tmpl-bogus-sched", input, test_now());
    assert!(
        matches!(result, Err(crate::error::AppError::NotFound { .. })),
        "expected NotFound for bogus schedule reassignment, got: {result:?}"
    );
}

#[test]
fn delete_template_happy_path() {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-del")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    delete_template(&store, "tmpl-del", test_now()).expect("should succeed");
    assert!(store.get_template("tmpl-del").unwrap().is_none());
}

#[test]
fn delete_template_not_found() {
    let store = test_store_with_config(default_config());
    let result = delete_template(&store, "nonexistent", test_now());
    assert!(matches!(
        result,
        Err(AppError::NotFound { ref entity, ref id })
        if entity == "RecurringTemplate" && id == "nonexistent"
    ));
}

// Open instances (and their chunks) are deleted; closed instances are
// delinked (template_id cleared) with their chunks preserved as history.
#[test_case(TaskStatus::Pending, false ; "pending instance deleted")]
#[test_case(TaskStatus::Scheduled, false ; "scheduled instance deleted")]
#[test_case(TaskStatus::Completed, true ; "completed instance delinked")]
#[test_case(TaskStatus::Cancelled, true ; "cancelled instance delinked")]
fn delete_template_disposes_instance_by_status(status: TaskStatus, delinked: bool) {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    seed_task(
        &store,
        &Task::test_default()
            .with_id("inst")
            .with_template("tmpl")
            .with_status(status),
    );
    seed_chunk(
        &store,
        &Chunk::test_default()
            .with_id("chunk")
            .with_task("inst")
            .with_status(ChunkStatus::Scheduled),
    );

    delete_template(&store, "tmpl", test_now()).expect("should succeed");

    let task = store.get_task("inst").expect("get_task");
    let chunk = store.get_chunk("chunk").expect("get_chunk");
    if delinked {
        assert_eq!(
            task.expect("closed instance preserved")
                .recurring_template_id,
            None
        );
        assert!(chunk.is_some(), "closed instance's chunk preserved");
    } else {
        assert!(task.is_none(), "open instance deleted");
        assert!(chunk.is_none(), "open instance's chunk deleted");
    }
}

#[test]
fn delete_template_sets_last_mutation() {
    check_sets_last_mutation(|store, now| {
        delete_template(store, "t", now).expect("should succeed");
    });
}

// A virtual "next" instance is returned only for an active template with no
// OPEN (pending/scheduled) instance; closed instances don't suppress it.
#[test_case(true, Cadence::weekly(vec![Weekday::Mon]), None => 1 ; "active, no instances")]
#[test_case(true, Cadence::monthly(15), None => 1 ; "active monthly, no instances")]
#[test_case(true, Cadence::weekly(vec![Weekday::Mon]), Some(TaskStatus::Completed) => 1 ; "active, only a closed instance")]
#[test_case(true, Cadence::weekly(vec![Weekday::Mon]), Some(TaskStatus::Pending) => 0 ; "active, an open instance")]
#[test_case(true, Cadence::weekly(vec![Weekday::Mon]), Some(TaskStatus::Scheduled) => 0 ; "active, a scheduled instance")]
#[test_case(false, Cadence::weekly(vec![Weekday::Mon]), None => 0 ; "inactive template")]
fn get_orphaned_virtual_count(active: bool, cadence: Cadence, seeded: Option<TaskStatus>) -> usize {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl")
            .with_cadence(cadence)
            .with_active(active),
    );
    if let Some(status) = seeded {
        seed_task(
            &store,
            &Task::test_default()
                .with_id("inst")
                .with_template("tmpl")
                .with_status(status),
        );
    }
    get_orphaned_template_instances(&store, test_now())
        .expect("should succeed")
        .len()
}

#[test]
fn orphaned_template_with_no_instances_returns_virtual_task() {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-b")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon, Weekday::Fri])),
    );

    let result = get_orphaned_template_instances(&store, test_now()).expect("should succeed");
    assert_eq!(result.len(), 1);

    let virtual_task = &result[0];
    assert_eq!(virtual_task.id, "virtual-tmpl-b");
    assert_eq!(virtual_task.title, "Test template");
    assert_eq!(virtual_task.duration_minutes, 60);
    assert_eq!(virtual_task.status, TaskStatus::Pending);
    assert!(virtual_task.no_split);
    assert_eq!(virtual_task.min_chunk_minutes, 60);
    assert_eq!(
        virtual_task.recurring_template_id,
        Some("tmpl-b".to_owned())
    );
    assert!(virtual_task.deadline.is_some());
}

#[test]
fn orphaned_no_templates_returns_empty() {
    let store = test_store_with_config(default_config());
    assert!(get_orphaned_template_instances(&store, test_now())
        .expect("should succeed")
        .is_empty());
}

// For each row: (update input, seeded status, is_pinned, expected_survives).
// The template starts with weekly-Mon cadence; the instance is seeded with a
// future deadline (test_now + 30 days) so the deadline > now filter includes
// it in the candidate set for deletion on cadence changes.
#[test_case(
    UpdateTemplateInput { title: Some("New".to_owned()), ..UpdateTemplateInput::default() },
    TaskStatus::Pending, false, true
    ; "non-cadence edit keeps open instance"
)]
#[test_case(
    UpdateTemplateInput { cadence: Some(Cadence::monthly(10)), ..UpdateTemplateInput::default() },
    TaskStatus::Pending, false, false
    ; "cadence change deletes open unpinned future instance"
)]
#[test_case(
    UpdateTemplateInput { cadence: Some(Cadence::monthly(10)), ..UpdateTemplateInput::default() },
    TaskStatus::Scheduled, true, true
    ; "cadence change keeps pinned instance"
)]
#[test_case(
    UpdateTemplateInput { cadence: Some(Cadence::monthly(10)), ..UpdateTemplateInput::default() },
    TaskStatus::Completed, false, true
    ; "cadence change keeps closed instance"
)]
#[test_case(
    UpdateTemplateInput {
        cadence: Some(Cadence::weekly(vec![Weekday::Mon])),
        ..UpdateTemplateInput::default()
    },
    TaskStatus::Pending, false, true
    ; "equal cadence update keeps open instance"
)]
#[test_case(
    UpdateTemplateInput { start_date: Some(utc(2026, 7, 8, 0, 0)), ..UpdateTemplateInput::default() },
    TaskStatus::Pending, false, false
    ; "start_date change deletes open unpinned future instance"
)]
#[allow(clippy::needless_pass_by_value)]
fn update_template_instance_survival(
    input: UpdateTemplateInput,
    status: TaskStatus,
    is_pinned: bool,
    expected_survives: bool,
) {
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-surv")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    seed_task(
        &store,
        &Task::test_default()
            .with_id("inst-surv")
            .with_template("tmpl-surv")
            .with_status(status)
            .with_pinned(is_pinned)
            .with_deadline(test_now() + Duration::days(30)),
    );
    seed_chunk(
        &store,
        &Chunk::test_default()
            .with_id("chunk-surv")
            .with_task("inst-surv"),
    );

    update_template(&store, "tmpl-surv", input, test_now()).expect("should succeed");

    let task = store.get_task("inst-surv").expect("get_task");
    let chunk = store.get_chunk("chunk-surv").expect("get_chunk");
    if expected_survives {
        assert!(task.is_some(), "instance should survive");
        assert!(chunk.is_some(), "chunk should survive");
    } else {
        assert!(task.is_none(), "instance should be deleted");
        assert!(chunk.is_none(), "chunk should be deleted");
    }
}

#[test]
fn update_template_cadence_change_preserves_overdue_instance() {
    // An open instance whose deadline is already past is not deleted by a
    // cadence change — auto_cancel_overdue handles overdue cleanup, not
    // the cadence-edit path.
    // Task::test_default() deadline = fixture_base() + 7 days (2026-07-08),
    // which is before test_now() (2030-01-01) → overdue.
    let store = test_store_with_config(default_config());
    seed_template(
        &store,
        &RecurringTemplate::test_default()
            .with_id("tmpl-ovd")
            .with_cadence(Cadence::weekly(vec![Weekday::Mon])),
    );
    seed_task(
        &store,
        &Task::test_default()
            .with_id("inst-ovd")
            .with_template("tmpl-ovd")
            .with_status(TaskStatus::Pending),
    );

    let input = UpdateTemplateInput {
        cadence: Some(Cadence::monthly(10)),
        ..UpdateTemplateInput::default()
    };
    update_template(&store, "tmpl-ovd", input, test_now()).expect("should succeed");

    assert!(
        store.get_task("inst-ovd").unwrap().is_some(),
        "overdue instance must not be deleted by a cadence change"
    );
}
