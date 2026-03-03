// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Recurrence [`Cadence`] — a uniform "interval + in-period day windows" model.
//!
//! A cadence is the recurrence *rule*, independent of when a template starts;
//! [`Cadence::occurrences`] expands it against a separate anchor (`start_date`),
//! mirroring iCalendar's RRULE/DTSTART split. Each [`Window`] is a contiguous
//! span of in-period days and yields exactly one occurrence per active period —
//! a task schedulable anywhere between the window's first-day start and its
//! last-day deadline. Weekly and monthly differ only in how a period is located
//! and advanced ([`Period`]); window resolution and the anchor filter are
//! uniform across both.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::date_utils::{end_of_day, start_of_day, start_of_month, start_of_week};
use crate::error::AppError;

const DAYS_PER_WEEK: i64 = 7;

/// A single generated occurrence: the schedulable span of one [`Window`] in one
/// active period.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occurrence {
    /// Earliest placement — 00:00 (local) of the window's first day, in UTC.
    /// Clamped up to the anchor for a window that straddles it.
    pub start: DateTime<Utc>,
    /// Deadline — 23:59:59 (local) of the window's last day, in UTC.
    pub deadline: DateTime<Utc>,
}

/// A contiguous range of in-period day offsets, inclusive and 0-indexed from the
/// period's first day (Monday for weekly, the 1st for monthly). One window
/// produces one instance per active period, schedulable across `start..=end`.
///
/// A window has no standalone invariant; it is validated only as part of a
/// [`Cadence`] (in range, sorted, non-overlapping — see [`Cadence::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub start: u8,
    pub end: u8,
}

/// The base recurrence period — the only thing that varies between cadences.
///
/// Carries no data: it is the discriminant that locates and advances periods.
/// Window resolution and the anchor filter are uniform across kinds (see
/// [`Cadence::occurrences`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    Weekly,
    Monthly,
}

impl Period {
    /// Locates the base period containing this date.
    fn floor(self, date: NaiveDate) -> NaiveDate {
        match self {
            Period::Weekly => start_of_week(date),
            Period::Monthly => start_of_month(date, 0),
        }
    }

    /// Start of the next active period, `interval` base periods after `period_start`.
    fn next(self, period_start: NaiveDate, interval: u8) -> NaiveDate {
        match self {
            Period::Weekly => period_start + Duration::days(DAYS_PER_WEEK * i64::from(interval)),
            Period::Monthly => start_of_month(period_start, u32::from(interval)),
        }
    }

    /// Largest valid in-period day offset (0-indexed from the period's first day).
    ///
    /// Weekly spans 7 days (`0..=6`). Monthly is capped at the 28th (`0..=27`) —
    /// the last day every month is guaranteed to have — so an offset always
    /// resolves to a real date with no clamping or month-skipping.
    fn max_offset(self) -> u8 {
        match self {
            Period::Weekly => 6,
            Period::Monthly => 27,
        }
    }
}

/// Recurrence cadence: a base [`Period`], an `interval` multiplier over it, and
/// the in-period day [`Window`]s to schedule (one instance per window).
///
/// Windows are sorted by start and non-overlapping. Construction — and
/// deserialization, via `RawCadence` — validates and canonicalizes them, so an
/// invalid `Cadence` is unrepresentable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawCadence")]
pub struct Cadence {
    period: Period,
    interval: u8,
    windows: Vec<Window>,
}

/// Unvalidated mirror of [`Cadence`] for `serde` deserialization, routed through
/// the validating [`Cadence::new`] so a deserialized cadence is always valid.
#[derive(Deserialize)]
struct RawCadence {
    period: Period,
    interval: u8,
    windows: Vec<Window>,
}

impl TryFrom<RawCadence> for Cadence {
    type Error = AppError;

    fn try_from(raw: RawCadence) -> Result<Self, AppError> {
        Cadence::new(raw.period, raw.interval, raw.windows)
    }
}

impl Cadence {
    /// Build a validated cadence. Windows are sorted by `(start, end)`.
    ///
    /// # Errors
    ///
    /// [`AppError::Validation`] if `interval == 0`, `windows` is empty, any
    /// window has `start > end` or `end` past the period maximum (`6` weekly,
    /// `27` monthly), or two windows overlap.
    pub fn new(period: Period, interval: u8, mut windows: Vec<Window>) -> Result<Self, AppError> {
        if interval == 0 {
            return Err(AppError::Validation(
                "cadence interval must be at least 1".to_owned(),
            ));
        }
        if windows.is_empty() {
            return Err(AppError::Validation(
                "cadence must have at least one window".to_owned(),
            ));
        }
        let max = period.max_offset();
        for w in &windows {
            if w.start > w.end {
                return Err(AppError::Validation(format!(
                    "cadence window start {} is after its end {}",
                    w.start, w.end
                )));
            }
            if w.end > max {
                return Err(AppError::Validation(format!(
                    "cadence window day {} exceeds the maximum {max} for this period",
                    w.end
                )));
            }
        }
        windows.sort_unstable_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
        for pair in windows.windows(2) {
            if pair[0].end >= pair[1].start {
                return Err(AppError::Validation(
                    "cadence windows must not overlap".to_owned(),
                ));
            }
        }
        Ok(Self {
            period,
            interval,
            windows,
        })
    }

    #[must_use]
    pub fn period(&self) -> Period {
        self.period
    }

    /// The in-period day windows (ascending by start, non-overlapping).
    #[must_use]
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Lazy ascending stream of occurrences from `start_date` (the anchor) forward.
    ///
    /// The iterator is infinite — callers must bound consumption (`take`,
    /// `take_while`, `find`). It never yields a deadline before `start_date`; a
    /// window straddling the anchor has its `start` clamped up to it.
    ///
    /// **Monthly extension**: each monthly window's schedulable span is widened to
    /// the last guaranteed day of the period (offset 27 / the 28th), so a task
    /// missed on its nominal day stays schedulable within the month rather than
    /// disappearing. The extension stops at the next window's start minus one day
    /// when multiple windows share the period, preventing overlap.
    pub fn occurrences(
        &self,
        start_date: DateTime<Utc>,
        tz: Tz,
    ) -> impl Iterator<Item = Occurrence> {
        let period = self.period;
        let interval = self.interval;
        let windows = self.windows.clone();
        let mut period_start = period.floor(start_date.date_naive());
        let mut idx = 0usize;

        std::iter::from_fn(move || loop {
            if idx >= windows.len() {
                period_start = period.next(period_start, interval);
                idx = 0;
            }
            let window = windows[idx];
            idx += 1;

            let first = period_start + Duration::days(i64::from(window.start));
            let effective_end = match period {
                Period::Monthly => {
                    // Extend to fill the gap to the next window (or the period max),
                    // so a missed occurrence stays visible within the month.
                    let ceiling = if idx < windows.len() {
                        windows[idx].start.saturating_sub(1)
                    } else {
                        period.max_offset()
                    };
                    // `window.end <= ceiling` is guaranteed: new() rejects end > 27
                    // for the last window, and non-overlap forces
                    // `windows[idx].start >= window.end + 1` for the others, so
                    // `ceiling = windows[idx].start - 1 >= window.end`.
                    debug_assert!(
                        window.end <= ceiling,
                        "window end {window_end} exceeds ceiling {ceiling}",
                        window_end = window.end
                    );
                    ceiling
                }
                Period::Weekly => window.end,
            };
            let last = period_start + Duration::days(i64::from(effective_end));
            let deadline = end_of_day(last, tz).with_timezone(&Utc);
            if deadline < start_date {
                continue;
            }
            let start = start_of_day(first, tz).with_timezone(&Utc).max(start_date);
            return Some(Occurrence { start, deadline });
        })
    }

    /// Deadline to store on an instance when it is *reused* by the ‹D2› reconcile
    /// path. The `stored` value is the deadline already on the instance in the DB
    /// (may be a user override or a pre-widening cadence value); `occ_deadline` is
    /// the freshly-generated occurrence's deadline.
    ///
    /// Monthly always takes the occurrence's deadline: the widened span (up to the
    /// 28th) is authoritative and must overwrite any pre-widening stored value.
    /// Weekly preserves a user override (stored via `crud::update_task`, bounded by
    /// `expire_at`) or falls back to the occurrence's deadline if absent.
    #[must_use]
    pub(crate) fn deadline_for_reuse(
        &self,
        stored: Option<DateTime<Utc>>,
        occ_deadline: DateTime<Utc>,
    ) -> DateTime<Utc> {
        match self.period {
            Period::Monthly => occ_deadline,
            Period::Weekly => stored.unwrap_or(occ_deadline),
        }
    }

    /// Expiry timestamp for an occurrence from this cadence, given the start of
    /// the next occurrence.
    ///
    /// Monthly: the instance expires at the end of its own schedulable span (its
    /// deadline), cancelling it promptly at month-end rather than letting it linger
    /// into the following period. Weekly: expires at the end of the next
    /// occurrence's first day, capping the overdue overlap at one day (M4.5).
    #[must_use]
    pub(crate) fn expiry_for_occurrence(
        &self,
        occ: &Occurrence,
        next_start: Option<DateTime<Utc>>,
        tz: Tz,
    ) -> Option<DateTime<Utc>> {
        match self.period {
            Period::Monthly => Some(occ.deadline),
            Period::Weekly => next_start
                .map(|s| end_of_day(s.with_timezone(&tz).date_naive(), tz).with_timezone(&Utc)),
        }
    }
}

#[cfg(test)]
impl Cadence {
    /// Weekly cadence with every-week interval and one singleton window per day (test ctor).
    pub(crate) fn weekly(days: Vec<chrono::Weekday>) -> Self {
        Self::weekly_every(1, days)
    }

    /// Test ctor: weekly, every `interval` weeks, one singleton window per day.
    pub(crate) fn weekly_every(interval: u8, days: Vec<chrono::Weekday>) -> Self {
        let windows = days
            .into_iter()
            .map(|d| {
                let o = u8::try_from(d.num_days_from_monday()).expect("weekday offset fits u8");
                Window { start: o, end: o }
            })
            .collect();
        Self::new(Period::Weekly, interval, windows).expect("valid weekly test cadence")
    }

    /// Test ctor: monthly, every month, on `day_of_month` (1-based, singleton window).
    pub(crate) fn monthly(day_of_month: u8) -> Self {
        Self::monthly_every(1, day_of_month)
    }

    /// Test ctor: monthly, every `interval` months, on `day_of_month` (1-based).
    pub(crate) fn monthly_every(interval: u8, day_of_month: u8) -> Self {
        let o = day_of_month - 1;
        Self::new(Period::Monthly, interval, vec![Window { start: o, end: o }])
            .expect("valid monthly test cadence")
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, NaiveDate, Utc, Weekday};
    use chrono_tz::Tz;
    use test_case::test_case;

    use super::{Cadence, Occurrence, Period, Window};
    use crate::error::AppError;
    use crate::test_support::utc;

    fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    /// Deadline dates of the first `n` occurrences anchored at `start`, in UTC.
    /// For singleton windows the deadline date equals the occurrence day.
    fn first_dates(cadence: &Cadence, start: DateTime<Utc>, n: usize) -> Vec<NaiveDate> {
        cadence
            .occurrences(start, chrono_tz::UTC)
            .take(n)
            .map(|o| o.deadline.date_naive())
            .collect()
    }

    fn w(start: u8, end: u8) -> Window {
        Window { start, end }
    }

    fn occ(start: DateTime<Utc>, deadline: DateTime<Utc>) -> Occurrence {
        Occurrence { start, deadline }
    }

    /// An occurrence deadline `hh:mm:59` UTC — deadlines land on the last second
    /// of the window's final local day.
    fn deadline_utc(year: i32, month: u32, day: u32, hour: u32, min: u32) -> DateTime<Utc> {
        utc(year, month, day, hour, min) + Duration::seconds(59)
    }

    #[test_case(Period::Weekly, 0, vec![w(0, 0)] ; "zero interval")]
    #[test_case(Period::Weekly, 1, vec![] ; "empty windows")]
    #[test_case(Period::Weekly, 1, vec![w(3, 1)] ; "start after end")]
    #[test_case(Period::Weekly, 1, vec![w(0, 7)] ; "weekly end past sunday")]
    #[test_case(Period::Monthly, 1, vec![w(0, 28)] ; "monthly end past the 28th")]
    #[test_case(Period::Weekly, 1, vec![w(0, 2), w(2, 4)] ; "overlapping windows")]
    #[test_case(Period::Weekly, 1, vec![w(1, 1), w(1, 1)] ; "duplicate windows")]
    fn new_rejects_invalid(period: Period, interval: u8, windows: Vec<Window>) {
        assert!(matches!(
            Cadence::new(period, interval, windows),
            Err(AppError::Validation(_))
        ));
    }

    #[test_case(Period::Weekly, vec![w(0, 0)] ; "weekly first day (Monday)")]
    #[test_case(Period::Weekly, vec![w(6, 6)] ; "weekly last day (Sunday)")]
    #[test_case(Period::Weekly, vec![w(0, 6)] ; "weekly full week")]
    #[test_case(Period::Monthly, vec![w(0, 0)] ; "monthly first of month")]
    #[test_case(Period::Monthly, vec![w(27, 27)] ; "monthly 28th singleton")]
    #[test_case(Period::Monthly, vec![w(0, 27)] ; "monthly full month (1st..28th)")]
    #[test_case(Period::Weekly, vec![w(0, 1), w(2, 3)] ; "adjacent windows allowed")]
    fn new_accepts_boundary(period: Period, windows: Vec<Window>) {
        assert!(Cadence::new(period, 1, windows).is_ok());
    }

    #[test_case(vec![w(2, 3)], vec![w(2, 3)] ; "single window")]
    #[test_case(vec![w(4, 5), w(0, 1)], vec![w(0, 1), w(4, 5)] ; "two reversed, with gap")]
    #[test_case(vec![w(2, 3), w(0, 1)], vec![w(0, 1), w(2, 3)] ; "two reversed, adjacent (no gap)")]
    #[test_case(vec![w(4, 4), w(0, 0), w(2, 2)], vec![w(0, 0), w(2, 2), w(4, 4)] ; "three singletons, gaps, shuffled")]
    #[test_case(
        vec![w(3, 3), w(6, 6), w(0, 0), w(4, 4), w(1, 1), w(5, 5), w(2, 2)],
        vec![w(0, 0), w(1, 1), w(2, 2), w(3, 3), w(4, 4), w(5, 5), w(6, 6)]
        ; "seven singletons (full week), shuffled"
    )]
    #[allow(clippy::needless_pass_by_value)]
    fn new_sorts_windows_by_start(input: Vec<Window>, expected: Vec<Window>) {
        let cadence = Cadence::new(Period::Weekly, 1, input).expect("valid");
        assert_eq!(cadence.windows(), expected.as_slice());
    }

    #[test_case(Period::Weekly, 1, vec![w(0, 0)] ; "weekly single singleton")]
    #[test_case(Period::Weekly, 1, vec![w(0, 6)] ; "weekly full-week span")]
    #[test_case(Period::Weekly, 2, vec![w(0, 1), w(4, 5)] ; "weekly two spans, gap, interval 2")]
    #[test_case(Period::Weekly, 1, vec![w(0, 0), w(1, 1), w(2, 2), w(3, 3), w(4, 4), w(5, 5), w(6, 6)] ; "weekly seven singletons (full week)")]
    #[test_case(Period::Monthly, 1, vec![w(0, 0)] ; "monthly single singleton")]
    #[test_case(Period::Monthly, 3, vec![w(0, 0), w(14, 15)] ; "monthly singleton + span, interval 3")]
    #[test_case(Period::Monthly, 12, vec![w(27, 27)] ; "monthly 28th, interval 12")]
    fn serde_roundtrip(period: Period, interval: u8, windows: Vec<Window>) {
        let cadence = Cadence::new(period, interval, windows).expect("valid");
        let json = serde_json::to_string(&cadence).expect("serialize");
        let back: Cadence = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cadence, back);
    }

    // Deserialization is routed through `Cadence::new` via `RawCadence`, so a
    // structurally well-formed JSON payload that violates a cadence invariant must
    // fail to deserialize. The exhaustive rule matrix lives in `new_rejects_invalid`;
    // these cases only prove the `#[serde(try_from)]` wiring surfaces each class of
    // validation error (one per validation stage) rather than silently bypassing it.
    #[test_case(r#"{"period":"Weekly","interval":0,"windows":[{"start":0,"end":0}]}"# ; "zero interval")]
    #[test_case(r#"{"period":"Weekly","interval":1,"windows":[]}"# ; "empty windows")]
    #[test_case(r#"{"period":"Weekly","interval":1,"windows":[{"start":0,"end":9}]}"# ; "offset out of range")]
    #[test_case(r#"{"period":"Weekly","interval":1,"windows":[{"start":0,"end":3},{"start":2,"end":4}]}"# ; "overlapping windows")]
    fn deserialize_rejects_invalid(json: &str) {
        let result: Result<Cadence, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // A cadence expands to a sequence of dates; for singleton windows the deadline
    // date is the occurrence day. One table over (cadence, anchor) → deadline
    // dates. Datetime/timezone/clamping precision lives in the table below.
    #[test_case(
        Cadence::weekly(vec![Weekday::Mon, Weekday::Fri]),
        utc(2026, 3, 2, 0, 0),
        vec![ymd(2026, 3, 2), ymd(2026, 3, 6), ymd(2026, 3, 9), ymd(2026, 3, 13)]
        ; "weekly Mon+Fri emits each selected day"
    )]
    #[test_case(
        Cadence::weekly_every(2, vec![Weekday::Mon]),
        utc(2026, 3, 2, 0, 0),
        vec![ymd(2026, 3, 2), ymd(2026, 3, 16), ymd(2026, 3, 30)]
        ; "weekly interval 2 skips off-weeks"
    )]
    #[test_case(
        Cadence::monthly_every(3, 15),
        utc(2026, 1, 15, 0, 0),
        vec![ymd(2026, 1, 28), ymd(2026, 4, 28), ymd(2026, 7, 28), ymd(2026, 10, 28)]
        ; "monthly interval 3: deadline extends to 28th each period"
    )]
    #[test_case(
        Cadence::monthly(28),
        utc(2026, 1, 28, 0, 0),
        vec![ymd(2026, 1, 28), ymd(2026, 2, 28), ymd(2026, 3, 28), ymd(2026, 4, 28)]
        ; "monthly 28th lands every month incl. February (already at max)"
    )]
    #[test_case(
        Cadence::new(Period::Monthly, 1, vec![w(0, 0), w(14, 14)]).expect("valid"),
        utc(2026, 3, 1, 0, 0),
        vec![ymd(2026, 3, 14), ymd(2026, 3, 28), ymd(2026, 4, 14), ymd(2026, 4, 28)]
        ; "monthly two windows: each extends to next window minus one, last to 28th"
    )]
    #[test_case(
        Cadence::monthly(25),
        utc(2026, 3, 25, 0, 0),
        vec![ymd(2026, 3, 28), ymd(2026, 4, 28), ymd(2026, 5, 28)]
        ; "monthly day-25 deadline extends to 28th each month"
    )]
    #[test_case(
        Cadence::monthly(1),
        utc(2026, 3, 1, 0, 0),
        vec![ymd(2026, 3, 28), ymd(2026, 4, 28)]
        ; "monthly day-1 deadline extends to 28th (full span)"
    )]
    #[test_case(
        Cadence::weekly(vec![Weekday::Mon, Weekday::Fri]),
        utc(2026, 3, 4, 0, 0),
        vec![ymd(2026, 3, 6), ymd(2026, 3, 9), ymd(2026, 3, 13)]
        ; "never emits before the anchor (mid-week start)"
    )]
    #[test_case(
        Cadence::weekly(vec![Weekday::Mon]),
        utc(2026, 12, 28, 0, 0),
        vec![ymd(2026, 12, 28), ymd(2027, 1, 4)]
        ; "crosses the year boundary"
    )]
    #[allow(clippy::needless_pass_by_value)]
    fn occurrence_deadline_dates(
        cadence: Cadence,
        anchor: DateTime<Utc>,
        expected: Vec<NaiveDate>,
    ) {
        assert_eq!(first_dates(&cadence, anchor, expected.len()), expected);
    }

    // Per occurrence: `start` = 00:00 of the window's first local day (clamped up
    // to the anchor), `deadline` = 23:59:59 of its last local day — both in UTC.
    // Covers multi-day windows, multiple windows per period, the timezone offset,
    // and anchor clamping.
    #[test_case(
        Cadence::weekly(vec![Weekday::Mon]),
        utc(2026, 1, 1, 0, 0),
        chrono_tz::Europe::Berlin,
        vec![occ(utc(2026, 1, 4, 23, 0), deadline_utc(2026, 1, 5, 22, 59))]
        ; "timezone: Berlin Mon maps to Sun 23:00..Mon 22:59:59 UTC"
    )]
    #[test_case(
        Cadence::new(Period::Weekly, 1, vec![w(5, 6)]).expect("valid"),
        utc(2026, 3, 2, 0, 0),
        chrono_tz::UTC,
        vec![occ(utc(2026, 3, 7, 0, 0), deadline_utc(2026, 3, 8, 23, 59))]
        ; "weekend window Sat..=Sun spans to one occurrence"
    )]
    #[test_case(
        Cadence::new(Period::Weekly, 1, vec![w(0, 1), w(3, 4)]).expect("valid"),
        utc(2026, 3, 2, 0, 0),
        chrono_tz::UTC,
        vec![
            occ(utc(2026, 3, 2, 0, 0), deadline_utc(2026, 3, 3, 23, 59)),
            occ(utc(2026, 3, 5, 0, 0), deadline_utc(2026, 3, 6, 23, 59)),
            occ(utc(2026, 3, 9, 0, 0), deadline_utc(2026, 3, 10, 23, 59)),
        ]
        ; "multiple windows per period emit in order"
    )]
    #[test_case(
        Cadence::new(Period::Weekly, 1, vec![w(0, 4)]).expect("valid"),
        utc(2026, 3, 4, 12, 0),
        chrono_tz::UTC,
        vec![occ(utc(2026, 3, 4, 12, 0), deadline_utc(2026, 3, 6, 23, 59))]
        ; "window straddling the anchor clamps start to the anchor"
    )]
    #[test_case(
        Cadence::monthly(25),
        utc(2026, 3, 25, 0, 0),
        chrono_tz::UTC,
        vec![occ(utc(2026, 3, 25, 0, 0), deadline_utc(2026, 3, 28, 23, 59))]
        ; "monthly day-25: start stays at 25th, deadline extends to 28th"
    )]
    #[test_case(
        Cadence::monthly(25),
        utc(2026, 3, 28, 0, 0),
        chrono_tz::UTC,
        vec![occ(utc(2026, 3, 28, 0, 0), deadline_utc(2026, 3, 28, 23, 59))]
        ; "monthly day-25 anchor on 28th: start clamped to anchor, deadline same day"
    )]
    #[allow(clippy::needless_pass_by_value)]
    fn occurrence_start_and_deadline(
        cadence: Cadence,
        anchor: DateTime<Utc>,
        tz: Tz,
        expected: Vec<Occurrence>,
    ) {
        let actual: Vec<Occurrence> = cadence
            .occurrences(anchor, tz)
            .take(expected.len())
            .collect();
        assert_eq!(actual, expected);
    }

    #[test_case(
        Cadence::monthly(25),
        Occurrence { start: utc(2026, 3, 25, 0, 0), deadline: deadline_utc(2026, 3, 28, 23, 59) },
        Some(utc(2026, 4, 25, 0, 0)),
        Some(deadline_utc(2026, 3, 28, 23, 59))
        ; "monthly: own deadline regardless of next_start present"
    )]
    #[test_case(
        Cadence::monthly(25),
        Occurrence { start: utc(2026, 3, 25, 0, 0), deadline: deadline_utc(2026, 3, 28, 23, 59) },
        None,
        Some(deadline_utc(2026, 3, 28, 23, 59))
        ; "monthly: own deadline even with no next_start"
    )]
    #[test_case(
        Cadence::weekly(vec![Weekday::Mon]),
        Occurrence { start: utc(2026, 3, 2, 0, 0), deadline: deadline_utc(2026, 3, 2, 23, 59) },
        Some(utc(2026, 3, 9, 0, 0)),
        Some(deadline_utc(2026, 3, 9, 23, 59))
        ; "weekly: end of next_start's day"
    )]
    #[test_case(
        Cadence::weekly(vec![Weekday::Mon]),
        Occurrence { start: utc(2026, 3, 2, 0, 0), deadline: deadline_utc(2026, 3, 2, 23, 59) },
        None,
        None
        ; "weekly: no next_start yields None"
    )]
    #[allow(clippy::needless_pass_by_value)]
    fn expiry_policy(
        cadence: Cadence,
        occ: Occurrence,
        next_start: Option<DateTime<Utc>>,
        expected: Option<DateTime<Utc>>,
    ) {
        assert_eq!(
            cadence.expiry_for_occurrence(&occ, next_start, chrono_tz::UTC),
            expected
        );
    }
}
