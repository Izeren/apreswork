// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Recurring template service — stateless functions implementing recurring
//! template business logic.
//!
//! Template CRUD lives here; instance generation/maintenance lives in
//! [`reconcile`]. Most edits leave instances in place and let the next reconcile
//! pass converge; cadence and anchor changes are the exception — they delete all
//! future open unpinned instances atomically so reconcile regenerates from the
//! new schedule rather than repositioning into the wrong slots.

mod reconcile;

pub use reconcile::{auto_cancel_overdue, reconcile};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::domain::date_utils::start_of_day;
use crate::domain::enums::{Priority, TaskStatus};
use crate::domain::inputs::{CreateTemplateInput, TaskFilter, UpdateTemplateInput};
use crate::domain::models::{RecurringTemplate, Task};
use crate::domain::validation::{
    validate_create_template, validate_template_fits_schedule, validate_update_template,
};
use crate::error::AppError;
use crate::traits::storage::Store;

use reconcile::{delete_instance, instance_from_template};

/// Apply defaults and persist a new template from input.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the input fails validation.
/// Returns [`AppError::NotFound`] if no default schedule exists and
/// `schedule_id` is `None`.
/// Returns [`AppError::Database`] on storage failure.
pub fn create_template(
    store: &dyn Store,
    input: CreateTemplateInput,
    now: DateTime<Utc>,
) -> Result<RecurringTemplate, AppError> {
    validate_create_template(&input)?;

    let schedule_id = match input.schedule_id {
        Some(id) => id,
        None => store.get_default_schedule()?.id,
    };

    // Recurring instances are always no_split=true, so the full duration must fit
    // the largest window. A bogus schedule_id returns NotFound rather than an FK error.
    validate_schedule_capacity(store, input.duration_minutes, &schedule_id)?;

    let tz = store.get_config()?.timezone_tz()?;
    let template = RecurringTemplate {
        id: Uuid::now_v7().to_string(),
        title: input.title,
        description: input.description,
        duration_minutes: input.duration_minutes,
        priority: input.priority.unwrap_or(Priority::Medium),
        schedule_id,
        cadence: input.cadence,
        labels: input.labels.unwrap_or_default(),
        is_active: true,
        start_date: normalize_anchor(input.start_date.unwrap_or(now), tz),
        created_at: now,
        updated_at: now,
    };

    store.create_template(&template)?;
    Ok(template)
}

/// Retrieve a recurring template by ID.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if no template with the given ID exists.
/// Returns [`AppError::Database`] on storage failure.
pub fn get_template(store: &dyn Store, id: &str) -> Result<RecurringTemplate, AppError> {
    store.get_template(id)?.ok_or_else(|| AppError::NotFound {
        entity: "RecurringTemplate".into(),
        id: id.to_owned(),
    })
}

/// Apply a partial update to an existing recurring template.
///
/// Persists the patched template and bumps `last_mutation`. When the cadence
/// or `start_date` changes, all future open unpinned instances (deadline > now)
/// are deleted inside the same transaction so the next reconcile regenerates
/// them from the new cadence. Pinned, closed, and overdue instances are
/// preserved. Deleted instances lose their Google Calendar events and receive
/// new ones on the next sync. Other edits leave instances in place (the next
/// reconcile repositions them without Google Calendar churn).
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the input fails validation.
/// Returns [`AppError::NotFound`] if the template does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn update_template(
    store: &dyn Store,
    template_id: &str,
    input: UpdateTemplateInput,
    now: DateTime<Utc>,
) -> Result<RecurringTemplate, AppError> {
    validate_update_template(&input)?;

    let mut template = get_template(store, template_id)?;
    let old_cadence = template.cadence.clone();
    let old_start_date = template.start_date;

    if let Some(v) = input.title {
        template.title = v;
    }
    if let Some(v) = input.description {
        template.description = v;
    }
    if let Some(v) = input.duration_minutes {
        template.duration_minutes = v;
    }
    if let Some(v) = input.priority {
        template.priority = v;
    }
    if let Some(v) = input.schedule_id {
        template.schedule_id = v;
    }
    if let Some(v) = input.cadence {
        template.cadence = v;
    }
    if let Some(v) = input.labels {
        template.labels = v;
    }
    if let Some(v) = input.is_active {
        template.is_active = v;
    }
    if let Some(v) = input.start_date {
        template.start_date = normalize_anchor(v, store.get_config()?.timezone_tz()?);
    }

    // Covers reactivation of oversized templates and schedule reassignment.
    validate_schedule_capacity(store, template.duration_minutes, &template.schedule_id)?;

    template.updated_at = now;
    let cadence_or_anchor_changed =
        old_cadence != template.cadence || old_start_date != template.start_date;

    let mut config = store.get_config()?;
    config.last_mutation = Some(now);

    // Future open unpinned instances to delete (deadline > now only); the next
    // reconcile regenerates them from the new cadence.
    let stale: Vec<Task> = if cadence_or_anchor_changed {
        open_instances(store, &template.id)?
            .into_iter()
            .filter(|t| !t.is_pinned && t.deadline.is_some_and(|d| d > now))
            .collect()
    } else {
        vec![]
    };

    // Atomically delete stale instances, persist the updated template, and bump
    // last_mutation in a single transaction.
    store.with_tx(&mut |tx| {
        for instance in &stale {
            delete_instance(tx, instance)?;
        }
        tx.update_template(&template)?;
        tx.update_config(&config)
    })?;

    Ok(template)
}

/// Delete a recurring template.
///
/// # Errors
///
/// Returns [`AppError::NotFound`] if the template does not exist.
/// Returns [`AppError::Database`] on storage failure.
pub fn delete_template(
    store: &dyn Store,
    template_id: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    get_template(store, template_id)?;

    // Clean up instances BEFORE deleting template — the DB has
    // ON DELETE SET NULL on recurring_template_id, which would
    // nullify the FK and make subsequent queries return empty.
    let active_instances = open_instances(store, template_id)?;

    // De-link completed/cancelled instances (preserve history, break foreign
    // reference). Chunks on these tasks are preserved as historical data.
    let mut historical_instances = closed_instances(store, template_id)?;
    for instance in &mut historical_instances {
        instance.recurring_template_id = None;
        instance.updated_at = now;
    }

    let mut config = store.get_config()?;
    config.last_mutation = Some(now);

    store.with_tx(&mut |tx| {
        for instance in &active_instances {
            delete_instance(tx, instance)?;
        }
        for instance in &historical_instances {
            tx.update_task(instance)?;
        }
        tx.delete_template(template_id)?;
        tx.update_config(&config)
    })?;

    Ok(())
}

/// Return virtual (non-persisted) [`Task`] objects for active templates that
/// have no pending/scheduled instances.
///
/// These virtual tasks represent the "next" instance so the template remains
/// accessible in the UI (requirement M9.5).
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn get_orphaned_template_instances(
    store: &dyn Store,
    now: DateTime<Utc>,
) -> Result<Vec<Task>, AppError> {
    let templates = store.list_templates()?;
    let tz = store.get_config()?.timezone_tz()?;
    let mut virtual_tasks = Vec::new();

    for template in &templates {
        if !template.is_active {
            continue;
        }

        let existing = open_instances(store, &template.id)?;

        if !existing.is_empty() {
            continue;
        }

        // A validated cadence is never empty, so this always yields an
        // occurrence; the `if let` keeps generation infallible regardless.
        if let Some(next) = template
            .cadence
            .occurrences(template.start_date, tz)
            .find(|o| o.deadline >= now)
        {
            virtual_tasks.push(instance_from_template(
                template,
                format!("virtual-{}", template.id),
                next.start,
                next.deadline,
                None,
                now,
            ));
        }
    }

    Ok(virtual_tasks)
}

fn validate_schedule_capacity(
    store: &dyn Store,
    duration: i64,
    schedule_id: &str,
) -> Result<(), AppError> {
    let schedule = crate::services::schedule::get_schedule(store, schedule_id)?;
    validate_template_fits_schedule(duration, schedule.largest_window_minutes(), &schedule.name)
}

/// Normalize a recurrence anchor to the start of its day in `tz`.
///
/// The anchor is a day-precision concept — [`Cadence::occurrences`] floors it to
/// the period and only the first-window clamp ever reads its time-of-day.
/// Truncating to local midnight keeps the editor's date round-trip lossless (so
/// re-saving an untouched template never nudges the anchor) and makes repeated
/// writes idempotent.
fn normalize_anchor(anchor: DateTime<Utc>, tz: Tz) -> DateTime<Utc> {
    start_of_day(anchor.with_timezone(&tz).date_naive(), tz).with_timezone(&Utc)
}

fn instances_with_statuses(
    store: &dyn Store,
    template_id: &str,
    statuses: Vec<TaskStatus>,
) -> Result<Vec<Task>, AppError> {
    store.list_tasks(&TaskFilter {
        recurring_template_id: Some(template_id.to_owned()),
        statuses: Some(statuses),
        ..TaskFilter::default()
    })
}

fn open_instances(store: &dyn Store, template_id: &str) -> Result<Vec<Task>, AppError> {
    instances_with_statuses(
        store,
        template_id,
        vec![TaskStatus::Pending, TaskStatus::Scheduled],
    )
}

fn closed_instances(store: &dyn Store, template_id: &str) -> Result<Vec<Task>, AppError> {
    instances_with_statuses(
        store,
        template_id,
        vec![TaskStatus::Completed, TaskStatus::Cancelled],
    )
}

#[cfg(test)]
mod tests;
