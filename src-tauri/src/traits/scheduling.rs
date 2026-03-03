// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Scheduling trait interface.
//!
//! Defines the pure algorithmic contract for the scheduling engine. The
//! [`Scheduler`] trait accepts a self-contained [`ScheduleInput`] and returns
//! a [`ScheduleResult`] with placed [`Chunk`]s and any [`ScheduleWarning`]s.
//! All domain-specific types used by the trait are defined in this module.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::models::{Chunk, EntityId, Task};
use crate::error::AppError;

/// A concrete time slot available for scheduling, expanded from schedule windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableSlot {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub schedule_id: EntityId,
}

/// The result of a scheduling run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleResult {
    pub placed_chunks: Vec<Chunk>,
    pub warnings: Vec<ScheduleWarning>,
}

/// A warning produced during scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleWarning {
    pub task_id: EntityId,
    pub task_title: String,
    pub kind: WarningKind,
}

/// Classification of scheduling warnings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WarningKind {
    /// The task cannot be completed before its deadline given available slots.
    DeadlineViolation {
        deadline: DateTime<Utc>,
        earliest_completion: DateTime<Utc>,
    },
    /// The task cannot be placed at all (e.g. no slots, chunk too large to split).
    Unschedulable { reason: String },
}

/// Input struct wrapping all data required for a scheduling run.
///
/// Designed to be enriched over time (new fields added) without breaking the
/// [`Scheduler`] trait signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInput {
    pub tasks: Vec<Task>,
    pub existing_fixed_chunks: Vec<Chunk>,
    pub available_slots: Vec<AvailableSlot>,
    pub horizon_end: DateTime<Utc>,
    /// Wall-clock time at the moment scheduling was requested; used for
    /// `created_at`/`updated_at` timestamps on newly placed chunks.
    pub now: DateTime<Utc>,
    /// Max back-to-back scheduled time (minutes) before a break is required.
    /// Sourced from [`crate::domain::models::AppConfig::max_continuous_minutes`].
    pub max_continuous_minutes: i64,
    /// Minimum break duration (minutes) between continuous blocks.
    /// Sourced from [`crate::domain::models::AppConfig::min_break_minutes`].
    pub min_break_minutes: i64,
}

/// Pure scheduling algorithm contract.
///
/// Implementations must be stateless with respect to scheduling decisions —
/// all input is provided via [`ScheduleInput`] and all output is returned in
/// [`ScheduleResult`]. Implementations must be `Send + Sync` so they can be
/// held in Tauri shared state.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if the algorithm encounters an irrecoverable
/// internal inconsistency. Soft failures (deadline violations, unschedulable
/// tasks) are reported as [`ScheduleWarning`]s in the result, not as errors.
pub trait Scheduler: Send + Sync {
    /// Run the scheduling algorithm over the given input.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] on irrecoverable algorithmic failure.
    fn schedule(&self, input: ScheduleInput) -> Result<ScheduleResult, AppError>;
}

/// The ONE task ordering used by every scheduling pipeline: priority
/// descending, then earlier deadline first (`None` last), then shorter
/// remaining duration, then title.
///
/// Both the full-run engine sort and the incremental cascade sort MUST call
/// this function — the cascade's convergence argument depends on the two
/// sorts agreeing (a processed task can only displace unprocessed tasks).
///
/// `remaining` maps a task id to its remaining unscheduled minutes
/// (duration − logged − fixed, floored at 0), which each pipeline computes
/// from its own view of the fixed chunks.
#[must_use]
pub fn scheduling_order(a: &Task, b: &Task, remaining: impl Fn(&str) -> i64) -> std::cmp::Ordering {
    b.priority
        .cmp(&a.priority)
        .then_with(|| cmp_deadline_option(a.deadline, b.deadline))
        .then_with(|| remaining(&a.id).cmp(&remaining(&b.id)))
        .then_with(|| a.title.cmp(&b.title))
}

/// Compare two optional deadlines: `Some(earlier)` < `Some(later)`, `None`
/// sorts last (greatest).
fn cmp_deadline_option(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(da), Some(db)) => da.cmp(&db),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use test_case::test_case;

    use super::scheduling_order;
    use crate::domain::enums::Priority;
    use crate::domain::models::Task;

    fn task(id: &str, priority: Priority, deadline: Option<&str>, title: &str) -> Task {
        let mut t = Task::test_default().with_id(id);
        t.priority = priority;
        t.deadline = deadline.map(|d| d.parse::<DateTime<Utc>>().expect("parse deadline"));
        t.title = title.to_owned();
        t
    }

    /// A `remaining` lookup where every task has the same remaining minutes.
    fn flat_remaining(_: &str) -> i64 {
        60
    }

    #[test]
    fn priority_dominates_all_other_keys() {
        // The Critical task loses the deadline and title keys — priority must
        // still win (lower-order keys are short-circuited entirely).
        let a = task("a", Priority::Critical, Some("2030-01-01T00:00:00Z"), "zz");
        let b = task("b", Priority::Low, Some("2026-01-01T00:00:00Z"), "aa");
        assert_eq!(scheduling_order(&a, &b, flat_remaining), Ordering::Less);
        assert_eq!(scheduling_order(&b, &a, flat_remaining), Ordering::Greater);
    }

    #[test_case(
        Some("2026-01-01T00:00:00Z"), Some("2026-06-01T00:00:00Z"), Ordering::Less;
        "earlier_deadline_first"
    )]
    #[test_case(
        Some("2030-01-01T00:00:00Z"), None, Ordering::Less;
        "some_deadline_before_none"
    )]
    #[test_case(
        None, Some("2026-01-01T00:00:00Z"), Ordering::Greater;
        "none_deadline_sorts_last"
    )]
    fn deadline_breaks_priority_ties(
        deadline_a: Option<&str>,
        deadline_b: Option<&str>,
        expected: Ordering,
    ) {
        let a = task("a", Priority::Medium, deadline_a, "same");
        let b = task("b", Priority::Medium, deadline_b, "same");
        assert_eq!(scheduling_order(&a, &b, flat_remaining), expected);
    }

    #[test]
    fn shorter_remaining_breaks_deadline_ties() {
        let a = task("a", Priority::Medium, None, "same");
        let b = task("b", Priority::Medium, None, "same");
        let remaining = |id: &str| if id == "a" { 30 } else { 120 };
        assert_eq!(scheduling_order(&a, &b, remaining), Ordering::Less);
        assert_eq!(scheduling_order(&b, &a, remaining), Ordering::Greater);
    }

    #[test]
    fn title_is_the_final_tiebreak() {
        let a = task("a", Priority::Medium, None, "alpha");
        let b = task("b", Priority::Medium, None, "beta");
        assert_eq!(scheduling_order(&a, &b, flat_remaining), Ordering::Less);
    }

    #[test]
    fn identical_keys_compare_equal() {
        let a = task("a", Priority::Medium, Some("2026-08-01T00:00:00Z"), "same");
        let b = task("b", Priority::Medium, Some("2026-08-01T00:00:00Z"), "same");
        assert_eq!(scheduling_order(&a, &b, flat_remaining), Ordering::Equal);
    }

    /// Shuffled fixture exercising every key level at once — pins the full
    /// policy so any change to the ordering breaks a test here, next to the
    /// single definition both pipelines share.
    #[test]
    fn shuffled_fixture_sorts_to_canonical_order() {
        let mut tasks = [
            task("t4", Priority::High, None, "beta"),
            task("t6", Priority::Low, Some("2026-01-01T00:00:00Z"), "gamma"),
            task("t2", Priority::High, Some("2026-03-01T00:00:00Z"), "any"),
            task("t5", Priority::High, None, "alpha"),
            task("t1", Priority::Critical, None, "zeta"),
            task("t3", Priority::High, Some("2026-04-01T00:00:00Z"), "any"),
        ];
        let remaining_by_task: HashMap<&str, i64> =
            HashMap::from([("t4", 30), ("t5", 30), ("t6", 15)]);
        tasks.sort_by(|a, b| {
            scheduling_order(a, b, |id| remaining_by_task.get(id).copied().unwrap_or(60))
        });
        let order: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        // t1: highest priority. t2/t3: deadline order. t4/t5: no deadline,
        // equal remaining, title order. t6: lowest priority despite the
        // earliest deadline and smallest remaining.
        assert_eq!(order, vec!["t1", "t2", "t3", "t5", "t4", "t6"]);
    }
}
