// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests for cadence/anchor changes driven through `update_template`.

use chrono::{DateTime, Utc, Weekday};
use test_case::test_case;

use super::{eod, setup};
use crate::domain::cadence::Cadence;
use crate::domain::inputs::UpdateTemplateInput;
use crate::domain::models::RecurringTemplate;
use crate::services::recurring::update_template;
use crate::test_support::utc;

// ── cadence-change end-to-end (via update_template) ─────────────

/// Changing a template's cadence interval deletes future open unpinned instances
/// so the following reconcile regenerates from the new cadence — no gap, no duplicate.
///
/// Pattern: `reconcile` / `update_template` (deletes stale) / `reconcile`.
/// Driving reconcile twice with `update_template` between is what exercises the fix
/// (Option 1: cadence-change detection lives in `update_template`, not in `reconcile`).
#[test_case(
    Cadence::weekly_every(4, vec![Weekday::Mon]),
    Cadence::weekly(vec![Weekday::Mon]),
    eod(2026, 3, 30),
    eod(2026, 3, 30),
    vec![eod(2026, 3, 2), eod(2026, 3, 30)],
    vec![eod(2026, 3, 2), eod(2026, 3, 9), eod(2026, 3, 16), eod(2026, 3, 23), eod(2026, 3, 30)]
    ; "4-weekly to weekly: five Mondays, no gap"
)]
#[test_case(
    Cadence::weekly(vec![Weekday::Mon]),
    Cadence::weekly_every(4, vec![Weekday::Mon]),
    eod(2026, 3, 23),
    eod(2026, 3, 30),
    vec![eod(2026, 3, 2), eod(2026, 3, 9), eod(2026, 3, 16), eod(2026, 3, 23)],
    vec![eod(2026, 3, 2), eod(2026, 3, 30)]
    ; "weekly to 4-weekly: two slots, old weekly cleared"
)]
#[allow(clippy::needless_pass_by_value)] // Vec params required by test_case literal syntax
fn cadence_interval_change_regenerates_from_new_cadence(
    old_cadence: Cadence,
    new_cadence: Cadence,
    initial_horizon: DateTime<Utc>,
    final_horizon: DateTime<Utc>,
    initial_expected: Vec<DateTime<Utc>>,
    final_expected: Vec<DateTime<Utc>>,
) {
    let f = setup();
    let template = f.seed_template(
        RecurringTemplate::test_default()
            .with_id("t")
            .with_cadence(old_cadence)
            .with_start_date(utc(2026, 3, 2, 0, 0)),
    );

    f.reconcile_to(&template, initial_horizon);
    assert_eq!(
        f.deadlines("t"),
        initial_expected,
        "precondition: old cadence instances"
    );

    let updated = update_template(
        &f.store,
        "t",
        UpdateTemplateInput {
            cadence: Some(new_cadence),
            ..UpdateTemplateInput::default()
        },
        f.now,
    )
    .expect("update_template");

    f.reconcile_to(&updated, final_horizon);
    assert_eq!(
        f.deadlines("t"),
        final_expected,
        "post-change: new cadence instances only"
    );
}
