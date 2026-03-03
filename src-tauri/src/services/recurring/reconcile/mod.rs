// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Single-pass, two-pointer reconciliation of recurring task instances.
//!
//! [`reconcile`] walks the desired occurrences and the existing instances in
//! lock-step (both ascending by deadline) and converges the two with the
//! minimum churn: an open instance is *repositioned in place* (patched by `id`,
//! preserving its Google Calendar event) rather than deleted and recreated.
//! See `SCHEDULER_ALGORITHM.md` §7 and `plans/recurring-reconciliation-rewrite.md`.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::domain::cadence::Occurrence;
use crate::domain::enums::{ChunkStatus, Priority, TaskStatus};
use crate::domain::inputs::TaskFilter;
use crate::domain::models::{RecurringTemplate, Task};
use crate::error::AppError;
use crate::traits::storage::Store;

/// A desired occurrence: its schedulable window (`start`..`deadline`) and when
/// it expires (see [`crate::domain::cadence::Cadence::expiry_for_occurrence`]).
struct Desired {
    start: DateTime<Utc>,
    deadline: DateTime<Utc>,
    expire_at: Option<DateTime<Utc>>,
}

/// Reconcile `template`'s instances within `(now, horizon]` against the cadence.
///
/// One forward pass over desired occurrences × existing future instances:
/// closed (Completed/Cancelled) instances keep the slots they own untouched;
/// user-pinned instances also keep their slots, timing, and sizing but their
/// template-owned identity is refreshed (see [`apply_template_identity`]);
/// other open instances are reused by `id` with their existing timing preserved
/// (null timing is backfilled from the cadence); surplus open instances are
/// deleted, and remaining occurrences are created.
/// No reschedule is triggered here — the scheduler pass that runs after
/// generation resolves any conflicts a moved deadline introduces.
///
/// # Errors
///
/// Returns [`AppError::Database`] on any storage failure.
// TODO(too-many-lines): split; task 019f9907-ed75-7552-8051-5fe4ffd2e01b
#[allow(clippy::too_many_lines)]
pub fn reconcile(
    store: &dyn Store,
    template: &RecurringTemplate,
    now: DateTime<Utc>,
    horizon: DateTime<Utc>,
    tz: &Tz,
) -> Result<(), AppError> {
    if !template.is_active {
        // ‹D0› Deactivated: delete open, unpinned instances — they will never be
        // scheduled again — but keep closed history and any the user has pinned.
        return delete_open_unpinned_instances(store, template);
    }

    let desired = desired_occurrences(template, now, horizon, *tz);

    let mut inst: Vec<(DateTime<Utc>, Task)> = store
        .list_tasks(&TaskFilter {
            recurring_template_id: Some(template.id.clone()),
            ..TaskFilter::default()
        })?
        .into_iter()
        .filter_map(|t| t.deadline.filter(|d| *d > now).map(|d| (d, t)))
        .collect();
    inst.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.created_at.cmp(&b.1.created_at))
    });

    let mut o = 0usize;
    let mut i = 0usize;

    while i < inst.len() {
        let (inst_deadline, task) = &inst[i];
        let closed = is_closed(task);
        if closed || task.is_pinned {
            // ‹D1› A sticky instance — closed history or user-pinned — owns every
            // desired slot up to its deadline; it is never repositioned or deleted.
            // A pinned instance is still live, though: pinning freezes timing and
            // sizing (its fixed chunk was placed at the old size), not identity —
            // a title/priority/label/schedule edit must still reach it. Closed
            // history receives nothing.
            if !closed {
                let mut patched = task.clone();
                apply_template_identity(&mut patched, template);
                if patched != *task {
                    patched.updated_at = now;
                    store.update_task(&patched)?;
                }
            }
            // Advance past desired slots this sticky instance covers. Use the
            // instance's immutable anchor (`start_date`) so:
            // (a) a monthly instance with a pre-widening narrow deadline still
            //     consumes its slot (start = 25th = desired[0].start, regardless
            //     of whether inst_deadline = 25th or 28th), and
            // (b) a weekly instance whose deadline was overridden into a later
            //     occurrence's span does not consume that later slot (its
            //     start_date is still the original occurrence's first day).
            let anchor = task.start_date.unwrap_or(*inst_deadline);
            while o < desired.len() && desired[o].start <= anchor {
                o += 1;
            }
            i += 1;
        } else if o < desired.len() {
            // Reuse this open instance: clone it, then refresh only template-owned
            // content (see `apply_template_content`). Its timing and progress
            // survive by construction; legacy rows with no timing are backfilled
            // from the cadence, and the derived expire_at is refreshed. An unchanged
            // instance stays equal (clone preserves updated_at), so the write is
            // skipped ‹D2›.
            let d = &desired[o];
            let mut patched = task.clone();
            apply_template_content(&mut patched, template);
            patched.start_date = Some(task.start_date.unwrap_or(d.start));
            patched.deadline = Some(
                template
                    .cadence
                    .deadline_for_reuse(task.deadline, d.deadline),
            );
            patched.expire_at = d.expire_at;
            if patched != *task {
                patched.updated_at = now;
                store.update_task(&patched)?; // reuse id → no GCal churn ‹D2›
            }
            o += 1;
            i += 1;
        } else {
            // ‹D3› surplus open instance with no occurrence left — delete it.
            delete_instance(store, task)?;
            i += 1;
        }
    }

    while o < desired.len() {
        let d = &desired[o];
        let instance = instance_from_template(
            template,
            Uuid::now_v7().to_string(),
            d.start,
            d.deadline,
            d.expire_at,
            now,
        );
        store.create_task(&instance)?;
        o += 1;
    }

    Ok(())
}

/// Delete all open, unpinned instances of `template`.
///
/// Preserves closed history (Completed/Cancelled) and any instance the user
/// has pinned. Used by [`reconcile`] when the template is deactivated (‹D0›).
///
/// # Errors
///
/// Returns [`AppError::Database`] on any storage failure.
fn delete_open_unpinned_instances(
    store: &dyn Store,
    template: &RecurringTemplate,
) -> Result<(), AppError> {
    let instances = store.list_tasks(&TaskFilter {
        recurring_template_id: Some(template.id.clone()),
        ..TaskFilter::default()
    })?;
    for task in instances {
        if !is_closed(&task) && !task.is_pinned {
            delete_instance(store, &task)?;
        }
    }
    Ok(())
}

fn is_closed(task: &Task) -> bool {
    matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled)
}

/// Desired occurrences whose deadline lands in `(now, horizon]`, each carrying
/// its `expire_at` via [`crate::domain::cadence::Cadence::expiry_for_occurrence`]
/// (the single expiry policy, period-aware). One occurrence past the horizon is
/// looked ahead to supply the last in-window slot's `expire_at` (‹D4›) but is
/// never persisted.
fn desired_occurrences(
    template: &RecurringTemplate,
    now: DateTime<Utc>,
    horizon: DateTime<Utc>,
    tz: Tz,
) -> Vec<Desired> {
    let mut in_window: Vec<Occurrence> = Vec::new();
    let mut next_occ_start: Option<DateTime<Utc>> = None;
    for occ in template.cadence.occurrences(template.start_date, tz) {
        if occ.deadline <= now {
            continue;
        }
        if occ.deadline <= horizon {
            in_window.push(occ);
        } else {
            next_occ_start = Some(occ.start);
            break;
        }
    }
    in_window
        .iter()
        .enumerate()
        .map(|(k, occ)| {
            let next = in_window.get(k + 1).map(|o| o.start).or(next_occ_start);
            Desired {
                start: occ.start,
                deadline: occ.deadline,
                expire_at: template.cadence.expiry_for_occurrence(occ, next, tz),
            }
        })
        .collect()
}

/// Remove the instance and its chunks from storage.
///
/// # Errors
///
/// Returns [`AppError::Database`] on any storage failure.
pub(super) fn delete_instance(store: &dyn Store, task: &Task) -> Result<(), AppError> {
    for chunk in store.get_chunks_for_task(&task.id)? {
        store.delete_chunk(&chunk.id)?;
    }
    store.delete_task(&task.id)?;
    Ok(())
}

/// Overwrite `task`'s template-owned *identity* fields with the template's
/// current values: title, description, priority, labels, and `schedule_id`.
///
/// Every live (non-closed) instance receives this — including pinned ones,
/// whose timing and sizing are frozen but whose identity must keep tracking
/// template edits. Reassigning `schedule_id` is safe for pinned instances
/// because their fixed chunks are window-exempt.
fn apply_template_identity(task: &mut Task, template: &RecurringTemplate) {
    task.title.clone_from(&template.title);
    task.description.clone_from(&template.description);
    task.priority = template.priority;
    task.schedule_id.clone_from(&template.schedule_id);
    task.labels.clone_from(&template.labels);
}

/// Overwrite `task`'s template-owned *sizing* fields: `duration_minutes` plus
/// the derived recurring shape (one un-splittable chunk of the whole
/// duration).
///
/// Only open, unpinned instances receive this — a pinned instance's fixed
/// chunk was placed at the old size, and resizing it implicitly would be
/// surprising.
fn apply_template_sizing(task: &mut Task, template: &RecurringTemplate) {
    task.duration_minutes = template.duration_minutes;
    // A recurring instance is one un-splittable chunk of the whole duration.
    task.min_chunk_minutes = template.duration_minutes;
    task.no_split = true;
}

/// Overwrite every template-owned field — identity + sizing — with the
/// template's current values, leaving the task's own id, timing, and progress
/// untouched.
///
/// Together with its two halves this is the single definition of what a
/// template *owns* on its instances, split by instance state: closed
/// (Completed/Cancelled) history receives neither, pinned instances receive
/// identity only, open unpinned instances receive both. Timing (`start_date`/
/// `deadline`/`expire_at`) and progress (`status`/`time_logged_minutes`/
/// `is_pinned`) are owned elsewhere — so reconcile applying only these
/// projections is what lets a live instance's manual deadline override and
/// logged progress survive a template edit.
///
/// (When `Task` grows a `TemplateContent` sub-struct this collapses to a single
/// field assignment — see the backlog refactor.)
fn apply_template_content(task: &mut Task, template: &RecurringTemplate) {
    apply_template_identity(task, template);
    apply_template_sizing(task, template);
}

/// Build a fresh recurring instance [`Task`] from its template: a Pending,
/// not-pinned, zero-logged instance whose `id`, timing (`start`/`deadline`/
/// `expire_at`), and `created_at`/`updated_at` (`now`) are supplied and whose
/// template-owned content is filled by [`apply_template_content`]. `start` is the
/// window's earliest-placement bound; `expire_at` is `None` for the virtual
/// (non-persisted) orphan instance (M9.5).
pub(super) fn instance_from_template(
    template: &RecurringTemplate,
    id: String,
    start: DateTime<Utc>,
    deadline: DateTime<Utc>,
    expire_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Task {
    let mut task = Task {
        id,
        recurring_template_id: Some(template.id.clone()),
        start_date: Some(start),
        deadline: Some(deadline),
        expire_at,
        status: TaskStatus::Pending,
        time_logged_minutes: 0,
        is_pinned: false,
        created_at: now,
        updated_at: now,
        // Template-owned fields are filled by apply_template_content below; these
        // are placeholders it overwrites.
        title: String::new(),
        description: None,
        duration_minutes: 0,
        priority: Priority::Low,
        schedule_id: String::new(),
        min_chunk_minutes: 0,
        no_split: false,
        labels: Vec::new(),
    };
    apply_template_content(&mut task, template);
    task
}

/// Cancel recurring-instance tasks whose `expire_at` has passed.
///
/// Iterates all `Pending`/`Scheduled` recurring instances; for any with
/// `expire_at` set where `now > expire_at`, deletes its scheduled chunks and
/// sets status to `Cancelled`. For weekly cadences `expire_at` is end of the
/// next window's first day (M4.5); for monthly it is end of the widened span
/// (the 28th). Instances with `expire_at == None` (legacy rows / virtual) never
/// auto-cancel. User-pinned instances are exempt and never auto-cancel.
///
/// # Errors
///
/// Returns [`AppError::Database`] on any storage failure.
pub fn auto_cancel_overdue(store: &dyn Store, now: DateTime<Utc>) -> Result<(), AppError> {
    let all_active = store.list_tasks(&TaskFilter {
        statuses: Some(vec![TaskStatus::Pending, TaskStatus::Scheduled]),
        ..TaskFilter::default()
    })?;

    for mut instance in all_active {
        if instance.recurring_template_id.is_none() {
            continue;
        }
        if instance.is_pinned {
            continue;
        }
        let Some(expire_at) = instance.expire_at else {
            continue; // never expires (legacy / virtual)
        };
        if now <= expire_at {
            continue;
        }

        let chunks = store.get_chunks_for_task(&instance.id)?;
        for chunk in chunks {
            if chunk.status == ChunkStatus::Scheduled {
                store.delete_chunk(&chunk.id)?;
            }
        }

        instance.status = TaskStatus::Cancelled;
        instance.updated_at = now;
        store.update_task(&instance)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
