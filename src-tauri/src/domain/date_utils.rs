// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Calendar date primitives shared by recurrence logic.

use chrono::{DateTime, Datelike, Months, NaiveDate, NaiveTime, TimeZone, Weekday};
use chrono_tz::Tz;

/// Monday of the week containing `date`.
pub(super) fn start_of_week(date: NaiveDate) -> NaiveDate {
    date.week(Weekday::Mon).first_day()
}

/// First day of the month `offset` months after `date`'s month.
pub(super) fn start_of_month(date: NaiveDate, offset: u32) -> NaiveDate {
    date.with_day(1)
        .and_then(|first| first.checked_add_months(Months::new(offset)))
        .expect("first-of-month plus a bounded month offset is always valid")
}

/// `date` at 23:59:59 in `tz` — the deadline's wall-clock intent, zoned.
///
/// Returns a zoned value; the recurrence boundary projects it to the domain's
/// canonical UTC deadline. `earliest()` resolves an ambiguous local time (a DST
/// fold) to the earlier offset; end-of-day never lands in a spring-forward gap,
/// so the missing-time fallback is defensive only.
pub(crate) fn end_of_day(date: NaiveDate, tz: Tz) -> DateTime<Tz> {
    let eod =
        date.and_time(NaiveTime::from_hms_opt(23, 59, 59).expect("23:59:59 is a valid wall time"));
    tz.from_local_datetime(&eod)
        .earliest()
        .unwrap_or_else(|| tz.from_utc_datetime(&eod))
}

/// `date` at 00:00:00 in `tz` — a window's earliest-placement bound, zoned.
///
/// Returns a zoned value; the recurrence boundary projects it to UTC. As with
/// [`end_of_day`], `earliest()` resolves an ambiguous local time (a DST fold) to
/// the earlier offset; the missing-time fallback (a DST gap at midnight, which a
/// few zones have) treats the naive time as UTC and is defensive only.
pub(crate) fn start_of_day(date: NaiveDate, tz: Tz) -> DateTime<Tz> {
    let sod = date.and_time(NaiveTime::MIN);
    tz.from_local_datetime(&sod)
        .earliest()
        .unwrap_or_else(|| tz.from_utc_datetime(&sod))
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime, Utc};
    use chrono_tz::Tz;
    use test_case::test_case;

    use super::{end_of_day, start_of_day, start_of_month, start_of_week};

    fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    #[test_case(ymd(2026, 3, 16) => ymd(2026, 3, 16) ; "monday maps to itself")]
    #[test_case(ymd(2026, 3, 18) => ymd(2026, 3, 16) ; "wednesday maps back to monday")]
    #[test_case(ymd(2026, 3, 22) => ymd(2026, 3, 16) ; "sunday maps back to monday")]
    #[test_case(ymd(2026, 1, 1) => ymd(2025, 12, 29) ; "thursday crosses year boundary")]
    fn start_of_week_cases(date: NaiveDate) -> NaiveDate {
        start_of_week(date)
    }

    #[test_case(ymd(2026, 6, 1), 0 => ymd(2026, 6, 1) ; "offset 0 keeps first")]
    #[test_case(ymd(2026, 6, 30), 0 => ymd(2026, 6, 1) ; "offset 0 normalizes to first")]
    #[test_case(ymd(2026, 1, 1), 1 => ymd(2026, 2, 1) ; "jan + 1 = feb first")]
    #[test_case(ymd(2026, 1, 31), 1 => ymd(2026, 2, 1) ; "mid-month + 1 normalizes")]
    #[test_case(ymd(2026, 11, 1), 3 => ymd(2027, 2, 1) ; "nov + 3 wraps year")]
    #[test_case(ymd(2026, 10, 1), 12 => ymd(2027, 10, 1) ; "plus twelve is next year")]
    fn start_of_month_cases(date: NaiveDate, offset: u32) -> NaiveDate {
        start_of_month(date, offset)
    }

    #[test_case("UTC", ymd(2026, 3, 15) => (ymd(2026, 3, 15), NaiveTime::from_hms_opt(23, 59, 59).unwrap()) ; "utc end of day")]
    #[test_case("Europe/Berlin", ymd(2026, 1, 15) => (ymd(2026, 1, 15), NaiveTime::from_hms_opt(22, 59, 59).unwrap()) ; "berlin winter is utc+1")]
    #[test_case("Europe/Berlin", ymd(2026, 7, 15) => (ymd(2026, 7, 15), NaiveTime::from_hms_opt(21, 59, 59).unwrap()) ; "berlin summer is utc+2")]
    fn end_of_day_cases(tz: &str, date: NaiveDate) -> (NaiveDate, NaiveTime) {
        let tz: Tz = tz.parse().expect("valid tz");
        let utc = end_of_day(date, tz).with_timezone(&Utc);
        (utc.date_naive(), utc.time())
    }

    #[test_case("UTC", ymd(2026, 3, 15) => (ymd(2026, 3, 15), NaiveTime::from_hms_opt(0, 0, 0).unwrap()) ; "utc start of day")]
    #[test_case("Europe/Berlin", ymd(2026, 1, 15) => (ymd(2026, 1, 14), NaiveTime::from_hms_opt(23, 0, 0).unwrap()) ; "berlin winter start is prev day 23:00 utc")]
    #[test_case("Europe/Berlin", ymd(2026, 7, 15) => (ymd(2026, 7, 14), NaiveTime::from_hms_opt(22, 0, 0).unwrap()) ; "berlin summer start is prev day 22:00 utc")]
    fn start_of_day_cases(tz: &str, date: NaiveDate) -> (NaiveDate, NaiveTime) {
        let tz: Tz = tz.parse().expect("valid tz");
        let utc = start_of_day(date, tz).with_timezone(&Utc);
        (utc.date_naive(), utc.time())
    }
}
