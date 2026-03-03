// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests for the reconcile module: recurring instance reconciliation and
//! overdue auto-cancellation, driven by a `Fixture` (`setup()`) over a real
//! in-memory `SqliteStore`.

use chrono::{DateTime, NaiveDate, Utc, Weekday};
use test_case::test_case;

use super::{auto_cancel_overdue, instance_from_template, reconcile};
use crate::db::sqlite::SqliteStore;
use crate::domain::cadence::{Cadence, Period, Window};
use crate::domain::date_utils::end_of_day;
use crate::domain::enums::{ChunkStatus, Priority, TaskStatus};
use crate::domain::inputs::TaskFilter;
use crate::domain::models::{Chunk, RecurringTemplate, Schedule, Task};
use crate::test_support::{
    default_config, seed_chunk, seed_schedule, seed_task, seed_template, test_store_with_config,
    utc,
};
use crate::traits::storage::{ChunkStore, TaskStore};

const TZ: chrono_tz::Tz = chrono_tz::UTC;

fn weekly_monday_builder(start: DateTime<Utc>) -> RecurringTemplate {
    RecurringTemplate::test_default()
        .with_id("t")
        .with_cadence(Cadence::weekly(vec![Weekday::Mon]))
        .with_start_date(start)
}

// ── Fixture (beforeEach analogue) ───────────────────────────────
// A shared in-memory store with fixed `now` (2026-03-01) — each test
// states only its template, seeds, and horizon.

pub(super) struct Fixture {
    pub(super) store: SqliteStore,
    pub(super) now: DateTime<Utc>,
}

pub(super) fn setup() -> Fixture {
    Fixture {
        store: test_store_with_config(default_config()),
        now: utc(2026, 3, 1, 0, 0),
    }
}

impl Fixture {
    pub(super) fn seed_template(&self, template: RecurringTemplate) -> RecurringTemplate {
        seed_template(&self.store, &template);
        template
    }

    fn weekly(&self, id: &str, day: Weekday) -> RecurringTemplate {
        self.seed_template(
            RecurringTemplate::test_default()
                .with_id(id)
                .with_cadence(Cadence::weekly(vec![day]))
                .with_start_date(utc(2026, 3, 2, 0, 0)),
        )
    }

    fn weekly_monday(&self, id: &str) -> RecurringTemplate {
        self.weekly(id, Weekday::Mon)
    }

    /// Seed the "Schedule X" schedule (sched-x) and return `template` edited
    /// with a full identity + sizing change (renamed title, description,
    /// priority, labels, and `schedule_id`; duration bumped to 99). Shared
    /// by the tests that exercise template-edit propagation onto open
    /// instances.
    fn seed_edited_template(&self, template: &RecurringTemplate) -> RecurringTemplate {
        seed_schedule(
            &self.store,
            &Schedule {
                name: "Schedule X".to_owned(),
                ..Schedule::test_default().with_id("sched-x")
            },
        );
        RecurringTemplate {
            title: "Renamed".to_owned(),
            description: Some("new desc".to_owned()),
            duration_minutes: 99,
            priority: Priority::Critical,
            schedule_id: "sched-x".to_owned(),
            labels: vec!["x".to_owned()],
            ..template.clone()
        }
    }

    fn weekend(&self, id: &str) -> RecurringTemplate {
        self.seed_template(
            RecurringTemplate::test_default()
                .with_id(id)
                .with_cadence(
                    Cadence::new(Period::Weekly, 1, vec![Window { start: 5, end: 6 }])
                        .expect("valid weekend cadence"),
                )
                .with_start_date(utc(2026, 3, 2, 0, 0)),
        )
    }

    fn seed(&self, task: &Task) {
        seed_task(&self.store, task);
    }

    fn seed_chunk(&self, chunk: &Chunk) {
        seed_chunk(&self.store, chunk);
    }

    pub(super) fn reconcile_to(&self, template: &RecurringTemplate, horizon: DateTime<Utc>) {
        reconcile(&self.store, template, self.now, horizon, &TZ).expect("reconcile");
    }

    fn auto_cancel(&self, now: DateTime<Utc>) {
        auto_cancel_overdue(&self.store, now).expect("auto_cancel_overdue");
    }

    fn all_tasks(&self) -> Vec<Task> {
        self.store
            .list_tasks(&TaskFilter::default())
            .expect("list_tasks")
    }

    /// `template_id`'s instances, sorted ascending by deadline.
    fn instances(&self, template_id: &str) -> Vec<Task> {
        let mut v: Vec<Task> = self
            .all_tasks()
            .into_iter()
            .filter(|t| t.recurring_template_id.as_deref() == Some(template_id))
            .collect();
        v.sort_by_key(|t| t.deadline);
        v
    }

    /// Deadlines of `template_id`'s instances, ascending.
    pub(super) fn deadlines(&self, template_id: &str) -> Vec<DateTime<Utc>> {
        self.instances(template_id)
            .iter()
            .map(|t| t.deadline.expect("instance has a deadline"))
            .collect()
    }

    fn get(&self, id: &str) -> Task {
        self.store
            .get_task(id)
            .expect("get_task")
            .expect("task exists")
    }

    fn missing(&self, id: &str) -> bool {
        self.store.get_task(id).expect("get_task").is_none()
    }
}

fn instance(id: &str, template_id: &str, status: TaskStatus, deadline: DateTime<Utc>) -> Task {
    Task::test_default()
        .with_id(id)
        .with_template(template_id)
        .with_status(status)
        .with_deadline(deadline)
}

fn recurring_instance(id: &str, template_id: &str, status: TaskStatus) -> Task {
    Task::test_default()
        .with_id(id)
        .with_template(template_id)
        .with_status(status)
}

/// End-of-day UTC deadline for a Y-M-D date.
pub(super) fn eod(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    end_of_day(NaiveDate::from_ymd_opt(year, month, day).unwrap(), TZ).with_timezone(&Utc)
}

/// Panics if no task with this deadline exists — replaces a closure duplicated across `expire_at` tests.
fn find_by_deadline(tasks: &[Task], deadline: DateTime<Utc>) -> &Task {
    tasks
        .iter()
        .find(|t| t.deadline == Some(deadline))
        .unwrap_or_else(|| panic!("missing instance {deadline}"))
}

// ── reconcile: creation / windows ───────────────────────────────

#[test_case(Weekday::Mon, 31, &[2, 9, 16, 23, 30] ; "weekly monday → five mondays")]
#[test_case(Weekday::Fri, 13, &[6, 13] ; "weekly friday → two fridays")]
fn weekly_cadence_creates_expected_days(day: Weekday, horizon_day: u32, days: &[u32]) {
    let f = setup();
    let template = f.weekly("t", day);

    f.reconcile_to(&template, eod(2026, 3, horizon_day));

    let expected: Vec<DateTime<Utc>> = days.iter().map(|d| eod(2026, 3, *d)).collect();
    assert_eq!(f.deadlines("t"), expected);
}

#[test]
fn last_in_window_expire_at_uses_lookahead() {
    let f = setup();
    let template = f.weekly_monday("t");

    // Horizon ends 03-16; the lookahead occurrence is 03-23.
    f.reconcile_to(&template, eod(2026, 3, 16));

    let tasks = f.all_tasks();
    // 03-02 expires at the next window's first day (03-09).
    assert_eq!(
        find_by_deadline(&tasks, eod(2026, 3, 2)).expire_at,
        Some(eod(2026, 3, 9))
    );
    // Last in-window (03-16) expires at the past-horizon lookahead (03-23).
    assert_eq!(
        find_by_deadline(&tasks, eod(2026, 3, 16)).expire_at,
        Some(eod(2026, 3, 23))
    );
}

#[test]
fn multi_day_window_expire_at_bounds_overlap_to_one_day() {
    // A Sat–Sun weekend cadence: a missed weekend must expire at the end of
    // the next weekend's Saturday (its first day), not at the next weekend's
    // Sunday deadline — capping the overdue overlap at a single day.
    let f = setup();
    let template = f.weekend("t");

    // Horizon covers two weekends (03-07/08, 03-14/15); lookahead is 03-21/22.
    f.reconcile_to(&template, eod(2026, 3, 15));

    let tasks = f.all_tasks();
    // Weekend 1 (Sun 03-08) expires at the end of weekend 2's Saturday (03-14).
    assert_eq!(
        find_by_deadline(&tasks, eod(2026, 3, 8)).expire_at,
        Some(eod(2026, 3, 14))
    );
    // Last in-window weekend (Sun 03-15) expires at the lookahead weekend's
    // Saturday (03-21), not its Sunday (03-22).
    assert_eq!(
        find_by_deadline(&tasks, eod(2026, 3, 15)).expire_at,
        Some(eod(2026, 3, 21))
    );
}

#[test]
fn weekend_window_creates_one_instance_spanning_the_weekend() {
    let f = setup();
    let template = f.weekend("t");

    f.reconcile_to(&template, eod(2026, 3, 8));

    let insts = f.instances("t");
    assert_eq!(insts.len(), 1, "one instance for the single weekend window");
    // Schedulable Saturday 00:00 → Sunday 23:59.
    assert_eq!(insts[0].start_date, Some(utc(2026, 3, 7, 0, 0)));
    assert_eq!(insts[0].deadline, Some(eod(2026, 3, 8)));
}

#[test]
fn created_instances_carry_their_window_start_date() {
    // Each weekly instance gets its own day's 00:00 as start_date, so the
    // scheduler can't stack them all onto the first free slot (the pile-up bug).
    let f = setup();
    let template = f.weekly_monday("t");

    f.reconcile_to(&template, eod(2026, 3, 31));

    let starts: Vec<Option<DateTime<Utc>>> =
        f.instances("t").iter().map(|t| t.start_date).collect();
    let expected: Vec<Option<DateTime<Utc>>> = [2, 9, 16, 23, 30]
        .iter()
        .map(|d| Some(utc(2026, 3, *d, 0, 0)))
        .collect();
    assert_eq!(starts, expected);
}

// ── reconcile: idempotence / churn-minimal ──────────────────────

#[test]
fn second_pass_is_idempotent_no_writes() {
    let f = setup();
    let template = f.weekly_monday("t");

    f.reconcile_to(&template, eod(2026, 3, 31));
    let after_first = f.all_tasks();

    f.reconcile_to(&template, eod(2026, 3, 31));
    let after_second = f.all_tasks();

    // Same ids, deadlines, and untouched updated_at (the patch is skipped).
    let key = |ts: &[Task]| {
        let mut v: Vec<(String, DateTime<Utc>, DateTime<Utc>)> = ts
            .iter()
            .map(|t| (t.id.clone(), t.deadline.unwrap(), t.updated_at))
            .collect();
        v.sort();
        v
    };
    assert_eq!(key(&after_first), key(&after_second));
}

#[test]
fn template_content_edits_propagate_to_open_instances() {
    // Bug 019f00bd: editing a template's content must re-sync onto its open
    // instances on the next reconcile (only timing was synced before).
    let f = setup();
    let template = f.weekly_monday("t");
    let horizon = eod(2026, 3, 2); // a single open Monday (03-02)

    f.reconcile_to(&template, horizon);
    let original = f.instances("t");
    assert_eq!(original.len(), 1, "one open instance to reuse");
    let id = original[0].id.clone();

    // Edit every content field (cadence/timing unchanged). schedule_id points
    // at a new schedule, which must exist for the FK on update.
    let edited = f.seed_edited_template(&template);
    f.reconcile_to(&edited, horizon);

    let after = f.instances("t");
    assert_eq!(after.len(), 1, "instance reused, not recreated");
    let inst = &after[0];
    assert_eq!(inst.id, id);
    // Content propagated …
    assert_eq!(inst.title, "Renamed");
    assert_eq!(inst.description, Some("new desc".to_owned()));
    assert_eq!(inst.duration_minutes, 99);
    assert_eq!(
        inst.min_chunk_minutes, 99,
        "instance min-chunk tracks duration"
    );
    assert_eq!(inst.priority, Priority::Critical);
    assert_eq!(inst.schedule_id, "sched-x");
    assert_eq!(inst.labels, vec!["x"]);
    // … while identity, status, and timing are preserved.
    assert_eq!(inst.status, TaskStatus::Pending, "status preserved");
    assert_eq!(inst.deadline, Some(eod(2026, 3, 2)), "timing unchanged");
}

#[test]
fn user_deadline_override_survives_reconcile() {
    // The user extends one instance's deadline within its window. A plain
    // reschedule (template unchanged) must keep that override, not reset it to the
    // cadence deadline — and the other window must still get its own instance.
    let f = setup();
    let template = f.weekly_monday("t");
    let horizon = eod(2026, 3, 9); // two open Mondays (03-02, 03-09)

    f.reconcile_to(&template, horizon);
    let mut overridden = f.instances("t")[0].clone(); // the 03-02 instance
    let id = overridden.id.clone();
    let original_start = overridden.start_date;

    // Push the deadline from Mon 03-02 to Wed 03-04 (still before expire_at 03-09).
    let new_deadline = utc(2026, 3, 4, 12, 0);
    overridden.deadline = Some(new_deadline);
    f.store.update_task(&overridden).expect("apply override");

    f.reconcile_to(&template, horizon);

    let after = f.get(&id);
    assert_eq!(after.deadline, Some(new_deadline), "override preserved");
    assert_eq!(after.start_date, original_start, "start unchanged");
    assert_eq!(f.instances("t").len(), 2, "next window still generated");
}

#[test]
fn horizon_roll_creates_only_tail() {
    let f = setup();
    let template = f.weekly_monday("t");

    f.reconcile_to(&template, eod(2026, 3, 16));
    let original_ids: Vec<String> = f.instances("t").iter().map(|t| t.id.clone()).collect();

    // Roll the horizon forward — only the new tail Mondays are created.
    f.reconcile_to(&template, eod(2026, 3, 31));
    let all = f.instances("t");
    assert_eq!(all.len(), 5, "two tail instances appended");
    for id in &original_ids {
        assert!(
            all.iter().any(|t| &t.id == id),
            "original id {id} preserved"
        );
    }
}

#[test]
fn occurrences_before_now_are_skipped() {
    // Anchor (2020) is years before `now`; reconcile must skip every past
    // occurrence and create only those inside (now, horizon].
    let f = setup();
    let template = f.seed_template(weekly_monday_builder(utc(2020, 1, 6, 0, 0)));

    f.reconcile_to(&template, eod(2026, 3, 16));

    let deadlines = f.deadlines("t");
    assert_eq!(deadlines.len(), 3, "only the three in-window Mondays");
    assert!(deadlines.iter().all(|d| *d > f.now), "none before now");
}

#[test]
fn cadence_edit_keeps_existing_open_instances_only_new_ones_follow() {
    // Contract (reconcile in isolation): reconcile never re-times an existing open
    // instance — generation owns start_date/deadline. A Monday→Wednesday cadence edit
    // leaves three open Monday instances on their old timing; only newly generated
    // instances follow the new cadence.
    //
    // This invariant is what Option 1 relies on: `update_template` deletes stale
    // future open unpinned instances *before* reconcile runs, so reconcile always
    // sees a clean slate after a cadence change. The end-to-end behaviour is tested
    // by `cadence_interval_change_regenerates_from_new_cadence`.
    let f = setup();
    // Template now runs on Wednesdays (anchor Wed 2026-03-04).
    let template = f.seed_template(
        RecurringTemplate::test_default()
            .with_id("t")
            .with_cadence(Cadence::weekly(vec![Weekday::Wed]))
            .with_start_date(utc(2026, 3, 4, 0, 0)),
    );
    // Three open instances aligned to the OLD Monday cadence (start matches deadline).
    f.seed(&Task {
        start_date: Some(utc(2026, 3, 2, 0, 0)),
        ..instance("i0", "t", TaskStatus::Pending, eod(2026, 3, 2))
    });
    f.seed(&Task {
        start_date: Some(utc(2026, 3, 9, 0, 0)),
        ..instance("i1", "t", TaskStatus::Pending, eod(2026, 3, 9))
    });
    f.seed(&Task {
        start_date: Some(utc(2026, 3, 16, 0, 0)),
        ..instance("i2", "t", TaskStatus::Pending, eod(2026, 3, 16))
    });

    f.reconcile_to(&template, eod(2026, 3, 25));

    // The three originals keep their Monday deadlines (and ids) …
    assert_eq!(f.get("i0").deadline, Some(eod(2026, 3, 2)));
    assert_eq!(f.get("i1").deadline, Some(eod(2026, 3, 9)));
    assert_eq!(f.get("i2").deadline, Some(eod(2026, 3, 16)));
    // … and the only new instance lands on the new Wednesday cadence (03-25).
    let all = f.instances("t");
    assert_eq!(all.len(), 4, "three kept + one new window");
    assert!(all.iter().any(|t| t.deadline == Some(eod(2026, 3, 25))));
}

// ── reconcile: sticky instances (closed / pinned) ───────────────

#[test]
fn closed_instance_past_all_desired_consumes_every_slot() {
    // A Completed instance dated after every in-window occurrence consumes
    // all desired slots, so nothing is created.
    let f = setup();
    let template = f.weekly_monday("t");
    f.seed(&instance(
        "done",
        "t",
        TaskStatus::Completed,
        eod(2026, 3, 30),
    ));

    f.reconcile_to(&template, eod(2026, 3, 16));

    let all = f.instances("t");
    assert_eq!(all.len(), 1, "no new instances");
    assert_eq!(all[0].id, "done");
}

#[test_case(TaskStatus::Completed ; "completed consumes its slot")]
#[test_case(TaskStatus::Cancelled ; "cancelled consumes its slot")]
fn closed_instance_consumes_slot_and_is_not_recreated(status: TaskStatus) {
    let f = setup();
    let template = f.weekly_monday("t");
    // 03-02 is closed (history); 03-09 + 03-16 are open and aligned.
    f.seed(&instance("done", "t", status, eod(2026, 3, 2)));
    f.seed(&instance("i1", "t", TaskStatus::Pending, eod(2026, 3, 9)));
    f.seed(&instance("i2", "t", TaskStatus::Pending, eod(2026, 3, 16)));

    f.reconcile_to(&template, eod(2026, 3, 23));

    let tasks = f.all_tasks();
    // Closed instance unchanged, not reversed, not duplicated.
    let on_0302: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.deadline == Some(eod(2026, 3, 2)))
        .collect();
    assert_eq!(on_0302.len(), 1, "no second instance on the closed day");
    assert_eq!(on_0302[0].status, status, "closed status preserved");
    // Open instances still aligned; the tail 03-23 was created.
    assert!(tasks.iter().any(|t| t.deadline == Some(eod(2026, 3, 23))));
    assert_eq!(f.instances("t").len(), 4);
}

#[test]
fn weekly_sticky_deadline_override_does_not_steal_next_slot() {
    // A pinned weekly-Monday instance whose deadline was overridden into the next
    // Monday's span must not consume the next Monday's desired slot. The D1 anchor
    // (start_date of the pinned instance = the first Monday) is earlier than the
    // next Monday's start, so the while loop stops at o=1 and the next Monday
    // still gets its own instance.
    let f = setup();
    let template = f.weekly_monday("t");

    // Seed a pinned instance on the first Monday (03-02) whose deadline was
    // overridden to eod(03-09) — the maximum legal value since expire_at = eod(03-09).
    // With the old comparator (`desired[o].deadline <= T.deadline`), eod(3/9) <= eod(3/9)
    // is true, so the loop would advance o past the 03-09 slot, create no instance for
    // it, and the len==2 assertion below would fail. The anchor fix uses T.start_date
    // (03-02) instead: utc(3/9) > utc(3/2), so the loop stops and the slot is preserved.
    let base = Task::test_default()
        .with_id("pinned-03-02")
        .with_template("t")
        .with_status(TaskStatus::Pending)
        .with_deadline(eod(2026, 3, 9));
    f.seed(&Task {
        start_date: Some(utc(2026, 3, 2, 0, 0)),
        is_pinned: true,
        ..base
    });

    reconcile(&f.store, &template, f.now, eod(2026, 3, 9), &TZ).expect("reconcile");

    let insts = f.instances("t");
    assert_eq!(insts.len(), 2, "pinned + a new instance for the 2nd Monday");
    assert!(
        insts.iter().any(|t| t.id == "pinned-03-02"),
        "pinned instance preserved"
    );
    assert!(
        insts
            .iter()
            .any(|t| t.id != "pinned-03-02" && t.start_date == Some(utc(2026, 3, 9, 0, 0))),
        "new instance for 03-09 with correct start_date"
    );
}

#[test]
fn surplus_open_instances_are_deleted_with_chunks() {
    let f = setup();
    let template = f.weekly_monday("t");
    // Five open Mondays exist, but the horizon only covers three.
    for (idx, day) in [2, 9, 16, 23, 30].iter().enumerate() {
        f.seed(&instance(
            &format!("i{idx}"),
            "t",
            TaskStatus::Pending,
            eod(2026, 3, *day),
        ));
    }
    // Attach a chunk to a surplus instance (03-30 → "i4").
    f.seed_chunk(
        &Chunk::test_default()
            .with_id("c4")
            .with_task("i4")
            .with_status(ChunkStatus::Scheduled),
    );

    f.reconcile_to(&template, eod(2026, 3, 18));

    assert_eq!(f.instances("t").len(), 3, "two surplus deleted");
    assert!(
        f.store.get_chunk("c4").unwrap().is_none(),
        "surplus instance's chunk deleted"
    );
}

#[test]
fn pinned_instance_is_sticky_and_consumes_its_slot() {
    // A user-pinned instance on a cadence day keeps its id/deadline and
    // consumes that desired slot, so no duplicate is created for the day.
    let f = setup();
    let template = f.weekly_monday("t"); // Mondays 03-02, 03-09
    f.seed(&instance("pinned", "t", TaskStatus::Scheduled, eod(2026, 3, 2)).with_pinned(true));

    f.reconcile_to(&template, eod(2026, 3, 9));

    // 03-02 slot consumed by the pinned instance; only 03-09 is newly created.
    assert_eq!(f.deadlines("t"), vec![eod(2026, 3, 2), eod(2026, 3, 9)]);
    let on_0302: Vec<Task> = f
        .instances("t")
        .into_iter()
        .filter(|t| t.deadline == Some(eod(2026, 3, 2)))
        .collect();
    assert_eq!(on_0302.len(), 1, "no duplicate on the pinned day");
    assert_eq!(on_0302[0].id, "pinned");
    assert!(on_0302[0].is_pinned, "still pinned, deadline untouched");
}

#[test]
fn pinned_surplus_instance_is_not_deleted() {
    // With no desired slots, a plain open instance is deleted as surplus,
    // but a pinned one survives unchanged.
    let f = setup();
    let template = f.weekly_monday("t");
    f.seed(&instance(
        "plain",
        "t",
        TaskStatus::Pending,
        eod(2026, 3, 9),
    ));
    f.seed(&instance("pinned", "t", TaskStatus::Scheduled, eod(2026, 3, 16)).with_pinned(true));

    // Horizon before the first Monday → zero desired occurrences.
    f.reconcile_to(&template, eod(2026, 3, 1));

    assert!(f.missing("plain"), "plain surplus deleted");
    let pinned = f.get("pinned");
    assert!(pinned.is_pinned);
    assert_eq!(pinned.deadline, Some(eod(2026, 3, 16)), "pinned kept as-is");
}

// ── reconcile: template-edit propagation matrix (B3) ───────────

/// Which half of a template edit reaches an instance, by state: open unpinned
/// instances receive identity + sizing, pinned receive identity only (timing
/// and sizing stay frozen), closed history receives neither — including a
/// pinned completed instance, where closed wins.
#[test_case(TaskStatus::Pending,   false, true,  true  ; "open unpinned gets identity and sizing")]
#[test_case(TaskStatus::Scheduled, true,  true,  false ; "pinned gets identity only")]
#[test_case(TaskStatus::Completed, false, false, false ; "completed gets neither")]
#[test_case(TaskStatus::Cancelled, false, false, false ; "cancelled gets neither")]
#[test_case(TaskStatus::Completed, true,  false, false ; "pinned completed is closed history")]
fn template_edit_propagation_matrix(
    status: TaskStatus,
    pinned: bool,
    expect_identity: bool,
    expect_sizing: bool,
) {
    let f = setup();
    let template = f.weekly_monday("t");
    // A single desired Monday (03-02); one instance aligned to it, in the
    // given state.
    let horizon = eod(2026, 3, 2);
    f.seed(&Task {
        start_date: Some(utc(2026, 3, 2, 0, 0)),
        ..instance("i", "t", status, eod(2026, 3, 2)).with_pinned(pinned)
    });
    // Edit identity (title/description/priority/labels/schedule) and sizing
    // (duration). The new schedule must exist for the FK on update.
    let edited = f.seed_edited_template(&template);

    f.reconcile_to(&edited, horizon);

    let insts = f.instances("t");
    assert_eq!(insts.len(), 1, "slot consumed, nothing recreated");
    let after = &insts[0];
    assert_eq!(after.id, "i", "instance reused, not recreated");
    // Identity half.
    if expect_identity {
        assert_eq!(after.title, "Renamed");
        assert_eq!(after.description, Some("new desc".to_owned()));
        assert_eq!(after.priority, Priority::Critical);
        assert_eq!(after.labels, vec!["x"]);
        assert_eq!(after.schedule_id, "sched-x");
    } else {
        assert_eq!(after.title, "Test task", "identity untouched");
        assert_eq!(after.priority, Priority::Medium);
        assert_eq!(after.schedule_id, "default-schedule-id");
    }
    // Sizing half.
    if expect_sizing {
        assert_eq!(after.duration_minutes, 99);
        assert_eq!(after.min_chunk_minutes, 99);
        assert!(after.no_split);
    } else {
        assert_eq!(after.duration_minutes, 60, "sizing untouched");
        assert_eq!(after.min_chunk_minutes, 30);
        assert!(!after.no_split);
    }
    // Timing and progress are never template-owned.
    assert_eq!(after.deadline, Some(eod(2026, 3, 2)), "timing intact");
    assert_eq!(after.status, status, "status preserved");
    assert_eq!(after.is_pinned, pinned, "pin preserved");
}

#[test]
fn pinned_identity_refresh_writes_once() {
    // The pinned-identity patch is skipped when nothing changed: a second
    // reconcile after the same edit leaves updated_at untouched ‹D2›.
    let f = setup();
    let template = f.weekly_monday("t");
    let horizon = eod(2026, 3, 2);
    f.seed(&instance("i", "t", TaskStatus::Scheduled, eod(2026, 3, 2)).with_pinned(true));
    let edited = RecurringTemplate {
        title: "Renamed".to_owned(),
        ..template.clone()
    };

    f.reconcile_to(&edited, horizon);
    let first = f.get("i");
    assert_eq!(
        first.title, "Renamed",
        "identity reached the pinned instance"
    );

    f.reconcile_to(&edited, horizon);
    let second = f.get("i");
    assert_eq!(second.updated_at, first.updated_at, "no second write");
}

// ── reconcile: in-place backfill / recompute ────────────────────

#[test]
fn backfills_missing_start_date_without_moving_deadline() {
    // Upgrade path: an instance from before the windows model has the right
    // deadline but no start_date. Reconcile fills it in place (same id,
    // deadline unchanged) rather than deleting and recreating it.
    let f = setup();
    let template = f.weekly_monday("t");
    let seeded = instance("legacy", "t", TaskStatus::Pending, eod(2026, 3, 2));
    assert!(seeded.start_date.is_none(), "precondition: no start_date");
    f.seed(&seeded);

    // Horizon covers only the first Monday → one desired occurrence (03-02).
    f.reconcile_to(&template, eod(2026, 3, 2));

    let insts = f.instances("t");
    assert_eq!(insts.len(), 1, "reused in place, not recreated");
    assert_eq!(insts[0].id, "legacy");
    assert_eq!(
        insts[0].deadline,
        Some(eod(2026, 3, 2)),
        "deadline unchanged"
    );
    assert_eq!(
        insts[0].start_date,
        Some(utc(2026, 3, 2, 0, 0)),
        "start_date backfilled to the window start"
    );
}

#[test]
fn recomputes_stale_expire_at_in_place() {
    // deadline + start_date already match the occurrence; only expire_at is
    // stale. Reconcile patches it in place (same id, deadline/start unchanged).
    let f = setup();
    let template = f.weekly_monday("t");
    let stale = Task {
        start_date: Some(utc(2026, 3, 2, 0, 0)),
        expire_at: Some(eod(2026, 3, 1)), // wrong — predates the real next deadline
        ..instance("stale", "t", TaskStatus::Pending, eod(2026, 3, 2))
    };
    f.seed(&stale);

    f.reconcile_to(&template, eod(2026, 3, 9));

    let after = f.get("stale");
    assert_eq!(after.deadline, Some(eod(2026, 3, 2)), "deadline unchanged");
    assert_eq!(
        after.start_date,
        Some(utc(2026, 3, 2, 0, 0)),
        "start unchanged"
    );
    assert_eq!(
        after.expire_at,
        Some(eod(2026, 3, 9)),
        "expire_at recomputed to the next window's first day"
    );
}

// ── reconcile: deactivation ─────────────────────────────────────

#[test]
fn deactivated_template_deletes_open_unpinned_keeps_closed_and_pinned() {
    // Deactivating a template drops its open, unpinned instances (and their
    // chunks) but preserves closed history and any the user has pinned.
    let f = setup();
    let template = f.seed_template(weekly_monday_builder(utc(2026, 3, 2, 0, 0)).with_active(false));
    f.seed(&instance("open", "t", TaskStatus::Pending, eod(2026, 3, 9)));
    f.seed(&instance(
        "scheduled",
        "t",
        TaskStatus::Scheduled,
        eod(2026, 3, 16),
    ));
    f.seed(&instance(
        "completed",
        "t",
        TaskStatus::Completed,
        eod(2026, 3, 2),
    ));
    f.seed(&instance(
        "cancelled",
        "t",
        TaskStatus::Cancelled,
        eod(2026, 3, 2),
    ));
    f.seed(&instance("pinned", "t", TaskStatus::Scheduled, eod(2026, 3, 23)).with_pinned(true));
    f.seed_chunk(
        &Chunk::test_default()
            .with_id("c-open")
            .with_task("open")
            .with_status(ChunkStatus::Scheduled),
    );

    f.reconcile_to(&template, eod(2026, 3, 31));

    // Open + scheduled (unpinned) deleted; closed + pinned kept; nothing created.
    assert!(f.missing("open"), "open unpinned deleted");
    assert!(f.missing("scheduled"), "scheduled unpinned deleted");
    assert!(
        f.store.get_chunk("c-open").unwrap().is_none(),
        "deleted instance's chunk removed"
    );
    let surviving: Vec<String> = f.instances("t").iter().map(|t| t.id.clone()).collect();
    assert_eq!(surviving.len(), 3, "completed, cancelled, pinned survive");
    for id in ["completed", "cancelled", "pinned"] {
        assert!(surviving.contains(&id.to_owned()), "{id} kept");
    }
}

// ── instance_from_template ──────────────────────────────────────

#[test]
fn instance_from_template_inherits_template_shape() {
    let template = RecurringTemplate::test_default()
        .with_id("t")
        .with_cadence(Cadence::monthly(10));
    let now = utc(2026, 5, 1, 0, 0);
    let start = utc(2026, 5, 10, 0, 0);
    let task = instance_from_template(
        &template,
        "id-1".to_owned(),
        start,
        eod(2026, 5, 10),
        None,
        now,
    );

    assert_eq!(task.id, "id-1");
    assert_eq!(task.title, template.title);
    assert_eq!(task.duration_minutes, template.duration_minutes);
    assert_eq!(task.min_chunk_minutes, template.duration_minutes);
    assert!(task.no_split);
    assert!(!task.is_pinned, "new instances are never pinned");
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.recurring_template_id, Some("t".to_owned()));
    assert_eq!(task.start_date, Some(start));
    assert_eq!(task.time_logged_minutes, 0);
    assert_eq!(task.deadline, Some(eod(2026, 5, 10)));
    assert!(task.expire_at.is_none());
}

// ── auto_cancel_overdue ─────────────────────────────────────────

// Cancellation triggers strictly when `now > expire_at`.
#[test_case(chrono::Duration::seconds(-1), true  ; "expire_at just passed -> cancelled")]
#[test_case(chrono::Duration::seconds(0),  false ; "now == expire_at -> kept")]
#[test_case(chrono::Duration::seconds(1),  false ; "expire_at in future -> kept")]
fn cancel_when_now_past_expire_at(expire_offset: chrono::Duration, should_cancel: bool) {
    let f = setup();
    let now = utc(2026, 3, 22, 12, 0);
    f.seed(
        &recurring_instance("task-x", "tmpl-x", TaskStatus::Pending)
            .with_expire_at(now + expire_offset),
    );

    f.auto_cancel(now);

    let expected = if should_cancel {
        TaskStatus::Cancelled
    } else {
        TaskStatus::Pending
    };
    assert_eq!(f.get("task-x").status, expected);
}

#[test]
fn pinned_instance_never_auto_cancels() {
    // A user-pinned instance is exempt from auto-cancellation even when its
    // expire_at is well in the past.
    let f = setup();
    let now = utc(2026, 3, 22, 12, 0);
    f.seed(
        &recurring_instance("pinned", "tmpl-x", TaskStatus::Scheduled)
            .with_expire_at(now - chrono::Duration::days(2))
            .with_pinned(true),
    );

    f.auto_cancel(now);

    assert_eq!(
        f.get("pinned").status,
        TaskStatus::Scheduled,
        "pinned instance is not cancelled"
    );
}

#[test]
fn cancels_without_template_lookup() {
    // `expire_at` is denormalized — cancellation reads only `expire_at`,
    // never the template. The genuine post-deletion production state is
    // `recurring_template_id = NULL` (ON DELETE SET NULL), at which point
    // `auto_cancel_overdue` skips the task (it filters for recurring instances
    // only — see `non_recurring_task_ignored`). We reach that state by seeding
    // normally (auto-creates the parent), then deleting the template row.
    use crate::traits::storage::RecurringTemplateStore;
    let f = setup();
    let now = utc(2026, 3, 22, 12, 0);

    // seed auto-creates the "tmpl-deleted" template parent.
    f.seed(
        &recurring_instance("task-orphan", "tmpl-deleted", TaskStatus::Pending)
            .with_expire_at(now - chrono::Duration::days(1)),
    );
    // Delete the template row directly — ON DELETE SET NULL nullifies the
    // task's recurring_template_id, making it a plain task.
    f.store
        .delete_template("tmpl-deleted")
        .expect("delete template");

    f.auto_cancel(now);

    assert_eq!(
        f.get("task-orphan").status,
        TaskStatus::Pending,
        "after FK nullification the task is no longer a recurring instance"
    );
}

#[test]
fn chunks_deleted_on_cancel() {
    let f = setup();
    let now = utc(2026, 3, 22, 12, 0);
    f.seed(
        &recurring_instance("task-chunks", "tmpl-chunks", TaskStatus::Scheduled)
            .with_expire_at(now - chrono::Duration::days(1)),
    );
    f.seed_chunk(
        &Chunk::test_default()
            .with_id("chunk-sched")
            .with_task("task-chunks")
            .with_status(ChunkStatus::Scheduled),
    );
    f.seed_chunk(
        &Chunk::test_default()
            .with_id("chunk-done")
            .with_task("task-chunks")
            .with_status(ChunkStatus::Completed),
    );

    f.auto_cancel(now);

    assert!(
        f.store.get_chunk("chunk-sched").unwrap().is_none(),
        "scheduled chunk should be deleted"
    );
    assert!(
        f.store.get_chunk("chunk-done").unwrap().is_some(),
        "completed chunk should be preserved"
    );
    assert_eq!(f.get("task-chunks").status, TaskStatus::Cancelled);
}

#[test]
fn no_expire_at_skipped() {
    let f = setup();
    let now = utc(2026, 3, 22, 12, 0);
    // expire_at defaults to None — never auto-cancels.
    f.seed(&recurring_instance(
        "task-nd",
        "tmpl-nd",
        TaskStatus::Pending,
    ));

    f.auto_cancel(now);

    assert_eq!(
        f.get("task-nd").status,
        TaskStatus::Pending,
        "instance without expire_at never auto-cancels"
    );
}

#[test]
fn non_recurring_task_ignored() {
    let f = setup();
    let now = utc(2026, 3, 22, 12, 0);
    // Non-recurring (no template link) even with a long-passed expire_at.
    f.seed(
        &Task::test_default()
            .with_id("task-nonrec")
            .with_status(TaskStatus::Pending)
            .with_expire_at(now - chrono::Duration::days(30)),
    );

    f.auto_cancel(now);

    assert_eq!(
        f.get("task-nonrec").status,
        TaskStatus::Pending,
        "non-recurring task is never auto-cancelled"
    );
}

mod cadence_change;
mod monthly;
