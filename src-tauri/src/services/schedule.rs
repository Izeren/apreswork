// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Schedule service — stateless functions implementing schedule business logic.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::enums::TaskStatus;
use crate::domain::inputs::{
    CreateScheduleInput, ScheduleWindowInput, TaskFilter, UpdateScheduleInput,
};
use crate::domain::models::{AppConfig, Schedule, ScheduleWindow};
use crate::domain::validation::{
    required_window_minutes, validate_create_schedule, validate_schedule_windows,
    validate_task_fits_schedule, validate_template_fits_schedule,
};
use crate::error::AppError;
use crate::traits::storage::Store;

fn build_schedule_windows(
    schedule_id: &str,
    inputs: Vec<ScheduleWindowInput>,
) -> Vec<ScheduleWindow> {
    inputs
        .into_iter()
        .map(|w| ScheduleWindow {
            id: Uuid::now_v7().to_string(),
            schedule_id: schedule_id.to_owned(),
            day_of_week: w.day_of_week,
            start_time: w.start_time,
            end_time: w.end_time,
        })
        .collect()
}

fn schedule_shrink_error(
    schedule_name: &str,
    entity_kind: &str,
    entity_title: &str,
    required_minutes: i64,
    largest: i64,
) -> AppError {
    AppError::Validation(format!(
        "cannot shrink schedule '{schedule_name}': {entity_kind} '{entity_title}' needs \
         a {required_minutes}-min window (largest would be {largest} min)",
    ))
}

fn stamp_config_mutation(store: &dyn Store, now: DateTime<Utc>) -> Result<AppConfig, AppError> {
    let mut config = store.get_config()?;
    config.last_mutation = Some(now);
    Ok(config)
}

/// Create a new schedule, applying validation and persisting it.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the input fails validation (empty name,
/// empty windows, bad time ranges, or overlapping windows).
/// Returns [`AppError::Database`] on storage failure.
pub fn create_schedule(
    store: &dyn Store,
    input: CreateScheduleInput,
    now: DateTime<Utc>,
) -> Result<Schedule, AppError> {
    validate_create_schedule(&input)?;

    let schedule_id = Uuid::now_v7().to_string();
    let windows = build_schedule_windows(&schedule_id, input.windows);

    let schedule = Schedule {
        id: schedule_id,
        name: input.name,
        is_default: false,
        windows,
        created_at: now,
        updated_at: now,
    };

    store.create_schedule(&schedule)?;
    Ok(schedule)
}

/// Retrieve a schedule by ID.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if no schedule with the given ID exists.
/// Returns [`AppError::Database`] on storage failure.
pub fn get_schedule(store: &dyn Store, id: &str) -> Result<Schedule, AppError> {
    store.get_schedule(id)?.ok_or_else(|| AppError::NotFound {
        entity: "Schedule".into(),
        id: id.to_owned(),
    })
}

/// Return all schedules.
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn list_schedules(store: &dyn Store) -> Result<Vec<Schedule>, AppError> {
    store.list_schedules()
}

/// Apply a partial update to an existing schedule.
///
/// # Rules
///
/// - Default schedule name is immutable (changing it returns `Validation` error).
/// - Default schedule windows *can* be updated.
/// - When `windows` is `Some`, the old windows are fully replaced.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if:
/// - The schedule is default and `name` is being changed
/// - The new windows fail validation
///
/// Returns [`AppError::NotFound`] if the schedule does not exist.
/// Returns [`AppError::Database`] on storage failure.
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
pub fn update_schedule(
    store: &dyn Store,
    schedule_id: &str,
    input: UpdateScheduleInput,
    now: DateTime<Utc>,
) -> Result<Schedule, AppError> {
    let mut schedule = get_schedule(store, schedule_id)?;

    if schedule.is_default {
        if let Some(ref new_name) = input.name {
            if *new_name != schedule.name {
                return Err(AppError::Validation(
                    "cannot change the name of the default schedule".to_owned(),
                ));
            }
        }
    }

    if let Some(ref windows) = input.windows {
        if windows.is_empty() {
            return Err(AppError::Validation(
                "schedule must have at least one window".to_owned(),
            ));
        }
        validate_schedule_windows(windows)?;
    }

    // Track whether windows are being replaced before consuming input.windows.
    let windows_replaced = input.windows.is_some();

    if let Some(name) = input.name {
        schedule.name = name;
    }
    if let Some(window_inputs) = input.windows {
        schedule.windows = build_schedule_windows(&schedule.id, window_inputs);
    }

    // When windows are replaced, reject the edit if it would strand existing work.
    // Terminal tasks (Completed/Cancelled) and inactive templates are excluded:
    // they never reschedule, so they must not block non-breaking edits.
    if windows_replaced {
        let largest = schedule.largest_window_minutes();

        let non_terminal_tasks = store.list_tasks(&TaskFilter {
            schedule_id: Some(schedule_id.to_owned()),
            statuses: Some(vec![
                TaskStatus::Backlog,
                TaskStatus::Pending,
                TaskStatus::Scheduled,
            ]),
            ..TaskFilter::default()
        })?;
        for task in &non_terminal_tasks {
            validate_task_fits_schedule(
                task.duration_minutes,
                task.min_chunk_minutes,
                task.no_split,
                largest,
                &schedule.name,
            )
            .map_err(|_| {
                schedule_shrink_error(
                    &schedule.name,
                    "task",
                    &task.title,
                    required_window_minutes(
                        task.duration_minutes,
                        task.min_chunk_minutes,
                        task.no_split,
                    ),
                    largest,
                )
            })?;
        }

        let active_templates: Vec<_> = store
            .list_templates()?
            .into_iter()
            .filter(|t| t.schedule_id == schedule_id && t.is_active)
            .collect();
        for tmpl in &active_templates {
            validate_template_fits_schedule(tmpl.duration_minutes, largest, &schedule.name)
                .map_err(|_| {
                    schedule_shrink_error(
                        &schedule.name,
                        "template",
                        &tmpl.title,
                        tmpl.duration_minutes,
                        largest,
                    )
                })?;
        }
    }

    schedule.updated_at = now;
    let config = stamp_config_mutation(store, now)?;
    store.with_tx(&mut |tx| {
        tx.update_schedule(&schedule)?;
        tx.update_config(&config)
    })?;

    Ok(schedule)
}

/// Delete a schedule, reassigning all tasks and templates to the default schedule.
///
/// # Rules
///
/// - The default schedule cannot be deleted.
/// - All tasks with this `schedule_id` are reassigned to the default schedule.
/// - All recurring templates with this `schedule_id` are reassigned to the
///   default schedule.
/// - The schedule itself is deleted (CASCADE deletes its windows).
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the schedule is the default.
/// Returns [`AppError::NotFound`] if the schedule does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn delete_schedule(
    store: &dyn Store,
    schedule_id: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let schedule = get_schedule(store, schedule_id)?;

    if schedule.is_default {
        return Err(AppError::Validation(
            "cannot delete the default schedule".to_owned(),
        ));
    }

    let default_schedule = store.get_default_schedule()?;

    let mut tasks = store.list_tasks(&TaskFilter {
        schedule_id: Some(schedule_id.to_owned()),
        ..TaskFilter::default()
    })?;
    for task in &mut tasks {
        task.schedule_id.clone_from(&default_schedule.id);
        task.updated_at = now;
    }

    let mut templates: Vec<_> = store
        .list_templates()?
        .into_iter()
        .filter(|template| template.schedule_id == schedule_id)
        .collect();
    for template in &mut templates {
        template.schedule_id.clone_from(&default_schedule.id);
        template.updated_at = now;
    }

    let config = stamp_config_mutation(store, now)?;

    store.with_tx(&mut |tx| {
        for task in &tasks {
            tx.update_task(task)?;
        }
        for template in &templates {
            tx.update_template(template)?;
        }
        // Delete the schedule (CASCADE deletes its windows)
        tx.delete_schedule(schedule_id)?;
        tx.update_config(&config)
    })?;

    Ok(())
}

#[cfg(test)]
mod tests;
