// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Tests specific to the monthly-instance visibility fix: widened deadlines,
//! reuse-path deadline refresh, expiry timing, and DST correctness.

use chrono::Duration;

use crate::domain::cadence::{Cadence, Period, Window};
use crate::domain::enums::TaskStatus;
use crate::domain::models::{RecurringTemplate, Task};
use crate::test_support::utc;

use super::super::{auto_cancel_overdue, reconcile};
use super::{eod, find_by_deadline, setup, TZ};

/// Seed a monthly(25) template anchored at `anchor` with id `"t"`.
fn seed_monthly_25(f: &super::Fixture, anchor: (i32, u32, u32)) -> RecurringTemplate {
    f.seed_template(
        RecurringTemplate::test_default()
            .with_id("t")
            .with_cadence(Cadence::monthly(25))
            .with_start_date(utc(anchor.0, anchor.1, anchor.2, 0, 0)),
    )
}

/// Seed a pre-widening monthly instance with narrow deadline (eod 3/25) and
/// `start_date` = 3/25. Used in sticky (D1) and reuse (D2) tests to represent an
/// instance created before this fix.
fn seed_pre_fix_instance(f: &super::Fixture, id: &str, status: TaskStatus) {
    let base = Task::test_default()
        .with_id(id)
        .with_template("t")
        .with_status(status)
        .with_deadline(eod(2026, 3, 25));
    f.seed(&Task {
        start_date: Some(utc(2026, 3, 25, 0, 0)),
        ..base
    });
}

#[test]
fn monthly_sticky_closed_consumes_slot_no_duplicate() {
    // A closed monthly instance with the pre-widening narrow deadline (25th) must
    // still consume the occurrence slot so reconcile does not create a second
    // Pending instance alongside it. The D1 anchor (start_date = 25th) is stable
    // under deadline widening; matching desired[0].start (25th) against the anchor
    // correctly advances o past it.
    let f = setup();
    let template = seed_monthly_25(&f, (2026, 3, 1));
    seed_pre_fix_instance(&f, "done", TaskStatus::Cancelled);

    // now = 3/10: before the 25th so the closed instance is still in `inst`.
    reconcile(
        &f.store,
        &template,
        utc(2026, 3, 10, 0, 0),
        eod(2026, 3, 31),
        &TZ,
    )
    .expect("reconcile");

    let all = f.instances("t");
    assert_eq!(
        all.len(),
        1,
        "slot consumed by closed instance — no duplicate"
    );
    assert_eq!(all[0].id, "done", "the closed instance, unchanged");
    assert_eq!(all[0].status, TaskStatus::Cancelled);
}

#[test]
fn monthly_reuse_path_widens_stored_deadline() {
    // When an open instance holds the pre-widening narrow deadline (25th) and
    // now < 25th, the D2 reuse path must update the stored deadline to the
    // occurrence's widened value (28th), not preserve the old stored deadline.
    let f = setup();
    let template = seed_monthly_25(&f, (2026, 3, 1));
    // Seed a realistic pre-fix instance: start_date = nominal day, narrow deadline.
    seed_pre_fix_instance(&f, "inst-old", TaskStatus::Pending);

    // now = 3/10: before the 25th so the Pending instance is still in `inst`.
    reconcile(
        &f.store,
        &template,
        utc(2026, 3, 10, 0, 0),
        eod(2026, 3, 31),
        &TZ,
    )
    .expect("reconcile");

    let insts = f.instances("t");
    assert_eq!(insts.len(), 1, "reused in place, not recreated");
    assert_eq!(
        insts[0].id, "inst-old",
        "same instance id confirms D2 reuse"
    );
    assert_eq!(
        insts[0].start_date,
        Some(utc(2026, 3, 25, 0, 0)),
        "start_date preserved by D2 reuse"
    );
    assert_eq!(
        insts[0].deadline,
        Some(eod(2026, 3, 28)),
        "deadline widened to 28th"
    );
    assert_eq!(
        insts[0].expire_at,
        Some(eod(2026, 3, 28)),
        "expire_at refreshed"
    );
}

#[test]
fn monthly_instance_visible_day_after_nominal_day() {
    // With the old single-day window, calling reconcile with now = 2026-03-26
    // would skip the March-25th occurrence (deadline <= now) and create no
    // instance. The widened deadline (28th) must be > now so the occurrence
    // remains desired and an instance is created.
    let f = setup();
    let template = seed_monthly_25(&f, (2026, 3, 25));
    let now = utc(2026, 3, 26, 0, 0);
    reconcile(&f.store, &template, now, eod(2026, 3, 31), &TZ).expect("reconcile");

    let insts = f.instances("t");
    assert_eq!(
        insts.len(),
        1,
        "instance not dropped on the day after the 25th"
    );
    assert_eq!(
        insts[0].deadline,
        Some(eod(2026, 3, 28)),
        "deadline is 28th"
    );
    assert_eq!(insts[0].status, TaskStatus::Pending);
}

#[test]
fn monthly_expire_at_equals_widened_deadline() {
    // expire_at must equal the widened deadline (28th), not the next month's
    // first day (~30 days out), so auto_cancel fires promptly at month-end.
    let f = setup();
    let template = seed_monthly_25(&f, (2026, 3, 25));
    f.reconcile_to(&template, eod(2026, 3, 31));

    let tasks = f.all_tasks();
    let inst = find_by_deadline(&tasks, eod(2026, 3, 28));
    assert_eq!(
        inst.expire_at,
        Some(eod(2026, 3, 28)),
        "expire_at is end of widened span"
    );
}

#[test]
fn monthly_auto_cancel_fires_after_widened_deadline() {
    // The instance must survive within the span and be cancelled promptly after.
    let f = setup();
    let template = seed_monthly_25(&f, (2026, 3, 25));
    f.reconcile_to(&template, eod(2026, 3, 31));

    auto_cancel_overdue(&f.store, utc(2026, 3, 28, 12, 0)).expect("auto_cancel");
    assert_eq!(
        f.instances("t")[0].status,
        TaskStatus::Pending,
        "still pending within the widened span"
    );

    auto_cancel_overdue(&f.store, utc(2026, 3, 29, 0, 1)).expect("auto_cancel");
    assert_eq!(
        f.instances("t")[0].status,
        TaskStatus::Cancelled,
        "cancelled after widened deadline"
    );
}

#[test]
fn monthly_deadline_survives_dst_transition() {
    // US/Eastern springs forward on 2026-03-08. monthly(1) anchors on March 1
    // (UTC-5, EST), so its start spans the transition; the widened deadline
    // falls on March 28 (UTC-4, EDT). An implementation that reads the offset
    // at start-time would compute the wrong UTC for the deadline.
    use chrono_tz::America::New_York;
    let f = setup();
    let template = f.seed_template(
        RecurringTemplate::test_default()
            .with_id("t")
            .with_cadence(Cadence::monthly(1))
            .with_start_date(utc(2026, 3, 1, 0, 0)),
    );
    reconcile(
        &f.store,
        &template,
        f.now,
        utc(2026, 3, 31, 23, 59),
        &New_York,
    )
    .expect("reconcile");

    let insts = f.instances("t");
    assert_eq!(insts.len(), 1, "one instance for March");
    // start = 2026-03-01 00:00:00 EST = 2026-03-01 05:00:00 UTC
    assert_eq!(
        insts[0].start_date,
        Some(utc(2026, 3, 1, 5, 0)),
        "start is midnight EST"
    );
    // Widened deadline = 2026-03-28 23:59:59 EDT (UTC-4) = 2026-03-29 03:59:59 UTC.
    assert_eq!(
        insts[0].deadline,
        Some(utc(2026, 3, 29, 3, 59) + Duration::seconds(59)),
        "deadline is eod(28th) in Eastern time, accounting for spring-forward"
    );
}

#[test]
fn monthly_multi_window_deadlines_and_expiry() {
    // A 2-window monthly cadence ({0,0} day-1 and {14,14} day-15):
    // first window extends to day 14 (before the next window starts at 15);
    // second window extends to the guaranteed max (28th).
    let f = setup();
    let template = f.seed_template(
        RecurringTemplate::test_default()
            .with_id("t")
            .with_cadence(
                Cadence::new(
                    Period::Monthly,
                    1,
                    vec![Window { start: 0, end: 0 }, Window { start: 14, end: 14 }],
                )
                .expect("valid"),
            )
            .with_start_date(utc(2026, 3, 1, 0, 0)),
    );
    f.reconcile_to(&template, eod(2026, 3, 31));

    assert_eq!(
        f.deadlines("t"),
        vec![eod(2026, 3, 14), eod(2026, 3, 28)],
        "first window ends at 14th, second at 28th"
    );

    let tasks = f.all_tasks();
    assert_eq!(
        find_by_deadline(&tasks, eod(2026, 3, 14)).expire_at,
        Some(eod(2026, 3, 14)),
        "first window expires at its own deadline"
    );
    assert_eq!(
        find_by_deadline(&tasks, eod(2026, 3, 28)).expire_at,
        Some(eod(2026, 3, 28)),
        "second window expires at its own deadline"
    );
}
