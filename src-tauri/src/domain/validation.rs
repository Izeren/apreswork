// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Domain validation logic for input DTOs.
//!
//! All validators return `Result<(), AppError>` using
//! [`AppError::Validation`](crate::error::AppError::Validation).

use chrono::{DateTime, Utc};

use super::inputs::{
    CreateScheduleInput, CreateTaskInput, CreateTemplateInput, ScheduleWindowInput,
    UpdateTaskInput, UpdateTemplateInput,
};
use super::models::AppConfig;
use crate::error::AppError;

fn validate_title(title: &str, label: &str) -> Result<(), AppError> {
    if title.trim().is_empty() {
        return Err(AppError::Validation(format!(
            "{label} title must not be empty"
        )));
    }
    Ok(())
}

fn validate_positive_duration(duration: i64) -> Result<(), AppError> {
    if duration <= 0 {
        return Err(AppError::Validation(
            "duration_minutes must be greater than 0".to_owned(),
        ));
    }
    Ok(())
}

fn validate_min_chunk(min_chunk: i64) -> Result<(), AppError> {
    if min_chunk < 5 {
        return Err(AppError::Validation(
            "min_chunk_minutes must be at least 5".to_owned(),
        ));
    }
    Ok(())
}

/// Validates a [`CreateTaskInput`] before persisting.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if:
/// - `title` is empty or whitespace-only
/// - `duration_minutes <= 0`
/// - `min_chunk_minutes` is `Some` and less than 5
/// - `start_date` is `Some` and later than `deadline`
pub fn validate_create_task(input: &CreateTaskInput) -> Result<(), AppError> {
    validate_title(&input.title, "task")?;
    validate_positive_duration(input.duration_minutes)?;
    if let Some(min_chunk) = input.min_chunk_minutes {
        validate_min_chunk(min_chunk)?;
    }
    validate_task_dates(input.start_date, Some(input.deadline))?;
    Ok(())
}

fn validate_optional_patch_fields(
    title: Option<&str>,
    label: &str,
    duration: Option<i64>,
    min_chunk: Option<i64>,
) -> Result<(), AppError> {
    if let Some(t) = title {
        validate_title(t, label)?;
    }
    if let Some(d) = duration {
        validate_positive_duration(d)?;
    }
    if let Some(mc) = min_chunk {
        validate_min_chunk(mc)?;
    }
    Ok(())
}

/// Validates an [`UpdateTaskInput`] before applying the patch.
///
/// Only validates fields that are `Some` — same rules as
/// [`validate_create_task`] for `title`, `duration_minutes`, and
/// `min_chunk_minutes`.
///
/// Date ordering is NOT checked here (the patch may set only one side).
/// The effective-values check in `update_task` covers both fields after
/// the patch is applied.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if any present field violates its constraint.
pub fn validate_update_task(input: &UpdateTaskInput) -> Result<(), AppError> {
    validate_optional_patch_fields(
        input.title.as_deref(),
        "task",
        input.duration_minutes,
        input.min_chunk_minutes,
    )
}

/// Validates a [`CreateTemplateInput`] before persisting.
///
/// The cadence is validated at construction (`Cadence::new` / deserialization),
/// so an invalid cadence cannot reach here — `title` and `duration_minutes`
/// are checked.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if:
/// - `title` is empty or whitespace-only
/// - `duration_minutes <= 0`
pub fn validate_create_template(input: &CreateTemplateInput) -> Result<(), AppError> {
    validate_title(&input.title, "template")?;
    validate_positive_duration(input.duration_minutes)?;
    Ok(())
}

/// Validates an [`UpdateTemplateInput`] before applying the patch.
///
/// Only validates fields that are `Some` — same rules as
/// [`validate_create_template`] for `title` and `duration_minutes`.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if any present field violates its constraint.
pub fn validate_update_template(input: &UpdateTemplateInput) -> Result<(), AppError> {
    validate_optional_patch_fields(
        input.title.as_deref(),
        "template",
        input.duration_minutes,
        None,
    )
}

/// Validates that a task's start date does not fall after its deadline.
/// Either side `None` passes (nothing to compare).
///
/// # Errors
/// Returns [`AppError::Validation`] if both are `Some` and `start_date > deadline`.
pub fn validate_task_dates(
    start_date: Option<DateTime<Utc>>,
    deadline: Option<DateTime<Utc>>,
) -> Result<(), AppError> {
    if let (Some(start), Some(end)) = (start_date, deadline) {
        if start > end {
            return Err(AppError::Validation(
                "start_date must not be later than deadline".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Minimum window size (minutes) a task requires under the effective-no-split
/// rule: unsplittable work (explicit `no_split`, or `duration_minutes <=
/// min_chunk_minutes`) needs its full duration in one window; splittable work
/// needs only its minimum chunk.
///
/// This is the ONE definition of the capacity predicate —
/// [`validate_task_fits_schedule`] and error-message call sites both use it.
#[must_use]
pub fn required_window_minutes(
    duration_minutes: i64,
    min_chunk_minutes: i64,
    no_split: bool,
) -> i64 {
    if no_split || duration_minutes <= min_chunk_minutes {
        duration_minutes
    } else {
        min_chunk_minutes
    }
}

/// Validates that a task's timing parameters can be accommodated by the
/// schedule's capacity.
///
/// The required window comes from [`required_window_minutes`]: the full
/// duration for effectively unsplittable tasks, the minimum chunk otherwise
/// (chunks always fit in any window at least as large as the minimum chunk
/// size).
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the task cannot be placed.
pub fn validate_task_fits_schedule(
    duration_minutes: i64,
    min_chunk_minutes: i64,
    no_split: bool,
    largest_window_minutes: i64,
    schedule_name: &str,
) -> Result<(), AppError> {
    let required = required_window_minutes(duration_minutes, min_chunk_minutes, no_split);
    if required <= largest_window_minutes {
        return Ok(());
    }
    // In the splittable branch required == min_chunk < duration, so equality
    // with the duration uniquely identifies the effectively-unsplittable case.
    if required == duration_minutes {
        Err(AppError::Validation(format!(
            "task duration ({duration_minutes} min) exceeds the largest window of \
             schedule '{schedule_name}' ({largest_window_minutes} min) and the task \
             cannot be split"
        )))
    } else {
        Err(AppError::Validation(format!(
            "min_chunk_minutes ({min_chunk_minutes}) exceeds the largest window of \
             schedule '{schedule_name}' ({largest_window_minutes} min)"
        )))
    }
}

/// Validates that a recurring template's duration can be accommodated by the
/// schedule's capacity.
///
/// Recurring instances are always created with `no_split = true`, so the full
/// template duration must fit within the schedule's largest single window.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the template duration exceeds the
/// schedule's capacity.
pub fn validate_template_fits_schedule(
    duration_minutes: i64,
    largest_window_minutes: i64,
    schedule_name: &str,
) -> Result<(), AppError> {
    if duration_minutes > largest_window_minutes {
        return Err(AppError::Validation(format!(
            "template duration ({duration_minutes} min) exceeds the largest window of \
             schedule '{schedule_name}' ({largest_window_minutes} min)"
        )));
    }
    Ok(())
}

/// Validates a [`CreateScheduleInput`] before persisting.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if:
/// - `name` is empty (or whitespace-only)
/// - `windows` is empty
/// - Any window has `start_time >= end_time`
/// - Two windows on the same `day_of_week` overlap
pub fn validate_create_schedule(input: &CreateScheduleInput) -> Result<(), AppError> {
    if input.name.trim().is_empty() {
        return Err(AppError::Validation(
            "schedule name must not be empty".to_owned(),
        ));
    }
    if input.windows.is_empty() {
        return Err(AppError::Validation(
            "schedule must have at least one window".to_owned(),
        ));
    }
    validate_schedule_windows(&input.windows)
}

/// Validates a slice of [`ScheduleWindowInput`]s.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if:
/// - Any window has `start_time >= end_time`
/// - Two windows on the same `day_of_week` overlap (touching is OK)
pub fn validate_schedule_windows(windows: &[ScheduleWindowInput]) -> Result<(), AppError> {
    for window in windows {
        if window.start_time >= window.end_time {
            return Err(AppError::Validation(
                "window start_time must be before end_time".to_owned(),
            ));
        }
    }
    for i in 0..windows.len() {
        for j in (i + 1)..windows.len() {
            if windows_overlap(&windows[i], &windows[j]) {
                return Err(AppError::Validation(
                    "schedule windows must not overlap on the same day".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

/// Returns `true` if two windows on the same day of week overlap.
///
/// Two windows are considered overlapping when they share the same
/// `day_of_week` and their time ranges intersect (touching boundaries
/// are OK — e.g., 18:00–20:00 and 20:00–22:00 do not overlap).
fn windows_overlap(a: &ScheduleWindowInput, b: &ScheduleWindowInput) -> bool {
    a.day_of_week == b.day_of_week && a.start_time < b.end_time && b.start_time < a.end_time
}

/// Inclusive bounds for [`AppConfig::planning_horizon_days`].
pub const PLANNING_HORIZON_DAYS_RANGE: (i64, i64) = (1, 365);
/// Inclusive bounds for [`AppConfig::max_continuous_minutes`].
pub const MAX_CONTINUOUS_MINUTES_RANGE: (i64, i64) = (15, 1440);
/// Inclusive bounds for [`AppConfig::min_break_minutes`].
pub const MIN_BREAK_MINUTES_RANGE: (i64, i64) = (0, 480);

fn check_range(value: i64, (lo, hi): (i64, i64), field: &str) -> Result<(), AppError> {
    if !(lo..=hi).contains(&value) {
        return Err(AppError::Validation(format!(
            "{field} must be between {lo} and {hi}"
        )));
    }
    Ok(())
}

/// Validates an [`AppConfig`] before persisting (the patched result of a
/// config update, so both the incoming patch and the final stored state are
/// covered).
///
/// # Errors
///
/// Returns [`AppError::Validation`] if:
/// - `planning_horizon_days` is outside [`PLANNING_HORIZON_DAYS_RANGE`]
/// - `max_continuous_minutes` is outside [`MAX_CONTINUOUS_MINUTES_RANGE`]
/// - `min_break_minutes` is outside [`MIN_BREAK_MINUTES_RANGE`]
/// - `timezone` is not a valid IANA zone
pub fn validate_config(config: &AppConfig) -> Result<(), AppError> {
    check_range(
        config.planning_horizon_days,
        PLANNING_HORIZON_DAYS_RANGE,
        "planning_horizon_days",
    )?;
    check_range(
        config.max_continuous_minutes,
        MAX_CONTINUOUS_MINUTES_RANGE,
        "max_continuous_minutes",
    )?;
    check_range(
        config.min_break_minutes,
        MIN_BREAK_MINUTES_RANGE,
        "min_break_minutes",
    )?;
    config.timezone_tz()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_config, validate_create_schedule, validate_create_task, validate_create_template,
        validate_schedule_windows, validate_task_dates, validate_task_fits_schedule,
        validate_template_fits_schedule, validate_update_task, validate_update_template,
        windows_overlap,
    };
    use crate::domain::models::AppConfig;
    use chrono::{DateTime, NaiveTime, TimeZone, Utc, Weekday};
    use test_case::test_case;

    use crate::domain::inputs::{
        CreateScheduleInput, CreateTaskInput, CreateTemplateInput, ScheduleWindowInput,
        UpdateTaskInput, UpdateTemplateInput,
    };
    use crate::domain::{cadence::Cadence, enums::Priority};
    use crate::error::AppError;
    use crate::test_support::assert_validation;

    /// Returns a `DateTime<Utc>` at midnight on the given date. Used in
    /// `#[test_case]` attributes where inline construction is needed.
    fn ts(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    fn valid_create_task() -> CreateTaskInput {
        CreateTaskInput {
            title: "Read a book".to_owned(),
            priority: Some(Priority::Medium),
            min_chunk_minutes: Some(30),
            ..CreateTaskInput::test_default()
        }
    }

    fn valid_update_task() -> UpdateTaskInput {
        UpdateTaskInput::default()
    }

    fn valid_weekly_template() -> CreateTemplateInput {
        CreateTemplateInput {
            title: "Weekly review".to_owned(),
            duration_minutes: 45,
            priority: Some(Priority::High),
            cadence: Cadence::weekly(vec![Weekday::Mon, Weekday::Wed, Weekday::Fri]),
            ..CreateTemplateInput::test_default()
        }
    }

    fn valid_monthly_template() -> CreateTemplateInput {
        CreateTemplateInput {
            title: "Monthly report".to_owned(),
            duration_minutes: 90,
            cadence: Cadence::monthly(15),
            ..CreateTemplateInput::test_default()
        }
    }

    /// Shorthand: `hm(18, 30)` → `NaiveTime 18:30:00`
    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
    }

    fn make_window(day: Weekday, start: NaiveTime, end: NaiveTime) -> ScheduleWindowInput {
        ScheduleWindowInput {
            day_of_week: day,
            start_time: start,
            end_time: end,
        }
    }

    fn assert_pass_or_error(result: &Result<(), AppError>, should_pass: bool) {
        if should_pass {
            assert!(result.is_ok(), "expected Ok, got: {result:?}");
        } else {
            assert_validation(result);
        }
    }

    /// Asserts `result` is a [`AppError::Validation`] whose message contains
    /// every fragment (schedule name, offending numbers, etc.).
    fn assert_validation_message_contains(result: Result<(), AppError>, fragments: &[&str]) {
        let msg = match result {
            Err(AppError::Validation(m)) => m,
            other => panic!("expected Validation error, got: {other:?}"),
        };
        for fragment in fragments {
            assert!(
                msg.contains(fragment),
                "expected {fragment:?} in message: {msg}"
            );
        }
    }

    // (duration, min_chunk, no_split, largest, schedule_name, should_pass)
    #[test_case(60, 30, false, 90, "S", true  ; "splittable min_chunk fits")]
    #[test_case(60, 30, false, 29, "S", false ; "splittable min_chunk exceeds largest")]
    #[test_case(60, 30, false, 30, "S", true  ; "splittable min_chunk equals largest boundary")]
    #[test_case(20, 30, true,  25, "S", true  ; "no-split duration fits min_chunk inert")]
    #[test_case(25, 30, true,  25, "S", true  ; "no-split duration equals largest")]
    #[test_case(60, 30, true,  50, "S", false ; "no-split duration exceeds largest")]
    #[test_case(20, 30, false, 25, "S", true  ; "auto-no-split duration fits")]
    #[test_case(20, 30, false, 19, "S", false ; "auto-no-split duration exceeds largest")]
    // Key pin: duration=20, min_chunk=30, no_split=false → effective_no_split (duration≤min_chunk)
    // → check duration(20) ≤ largest(25), NOT min_chunk(30): must PASS
    #[test_case(20, 30, false, 25, "S", true  ; "effective-no-split ignores min_chunk pin")]
    fn task_capacity_fit(
        duration: i64,
        min_chunk: i64,
        no_split: bool,
        largest: i64,
        name: &str,
        should_pass: bool,
    ) {
        let result = validate_task_fits_schedule(duration, min_chunk, no_split, largest, name);
        if should_pass {
            assert!(result.is_ok(), "expected Ok, got: {result:?}");
        } else {
            assert_validation(&result);
        }
    }

    #[test]
    fn task_capacity_fit_no_split_message_contains_name_and_numbers() {
        let result = validate_task_fits_schedule(50, 30, true, 40, "Evening");
        assert_validation_message_contains(result, &["Evening", "50", "40"]);
    }

    #[test]
    fn task_capacity_fit_splittable_message_contains_name_and_numbers() {
        let result = validate_task_fits_schedule(60, 45, false, 30, "Morning");
        assert_validation_message_contains(result, &["Morning", "45", "30"]);
    }

    #[test_case(60, 90, "T", true  ; "duration fits")]
    #[test_case(60, 60, "T", true  ; "duration equals largest boundary")]
    #[test_case(60, 50, "T", false ; "duration exceeds largest")]
    #[test_case( 0, 60, "T", true  ; "zero duration edge case")]
    fn template_capacity_fit(duration: i64, largest: i64, name: &str, should_pass: bool) {
        let result = validate_template_fits_schedule(duration, largest, name);
        if should_pass {
            assert!(result.is_ok(), "expected Ok, got: {result:?}");
        } else {
            assert_validation(&result);
        }
    }

    #[test]
    fn template_capacity_fit_message_contains_name_and_numbers() {
        let result = validate_template_fits_schedule(120, 90, "Evening");
        assert_validation_message_contains(result, &["Evening", "120", "90"]);
    }

    #[test_case(60, Some(30), true  ; "valid input")]
    #[test_case( 0, Some(30), false ; "zero duration")]
    #[test_case(-1, Some(30), false ; "negative duration")]
    #[test_case(60, Some(4),  false ; "min chunk below 5")]
    #[test_case(60, Some(5),  true  ; "min chunk exactly 5")]
    fn create_task(duration: i64, min_chunk: Option<i64>, should_pass: bool) {
        let mut input = valid_create_task();
        input.duration_minutes = duration;
        input.min_chunk_minutes = min_chunk;
        let result = validate_create_task(&input);
        if should_pass {
            assert!(result.is_ok());
        } else {
            assert_validation(&result);
        }
    }

    #[test_case(None,      None,     true  ; "all none")]
    #[test_case(Some(120), None,     true  ; "valid duration")]
    #[test_case(Some(0),   None,     false ; "zero duration")]
    #[test_case(Some(-1),  None,     false ; "negative duration")]
    #[test_case(None,      Some(3),  false ; "min chunk below 5")]
    #[test_case(None,      Some(5),  true  ; "min chunk exactly 5")]
    fn update_task(duration: Option<i64>, min_chunk: Option<i64>, should_pass: bool) {
        let mut input = valid_update_task();
        input.duration_minutes = duration;
        input.min_chunk_minutes = min_chunk;
        let result = validate_update_task(&input);
        if should_pass {
            assert!(result.is_ok());
        } else {
            assert_validation(&result);
        }
    }

    #[test]
    fn template_valid_weekly() {
        assert!(validate_create_template(&valid_weekly_template()).is_ok());
    }

    #[test]
    fn template_valid_monthly() {
        assert!(validate_create_template(&valid_monthly_template()).is_ok());
    }

    #[test_case(0,  true  ; "zero duration")]
    #[test_case(45, false ; "valid duration")]
    fn template_duration(duration: i64, should_fail: bool) {
        let mut input = valid_weekly_template();
        input.duration_minutes = duration;
        let result = validate_create_template(&input);
        if should_fail {
            assert_validation(&result);
        } else {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn schedule_windows_valid() {
        let windows = vec![
            make_window(Weekday::Mon, hm(18, 0), hm(20, 0)),
            make_window(Weekday::Wed, hm(19, 30), hm(21, 15)),
        ];
        assert!(validate_schedule_windows(&windows).is_ok());
    }

    #[test]
    fn schedule_windows_empty() {
        assert!(validate_schedule_windows(&[]).is_ok());
    }

    #[test_case(hm(18, 0),  hm(18, 0),  false ; "start equals end")]
    #[test_case(hm(18, 30), hm(18, 30), false ; "start equals end with minutes")]
    #[test_case(hm(20, 0),  hm(18, 0),  false ; "start after end")]
    #[test_case(hm(18, 0),  hm(20, 0),  true  ; "valid range")]
    #[test_case(hm(18, 15), hm(20, 45), true  ; "valid range with minutes")]
    fn schedule_window_time_range(start: NaiveTime, end: NaiveTime, should_pass: bool) {
        let windows = vec![make_window(Weekday::Mon, start, end)];
        let result = validate_schedule_windows(&windows);
        if should_pass {
            assert!(result.is_ok());
        } else {
            assert_validation(&result);
        }
    }

    // first window is always Mon 18:00–20:00
    #[test_case(Weekday::Mon, hm(19, 0),  hm(21, 0),  false ; "same day overlapping hours")]
    #[test_case(Weekday::Mon, hm(19, 30), hm(20, 30), false ; "same day overlap by minutes")]
    #[test_case(Weekday::Tue, hm(19, 0),  hm(21, 0),  true  ; "different day overlapping times")]
    #[test_case(Weekday::Mon, hm(20, 0),  hm(22, 0),  true  ; "same day adjacent")]
    #[test_case(Weekday::Mon, hm(19, 59), hm(21, 0),  false ; "overlap by 1 minute")]
    fn schedule_window_overlap(day: Weekday, start: NaiveTime, end: NaiveTime, should_pass: bool) {
        let windows = vec![
            make_window(Weekday::Mon, hm(18, 0), hm(20, 0)),
            make_window(day, start, end),
        ];
        let result = validate_schedule_windows(&windows);
        if should_pass {
            assert!(result.is_ok());
        } else {
            assert_validation(&result);
        }
    }

    #[test_case(Weekday::Mon, hm(18, 0),  hm(20, 0),  Weekday::Mon, hm(19, 0),  hm(21, 0),  true  ; "same day overlapping")]
    #[test_case(Weekday::Mon, hm(18, 0),  hm(20, 0),  Weekday::Mon, hm(20, 0),  hm(22, 0),  false ; "same day adjacent")]
    #[test_case(Weekday::Mon, hm(18, 0),  hm(20, 0),  Weekday::Tue, hm(18, 0),  hm(20, 0),  false ; "different days")]
    #[test_case(Weekday::Mon, hm(17, 0),  hm(23, 0),  Weekday::Mon, hm(18, 0),  hm(20, 0),  true  ; "contained")]
    #[test_case(Weekday::Mon, hm(18, 0),  hm(19, 30), Weekday::Mon, hm(19, 15), hm(20, 0),  true  ; "overlap by 15 min")]
    #[test_case(Weekday::Mon, hm(18, 0),  hm(19, 30), Weekday::Mon, hm(19, 30), hm(20, 0),  false ; "adjacent at minute boundary")]
    fn overlap_helper(
        day_a: Weekday,
        start_a: NaiveTime,
        end_a: NaiveTime,
        day_b: Weekday,
        start_b: NaiveTime,
        end_b: NaiveTime,
        expected: bool,
    ) {
        let a = make_window(day_a, start_a, end_a);
        let b = make_window(day_b, start_b, end_b);
        assert_eq!(windows_overlap(&a, &b), expected);
    }

    fn valid_create_schedule() -> CreateScheduleInput {
        CreateScheduleInput {
            name: "Evening".to_owned(),
            windows: vec![make_window(Weekday::Mon, hm(18, 0), hm(20, 0))],
        }
    }

    #[test]
    fn create_schedule_valid() {
        assert!(validate_create_schedule(&valid_create_schedule()).is_ok());
    }

    #[test_case(""   ; "empty name")]
    #[test_case("  " ; "whitespace-only name")]
    fn create_schedule_empty_name(name: &str) {
        let mut input = valid_create_schedule();
        input.name = name.to_owned();
        assert_validation(&validate_create_schedule(&input));
    }

    #[test]
    fn create_schedule_empty_windows() {
        let mut input = valid_create_schedule();
        input.windows = vec![];
        assert_validation(&validate_create_schedule(&input));
    }

    #[test]
    fn create_schedule_delegates_window_validation() {
        let mut input = valid_create_schedule();
        // Overlapping windows on the same day
        input.windows = vec![
            make_window(Weekday::Mon, hm(18, 0), hm(20, 0)),
            make_window(Weekday::Mon, hm(19, 0), hm(21, 0)),
        ];
        assert_validation(&validate_create_schedule(&input));
    }

    #[test_case(None,               None,               true  ; "both none")]
    #[test_case(None,               Some(ts(2026,1,1)), true  ; "start none deadline some")]
    #[test_case(Some(ts(2026,1,1)), None,               true  ; "start some deadline none")]
    #[test_case(Some(ts(2026,1,1)), Some(ts(2026,1,2)), true  ; "start before deadline")]
    #[test_case(Some(ts(2026,1,1)), Some(ts(2026,1,1)), true  ; "start equals deadline")]
    #[test_case(Some(ts(2026,1,2)), Some(ts(2026,1,1)), false ; "start after deadline")]
    fn task_dates(start_date: Option<DateTime<Utc>>, deadline: Option<DateTime<Utc>>, ok: bool) {
        let result = validate_task_dates(start_date, deadline);
        if ok {
            assert!(result.is_ok(), "expected Ok, got: {result:?}");
        } else {
            assert_validation(&result);
        }
    }

    #[test_case("",    false ; "empty title")]
    #[test_case("   ", false ; "whitespace-only title")]
    #[test_case("ok",  true  ; "valid title")]
    fn create_task_title(title: &str, should_pass: bool) {
        let mut input = valid_create_task();
        input.title = title.to_owned();
        let result = validate_create_task(&input);
        if should_pass {
            assert!(result.is_ok(), "expected Ok, got: {result:?}");
        } else {
            assert_validation(&result);
        }
    }

    /// Wiring pin: `validate_create_task` calls `validate_task_dates`.
    #[test]
    fn create_task_start_after_deadline_fails() {
        let mut input = valid_create_task();
        input.deadline = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        input.start_date = Some(Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap());
        assert_validation(&validate_create_task(&input));
    }

    #[test_case(Some(""),    false ; "empty title")]
    #[test_case(Some("   "), false ; "whitespace-only title")]
    #[test_case(Some("ok"),  true  ; "valid title")]
    #[test_case(None,        true  ; "none title unchanged")]
    fn update_title_validation(title: Option<&str>, should_pass: bool) {
        let mut task_input = valid_update_task();
        task_input.title = title.map(str::to_owned);
        assert_pass_or_error(&validate_update_task(&task_input), should_pass);
        let template_input = UpdateTemplateInput {
            title: title.map(str::to_owned),
            ..UpdateTemplateInput::default()
        };
        assert_pass_or_error(&validate_update_template(&template_input), should_pass);
    }

    #[test_case("",    false ; "empty title")]
    #[test_case("   ", false ; "whitespace-only title")]
    #[test_case("ok",  true  ; "valid title")]
    fn create_template_title(title: &str, should_pass: bool) {
        let mut input = valid_weekly_template();
        input.title = title.to_owned();
        let result = validate_create_template(&input);
        assert_pass_or_error(&result, should_pass);
    }

    fn config(horizon: i64, max_cont: i64, min_break: i64, tz: &str) -> AppConfig {
        AppConfig {
            planning_horizon_days: horizon,
            timezone: tz.to_owned(),
            max_continuous_minutes: max_cont,
            min_break_minutes: min_break,
            last_reschedule: None,
            last_mutation: None,
            last_sync: None,
            last_busy_sync: None,
        }
    }

    #[test_case(30, 120, 5, "UTC",             true  ; "seeded defaults")]
    #[test_case(1, 15, 0, "UTC",               true  ; "all lower bounds")]
    #[test_case(365, 1440, 480, "Europe/London", true ; "all upper bounds")]
    #[test_case(0, 120, 5, "UTC",              false ; "horizon below range")]
    #[test_case(366, 120, 5, "UTC",            false ; "horizon above range")]
    #[test_case(30, 14, 5, "UTC",              false ; "max continuous below range")]
    #[test_case(30, 1441, 5, "UTC",            false ; "max continuous above range")]
    #[test_case(30, 120, -1, "UTC",            false ; "min break negative")]
    #[test_case(30, 120, 481, "UTC",           false ; "min break above range")]
    #[test_case(30, 120, 5, "Mars/Olympus",    false ; "unknown timezone")]
    #[test_case(30, 120, 5, "",                false ; "empty timezone")]
    fn config_bounds(horizon: i64, max_cont: i64, min_break: i64, tz: &str, should_pass: bool) {
        let result = validate_config(&config(horizon, max_cont, min_break, tz));
        if should_pass {
            assert!(result.is_ok(), "expected Ok, got: {result:?}");
        } else {
            assert_validation(&result);
        }
    }

    /// The validation message must name the offending field so the UI can
    /// surface an actionable error (frontend shows Validation messages as-is).
    #[test_case(0, 120, 5, "UTC",   "planning_horizon_days" ; "horizon named")]
    #[test_case(30, 5, 5, "UTC",    "max_continuous_minutes" ; "max continuous named")]
    #[test_case(30, 120, -1, "UTC", "min_break_minutes" ; "min break named")]
    #[test_case(30, 120, 5, "bad",  "timezone" ; "timezone named")]
    fn config_error_names_field(
        horizon: i64,
        max_cont: i64,
        min_break: i64,
        tz: &str,
        expected_fragment: &str,
    ) {
        let result = validate_config(&config(horizon, max_cont, min_break, tz));
        match result {
            Err(AppError::Validation(msg)) => assert!(
                msg.contains(expected_fragment),
                "message {msg:?} should mention {expected_fragment:?}"
            ),
            other => panic!("expected Validation error, got: {other:?}"),
        }
    }
}
