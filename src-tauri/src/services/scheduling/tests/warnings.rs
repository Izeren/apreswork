// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the `retain_horizon_warnings` filter
//! (DESIGN.md § "Warning semantics").

use chrono::{DateTime, Duration, Utc};
use test_case::test_case;

use crate::domain::models::Task;
use crate::services::scheduling::retain_horizon_warnings;
use crate::test_support::utc;
use crate::traits::scheduling::{ScheduleWarning, WarningKind};

/// Horizon = 2026-04-22 10:00 UTC (30 days after a notional "now").
fn horizon() -> DateTime<Utc> {
    utc(2026, 4, 22, 10, 0)
}

fn unschedulable(task_id: &str) -> ScheduleWarning {
    ScheduleWarning {
        task_id: task_id.to_owned(),
        task_title: format!("Task {task_id}"),
        kind: WarningKind::Unschedulable {
            reason: "no fitting slot".to_owned(),
        },
    }
}

fn deadline_violation(task_id: &str, payload_deadline: DateTime<Utc>) -> ScheduleWarning {
    ScheduleWarning {
        task_id: task_id.to_owned(),
        task_title: format!("Task {task_id}"),
        kind: WarningKind::DeadlineViolation {
            deadline: payload_deadline,
            earliest_completion: payload_deadline + Duration::hours(1),
        },
    }
}

fn task_with_deadline(id: &str, deadline: Option<DateTime<Utc>>) -> Task {
    Task {
        id: id.to_owned(),
        deadline,
        ..Task::test_default()
    }
}

// ── Unschedulable: task-deadline governs ─────────────────────────────────────

/// Unschedulable with NO task deadline → dropped (normal backlog).
/// Unschedulable with deadline AFTER `horizon_end` → dropped (future backlog).
/// Unschedulable with deadline == `horizon_end` → kept (boundary: "on or before").
/// Unschedulable with deadline BEFORE `horizon_end` → kept (in-horizon shortfall).
#[test_case(
    None, false;
    "no_deadline_dropped"
)]
#[test_case(
    Some(utc(2026, 4, 23, 10, 0)), false;
    "deadline_after_horizon_dropped"
)]
#[test_case(
    Some(utc(2026, 4, 22, 10, 0)), true;
    "deadline_eq_horizon_kept"
)]
#[test_case(
    Some(utc(2026, 4, 21, 10, 0)), true;
    "deadline_before_horizon_kept"
)]
fn unschedulable_filter_by_task_deadline(
    task_deadline: Option<DateTime<Utc>>,
    expected_kept: bool,
) {
    let mut warnings = vec![unschedulable("task-1")];
    let tasks = vec![task_with_deadline("task-1", task_deadline)];
    retain_horizon_warnings(&mut warnings, &tasks, horizon());
    assert_eq!(
        warnings.len() == 1,
        expected_kept,
        "deadline={task_deadline:?}: expected kept={expected_kept}"
    );
}

/// Unschedulable warning whose task id is absent from the `tasks` slice
/// is dropped (defensive: cannot determine deadline, treat as no-deadline).
#[test]
fn unschedulable_unknown_task_id_dropped() {
    // The task slice contains a different id from the warning.
    let tasks = vec![task_with_deadline(
        "other-task",
        Some(utc(2026, 4, 21, 10, 0)),
    )];
    let mut warnings = vec![unschedulable("task-1")];
    retain_horizon_warnings(&mut warnings, &tasks, horizon());
    assert!(
        warnings.is_empty(),
        "warning for unknown task id must be dropped"
    );
}

// ── DeadlineViolation: payload deadline governs ───────────────────────────────

/// `DeadlineViolation` payload deadline BEFORE `horizon_end` → kept.
/// `DeadlineViolation` payload deadline == `horizon_end` → kept (boundary).
/// `DeadlineViolation` payload deadline AFTER `horizon_end` → dropped.
/// (Task map is NOT consulted for this kind — payload decides.)
#[test_case(
    utc(2026, 4, 21, 10, 0), true;
    "payload_deadline_before_horizon_kept"
)]
#[test_case(
    utc(2026, 4, 22, 10, 0), true;
    "payload_deadline_eq_horizon_kept"
)]
#[test_case(
    utc(2026, 4, 23, 10, 0), false;
    "payload_deadline_after_horizon_dropped"
)]
fn deadline_violation_filter_by_payload_deadline(
    payload_deadline: DateTime<Utc>,
    expected_kept: bool,
) {
    // Task map entry is irrelevant for DeadlineViolation — set a tight in-
    // horizon deadline to confirm the payload, not the task row, decides.
    let tasks = vec![task_with_deadline("task-1", Some(utc(2026, 4, 21, 10, 0)))];
    let mut warnings = vec![deadline_violation("task-1", payload_deadline)];
    retain_horizon_warnings(&mut warnings, &tasks, horizon());
    assert_eq!(
        warnings.len() == 1,
        expected_kept,
        "payload_deadline={payload_deadline}: expected kept={expected_kept}"
    );
}

/// Mixed warnings: kept items preserve original relative order; dropped items
/// are gone.
///
/// Input order:  A(Unsched,kept) · B(Unsched,drop) · C(DV,kept) · D(DV,drop)
/// Expected out: A · C
#[test]
fn mixed_warnings_kept_preserve_original_order() {
    let h = horizon();
    let tasks = vec![
        // task-a: has in-horizon deadline → Unschedulable kept
        task_with_deadline("task-a", Some(utc(2026, 4, 21, 10, 0))),
        // task-b: no deadline → Unschedulable dropped
        task_with_deadline("task-b", None),
        // task-c: (task entry present; kind decides for DV) → DV kept
        task_with_deadline("task-c", Some(utc(2026, 4, 21, 10, 0))),
        // task-d: (task entry present; kind decides for DV) → DV dropped
        task_with_deadline("task-d", Some(utc(2026, 4, 21, 10, 0))),
    ];
    let mut warnings = vec![
        unschedulable("task-a"),                               // kept
        unschedulable("task-b"),                               // dropped (no deadline)
        deadline_violation("task-c", utc(2026, 4, 21, 10, 0)), // kept (deadline < horizon)
        deadline_violation("task-d", utc(2026, 4, 23, 10, 0)), // dropped (deadline > horizon)
    ];

    retain_horizon_warnings(&mut warnings, &tasks, h);

    assert_eq!(warnings.len(), 2, "two warnings must survive");
    assert_eq!(
        warnings[0].task_id, "task-a",
        "first survivor must be task-a"
    );
    assert_eq!(
        warnings[1].task_id, "task-c",
        "second survivor must be task-c"
    );
}
