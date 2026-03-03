// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Calendar-sync service functions.
//!
//! Stateless free functions that orchestrate the `CalendarSync` trait and
//! the `Store` trait. `pull_external_events` refreshes the local mirror from
//! the provider; `disconnect_provider` wipes all local sync state on exit.

use chrono::{DateTime, Duration, SubsecRound, Utc};

use crate::domain::models::{ChunkSyncState, ExternalEventRecord, GoogleAuthState};
use crate::error::AppError;
use crate::services::trigger::RescheduleTrigger;
use crate::traits::calendar_sync::{
    CalendarSync, ChunkEventPayload, ExternalEvent, RemoteChunkEvent, SyncOp, SyncOpResult,
    UserEventPayload,
};
use crate::traits::scheduling::{ScheduleResult, Scheduler};
use crate::traits::storage::Store;

/// Disconnect the calendar provider and wipe all LOCAL sync state.
///
/// Decision 6 disconnect semantics: token file + `google_auth` +
/// `chunk_sync_state` + `external_events` are cleared. Remote data is never
/// touched.
///
/// The provider disconnect runs FIRST (outside the transaction). If it fails
/// the function returns the error immediately and the DB rows are left intact.
/// The caller can retry.
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if `sync.disconnect()` fails.
/// Returns [`AppError::Database`] if the DB wipe fails.
pub fn disconnect_provider(store: &dyn Store, sync: &dyn CalendarSync) -> Result<(), AppError> {
    sync.disconnect()?;

    store.with_tx(&mut |tx| {
        tx.clear_google_auth()?;
        tx.clear_all_chunk_sync_state()?;
        tx.clear_all_external_events()?;
        Ok(())
    })
}

/// Parse the `pull_calendar_ids` config value into a list of calendar IDs.
///
/// - `None` or blank/whitespace-only → `Ok(vec![])`
/// - Valid JSON array of strings → `Ok(ids)`
/// - Otherwise → `Err(AppError::Validation(...))`
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the value is not a JSON array of strings.
pub(crate) fn parse_pull_calendar_ids(raw: Option<&str>) -> Result<Vec<String>, AppError> {
    let Some(s) = raw else {
        return Ok(vec![]);
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    serde_json::from_str::<Vec<String>>(trimmed).map_err(|_| {
        AppError::Validation("pull_calendar_ids must be a JSON array of calendar ids".into())
    })
}

/// Read the Settings calendar picker selection.
///
/// Parses the `pull_calendar_ids` config key (a JSON array of strings). An
/// unset or blank key is treated as an empty selection — no Validation error.
///
/// This is the single authoritative read-side of the picker; all callers must
/// use this function rather than reading `pull_calendar_ids` directly so the
/// parse logic has one definition.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the stored value is not a valid JSON
/// array of strings.
/// Returns [`AppError::Database`] on storage failure.
pub fn get_pull_calendars(store: &dyn Store) -> Result<Vec<String>, AppError> {
    let raw = store.get_config_value("pull_calendar_ids")?;
    // Reuse the ONE parse definition so read and write sides share one format.
    parse_pull_calendar_ids(raw.as_deref())
}

/// Persist the Settings calendar picker selection.
///
/// Validates that every id is non-blank after trim (trust-boundary check —
/// input arrives over IPC). IDs are trimmed and deduplicated preserving
/// first-seen order before persisting as a JSON array.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if any calendar id is blank after trim.
/// Returns [`AppError::Internal`] if JSON serialization fails (practically
/// unreachable; `Vec<String>` always serialises).
/// Returns [`AppError::Database`] on storage failure.
pub fn set_pull_calendars(store: &dyn Store, ids: &[String]) -> Result<(), AppError> {
    // Validate and trim every id before writing (trust-boundary check).
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut deduped: Vec<String> = Vec::with_capacity(ids.len());
    for raw_id in ids {
        let trimmed = raw_id.trim();
        if trimmed.is_empty() {
            return Err(AppError::Validation(
                "calendar ids must be non-empty".into(),
            ));
        }
        if seen.insert(trimmed) {
            deduped.push(trimmed.to_owned());
        }
    }

    // Serialize in the same JSON-array format that parse_pull_calendar_ids reads.
    let json = serde_json::to_string(&deduped)
        .map_err(|e| AppError::Internal(format!("failed to serialise calendar ids: {e}")))?;
    store.set_config_value("pull_calendar_ids", &json)
}

/// Map a provider [`ExternalEvent`] into a storable [`ExternalEventRecord`],
/// stamping a fresh surrogate id and `updated_at = now`.
///
/// Shared by the mirror pull and the user-event write-through so both build
/// records identically. The surrogate `id` is discarded on an upsert conflict
/// (the store preserves the existing row id), so a fresh uuid here is harmless
/// for an event already mirrored.
fn external_event_to_record(event: ExternalEvent, now: DateTime<Utc>) -> ExternalEventRecord {
    ExternalEventRecord {
        id: uuid::Uuid::now_v7().to_string(),
        calendar_id: event.calendar_id,
        event_id: event.event_id,
        title: event.title,
        description: event.description,
        start_time: event.start,
        end_time: event.end,
        busy: event.busy,
        declined: event.declined,
        all_day: event.all_day,
        updated_at: now,
    }
}

/// Refresh the local `external_events` mirror from the provider.
///
/// Cleans up rows for calendars that are no longer selected, then for each
/// selected calendar fetches events in `[now - 7 days, horizon_end)` and
/// calls `replace_external_events_in_window` so deletions and updates are
/// applied atomically per calendar. Rows outside the window are retained.
///
/// Returns `Ok(())` immediately if the provider is unavailable or no calendars
/// are selected — the scheduler proceeds with whatever is already mirrored.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `pull_calendar_ids` is malformed.
/// Returns [`AppError::Database`] on storage failure.
/// Returns [`AppError::CalendarSync`] if any `list_events` call fails.
pub fn pull_external_events(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    if !sync.is_available() {
        return Ok(());
    }

    let ids = get_pull_calendars(store)?;
    if ids.is_empty() {
        return Ok(());
    }

    // TODO(atomicity): wrap in with_tx once cross-method support exists.
    let mirrored_calendar_ids = store.get_mirrored_calendar_ids()?;
    for deselected_id in mirrored_calendar_ids.iter().filter(|id| !ids.contains(id)) {
        store.delete_external_events_for_calendar(deselected_id)?;
    }

    let config = store.get_config()?;
    let window_start = now - Duration::days(7);
    let window_end = now + Duration::days(config.planning_horizon_days);

    for calendar_id in &ids {
        let started = std::time::Instant::now();
        let events = sync.list_events(now, calendar_id, window_start, window_end)?;
        let records: Vec<ExternalEventRecord> = events
            .into_iter()
            .map(|ev| external_event_to_record(ev, now))
            .collect();
        store.replace_external_events_in_window(calendar_id, window_start, window_end, &records)?;
        log::info!(
            "sync: pulled {} events from calendar {calendar_id} in {}ms",
            records.len(),
            started.elapsed().as_millis()
        );
    }

    Ok(())
}

/// Pull external events then run a full reschedule.
///
/// The pull runs outside any guard (network; must not hold a mutex across I/O).
/// The mutation guard is held only around the reschedule itself, matching the
/// `trigger_reschedule` pattern.
///
/// The reschedule runs even when the pull is a no-op (provider unavailable or
/// no calendars selected) so that any existing mirror data is incorporated.
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if the pull fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on scheduling or
/// lock failure.
pub fn pull_and_reschedule(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    now: DateTime<Utc>,
) -> Result<ScheduleResult, AppError> {
    pull_external_events(store, sync, now)?;
    let _guard = trigger.mutation_guard()?;
    crate::services::scheduling::reschedule(store, scheduler, now)
}

/// Reject a blank identifier arriving over the IPC/REST trust boundary.
fn require_non_blank(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation(format!("{field} must not be empty")));
    }
    Ok(())
}

/// Validate a user-event payload: non-blank title and a positive duration.
fn validate_user_event_payload(payload: &UserEventPayload) -> Result<(), AppError> {
    if payload.title.trim().is_empty() {
        return Err(AppError::Validation("event title must not be empty".into()));
    }
    if payload.start >= payload.end {
        return Err(AppError::Validation(
            "event start must be before end".into(),
        ));
    }
    Ok(())
}

/// Apply a local mirror mutation and full-reschedule under the mutation guard.
///
/// The provider write must already have completed OUTSIDE the guard (network
/// I/O must never be held across the mutation lock). This holds the guard across
/// the fast local mirror write and the reschedule so no other mutation
/// interleaves between them — the single definition of the "a user event
/// changed ⇒ full reschedule" policy.
fn commit_external_event_change(
    store: &dyn Store,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    now: DateTime<Utc>,
    mirror_op: impl FnOnce(&dyn Store) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let _guard = trigger.mutation_guard()?;
    // TODO(atomicity): the mirror write and the reschedule run in separate per-method
    // transactions; fold into one cross-store transaction once with_tx cross-method
    // support exists.
    mirror_op(store)?;
    crate::services::scheduling::reschedule(store, scheduler, now)?;
    Ok(())
}

/// Create a user-owned calendar event, write it through to the provider, mirror
/// it locally, and reschedule.
///
/// Write-through model: the provider is the source of truth. The event is
/// created remotely FIRST (no lock); on success the returned event is mirrored
/// into `external_events` and a full reschedule runs so chunks move around the
/// new busy block. The mirrored [`ExternalEventRecord`] is returned.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `calendar_id` is blank or the payload is
/// invalid (blank title, or start not before end).
/// Returns [`AppError::CalendarSync`] if the provider write fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on mirror-write,
/// scheduling, or lock failure.
pub fn create_user_event(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    calendar_id: &str,
    payload: &UserEventPayload,
    now: DateTime<Utc>,
) -> Result<ExternalEventRecord, AppError> {
    require_non_blank(calendar_id, "calendar id")?;
    validate_user_event_payload(payload)?;

    let event = sync.create_user_event(now, calendar_id, payload)?;
    let record = external_event_to_record(event, now);

    commit_external_event_change(store, scheduler, trigger, now, |s| {
        s.upsert_external_event(&record)
    })?;
    Ok(record)
}

/// Update a user-owned calendar event's time/content, write through, re-mirror,
/// and reschedule.
///
/// The event must already exist in the local mirror (defensive trust-boundary
/// check — the id arrives over IPC/REST); an unknown id yields
/// [`AppError::NotFound`] before any provider call.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `calendar_id`/`event_id` are blank or the
/// payload is invalid.
/// Returns [`AppError::NotFound`] if the event is not mirrored locally.
/// Returns [`AppError::CalendarSync`] if the provider write fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on mirror-write,
/// scheduling, or lock failure.
// Every argument is essential: the reschedule context (store/sync/scheduler/
// trigger, mirroring `pull_and_reschedule`), the event target (calendar_id +
// event_id), the new payload, and the injected clock. A context struct would
// diverge from the sibling sync signatures for no real gain.
#[allow(clippy::too_many_arguments)]
pub fn update_user_event(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    calendar_id: &str,
    event_id: &str,
    payload: &UserEventPayload,
    now: DateTime<Utc>,
) -> Result<ExternalEventRecord, AppError> {
    require_non_blank(calendar_id, "calendar id")?;
    require_non_blank(event_id, "event id")?;
    validate_user_event_payload(payload)?;

    // Trust-boundary: only act on mirrored events (defensive). The check runs
    // outside the guard, so concurrent pull could remove the row; TOCTOU is benign
    // — the re-mirror re-inserts the Google-confirmed row.
    let Some(existing) = store.get_external_event(calendar_id, event_id)? else {
        return Err(AppError::NotFound {
            entity: "external event".to_owned(),
            id: event_id.to_owned(),
        });
    };

    let event = sync.update_user_event(now, calendar_id, event_id, payload)?;
    let mut record = external_event_to_record(event, now);
    // Echo the persisted surrogate id: the upsert's ON CONFLICT keeps the existing
    // row id, so return it rather than the throwaway uuid external_event_to_record
    // minted (which the conflict path discards).
    record.id = existing.id;

    commit_external_event_change(store, scheduler, trigger, now, |s| {
        s.upsert_external_event(&record)
    })?;
    Ok(record)
}

/// Delete a user-owned calendar event, remove it from the mirror, and reschedule.
///
/// The event must already exist in the local mirror (defensive trust-boundary
/// check); an unknown id yields [`AppError::NotFound`] before any provider call.
/// The provider delete is idempotent, so a remote-side already-deleted event is
/// still a success there — this guard is purely about not acting on ids we never
/// mirrored.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `calendar_id`/`event_id` are blank.
/// Returns [`AppError::NotFound`] if the event is not mirrored locally.
/// Returns [`AppError::CalendarSync`] if the provider delete fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on mirror-write,
/// scheduling, or lock failure.
pub fn delete_user_event(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    calendar_id: &str,
    event_id: &str,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    require_non_blank(calendar_id, "calendar id")?;
    require_non_blank(event_id, "event id")?;

    // Trust-boundary guard: only act on events we already mirror. As in
    // `update_user_event`, the check sits outside the mutation guard; the TOCTOU is
    // benign — the provider delete is idempotent and the mirror delete is a no-op on
    // an already-absent row.
    if store.get_external_event(calendar_id, event_id)?.is_none() {
        return Err(AppError::NotFound {
            entity: "external event".to_owned(),
            id: event_id.to_owned(),
        });
    }

    sync.delete_user_event(now, calendar_id, event_id)?;

    commit_external_event_change(store, scheduler, trigger, now, |s| {
        s.delete_external_event(calendar_id, event_id)
    })?;
    Ok(())
}

/// Build the event description for one chunk of a task.
///
/// Single chunk: `"Après Work\n\n{description}"` (or just `"Après Work"`).
/// Multi-chunk: `"Part N of M — Après Work\n\n{description}"` (or just the header).
fn make_chunk_description(
    task: &crate::domain::models::Task,
    chunk_index: usize,
    total: usize,
) -> String {
    let marker = if total > 1 {
        format!("Part {} of {} — Après Work", chunk_index + 1, total)
    } else {
        "Après Work".to_owned()
    };
    match &task.description {
        Some(desc) if !desc.trim().is_empty() => {
            format!("{marker}\n\n{desc}").trim_end().to_owned()
        }
        _ => marker,
    }
}

fn chunk_index_in_task(
    chunk_id: &str,
    sorted_task_chunks: &[crate::domain::models::Chunk],
) -> usize {
    sorted_task_chunks
        .iter()
        .position(|c| c.id == chunk_id)
        .unwrap_or(0)
}

/// Recompute a chunk's sync base — `synced_title`/`synced_description` reflect
/// the chunk's current position within the task's ordering — and persist it via
/// `upsert_chunk_sync_state`. Silently returns if the owning task row is gone,
/// matching the pre-extraction `else continue`.
fn upsert_chunk_sync_base(
    tx: &dyn Store,
    chunk: &crate::domain::models::Chunk,
    event_id: &str,
    etag: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let Some(task) = tx.get_task(&chunk.task_id)? else {
        return Ok(());
    };
    let mut sorted = tx.get_chunks_for_task(&chunk.task_id)?;
    sorted.sort_by_key(|c| c.start_time);
    let idx = chunk_index_in_task(&chunk.id, &sorted);
    let desc = make_chunk_description(&task, idx, sorted.len());

    tx.upsert_chunk_sync_state(&ChunkSyncState {
        chunk_id: chunk.id.clone(),
        event_id: event_id.to_owned(),
        etag: etag.map(str::to_owned),
        synced_start: chunk.start_time,
        synced_end: chunk.end_time,
        synced_title: task.title.clone(),
        synced_description: desc,
        updated_at: now,
    })?;
    Ok(())
}

/// v1 remote-move validation: the moved end must not exceed the task's deadline.
fn validate_remote_move(new_end: DateTime<Utc>, task: &crate::domain::models::Task) -> bool {
    task.deadline.is_none_or(|dl| new_end <= dl)
}

/// Persist a `calendar_id` into the singleton `google_auth` row, preserving
/// `connected_at` from the existing row.
fn store_calendar_id(store: &dyn Store, calendar_id: &str) -> Result<(), AppError> {
    let connected_at = store.get_google_auth()?.and_then(|a| a.connected_at);
    store.set_google_auth(&GoogleAuthState {
        calendar_id: Some(calendar_id.to_owned()),
        connected_at,
    })
}

/// Compare two instants at the provider's storage precision (whole seconds).
///
/// Google Calendar truncates event times to whole seconds (verified live
/// 2026-07-16: a pushed sub-second time comes back truncated). Every
/// local-vs-base and remote-vs-base time comparison in the merge must use
/// this, or sub-second drift between what we recorded and what the provider
/// echoes reads as a permanent change and flags false conflicts.
fn times_match(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    a.trunc_subsecs(0) == b.trunc_subsecs(0)
}

/// How many provider events a [`sync_cycle`] push touched, by op kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct PushCounts {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
}

/// Push-sync outstanding chunk changes to the dedicated "Après Work" calendar.
///
/// Three-way merge: local state vs. remote state vs. the last-synced base.
/// Time comparisons happen at the provider's whole-second precision
/// ([`times_match`]).
///
/// # Algorithm (v1 — sequential, no batch)
///
/// 1. **Fetch** (no lock): list remote events on the app calendar in `[now, horizon)`.
/// 2. **Merge + local apply** (mutation guard + `with_tx`): compute sync ops,
///    accept/reject remote moves, delete locally any chunks the remote deleted.
/// 3. **Push** (no lock): execute computed ops on the provider.
/// 4. **Record bases** (mutation guard + `with_tx`): upsert `chunk_sync_state` rows
///    for successful creates/updates; handle accepted remote moves.
/// 5. **Reschedule** (mutation guard): incremental reschedule for tasks whose
///    chunks were affected by remote changes.
///
/// Returns the pushed-op counts; zero counts (immediately) when the provider
/// is unavailable.
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] on provider errors or if the mass-delete
/// guard triggers. Returns [`AppError::Database`] on storage failures.
// The four-phase sync algorithm is intentionally kept in one function so the
// sequence of guard acquisitions, network calls, and DB writes is auditable in
// one place. Extracting phases into helpers would hide the locking protocol.
#[allow(clippy::too_many_lines)]
// `SyncDecision` is defined after preliminary let-bindings because it captures
// only the fields that survive Step 2; defining it before the bindings would
// require forward-declaring variables it depends on.
#[allow(clippy::items_after_statements)]
pub fn sync_cycle(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    now: DateTime<Utc>,
) -> Result<PushCounts, AppError> {
    if !sync.is_available() {
        return Ok(PushCounts::default());
    }

    let calendar_id = {
        let auth = store.get_google_auth()?;
        if let Some(id) = auth.and_then(|a| a.calendar_id).filter(|s| !s.is_empty()) {
            id
        } else {
            let new_id = sync.ensure_app_calendar(now)?;
            store_calendar_id(store, &new_id)?;
            new_id
        }
    };

    let config = store.get_config()?;
    let horizon_end = now + Duration::days(config.planning_horizon_days);

    let started_fetch = std::time::Instant::now();
    let remote_events = sync.list_app_calendar_events(now, &calendar_id, now, horizon_end)?;
    log::info!(
        "sync_cycle: fetched {} remote events in {}ms",
        remote_events.len(),
        started_fetch.elapsed().as_millis()
    );
    let remote_by_event_id: std::collections::HashMap<&str, &RemoteChunkEvent> = remote_events
        .iter()
        .map(|e| (e.event_id.as_str(), e))
        .collect();

    struct SyncDecision {
        ops: Vec<SyncOp>,
        /// `(chunk_id, event_id, etag)` for remote moves accepted without a `SyncOp`.
        accepted_moves: Vec<(String, String, Option<String>)>,
        reschedule_task_ids: std::collections::HashSet<String>,
        /// Snapshot of `chunk.updated_at` at Step 2 for the Step 4 staleness check.
        chunk_updated_at_snapshot: std::collections::HashMap<String, DateTime<Utc>>,
    }

    let decision = {
        let _guard = trigger.mutation_guard()?;

        let bases = store.get_chunk_sync_states_in_range(now, horizon_end)?;
        let base_by_chunk_id: std::collections::HashMap<&str, &ChunkSyncState> =
            bases.iter().map(|b| (b.chunk_id.as_str(), b)).collect();
        let base_by_event_id: std::collections::HashMap<&str, &ChunkSyncState> =
            bases.iter().map(|b| (b.event_id.as_str(), b)).collect();

        let chunks_in_range = store.get_chunks_in_range(now, horizon_end)?;

        let mut ops: Vec<SyncOp> = vec![];
        let mut accepted_moves: Vec<(String, String, Option<String>)> = vec![];
        let mut reschedule_task_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut chunk_updated_at_snapshot: std::collections::HashMap<String, DateTime<Utc>> =
            std::collections::HashMap::new();
        let mut chunks_to_delete: Vec<String> = vec![];

        for chunk in &chunks_in_range {
            chunk_updated_at_snapshot.insert(chunk.id.clone(), chunk.updated_at);

            let Some(task) = store.get_task(&chunk.task_id)? else {
                continue;
            };

            let mut sorted_chunks = store.get_chunks_for_task(&chunk.task_id)?;
            sorted_chunks.sort_by_key(|c| c.start_time);
            let chunk_index = chunk_index_in_task(&chunk.id, &sorted_chunks);
            let total = sorted_chunks.len();

            let title = task.title.clone();
            let description = make_chunk_description(&task, chunk_index, total);

            let payload = ChunkEventPayload {
                chunk_id: chunk.id.clone(),
                title: title.clone(),
                description: description.clone(),
                start: chunk.start_time,
                end: chunk.end_time,
            };

            match base_by_chunk_id.get(chunk.id.as_str()) {
                None => {
                    ops.push(SyncOp::Create(payload));
                }
                Some(base) => {
                    match remote_by_event_id.get(base.event_id.as_str()) {
                        None => {
                            chunks_to_delete.push(chunk.id.clone());
                            reschedule_task_ids.insert(chunk.task_id.clone());
                        }
                        Some(remote) => {
                            let local_time_changed =
                                !times_match(chunk.start_time, base.synced_start)
                                    || !times_match(chunk.end_time, base.synced_end);
                            let local_content_changed = title != base.synced_title
                                || description != base.synced_description;
                            let remote_time_changed = !times_match(remote.start, base.synced_start)
                                || !times_match(remote.end, base.synced_end);

                            if !local_time_changed && !local_content_changed && !remote_time_changed
                            {
                                // Case C: nothing changed — skip.
                            } else if remote_time_changed
                                && !local_time_changed
                                && !local_content_changed
                            {
                                if validate_remote_move(remote.end, &task) {
                                    log::info!(
                                        "sync_cycle: accepting remote move for chunk {} (pinned)",
                                        chunk.id
                                    );
                                    accepted_moves.push((
                                        chunk.id.clone(),
                                        base.event_id.clone(),
                                        remote.etag.clone(),
                                    ));
                                    reschedule_task_ids.insert(chunk.task_id.clone());
                                } else {
                                    ops.push(SyncOp::Update {
                                        event_id: base.event_id.clone(),
                                        payload,
                                    });
                                }
                            } else {
                                if remote_time_changed {
                                    let cid = &chunk.id;
                                    log::info!(
                                        "sync_cycle: conflict on chunk {cid} — both changed, app wins"
                                    );
                                }
                                ops.push(SyncOp::Update {
                                    event_id: base.event_id.clone(),
                                    payload,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Mass-delete guard: abort if remote deleted too many synced chunks.
        let remote_deleted_count = chunks_to_delete.len();
        let synced_count = bases.len();
        if synced_count > 0 && remote_deleted_count > std::cmp::max(5, synced_count / 2) {
            return Err(AppError::CalendarSync(format!(
                "sync_cycle: mass delete guard triggered ({remote_deleted_count} of \
                 {synced_count} synced chunks missing from remote)"
            )));
        }

        // Sorted so the emitted Delete ops are deterministic across runs.
        let mut orphan_event_ids: Vec<&str> = remote_by_event_id
            .keys()
            .filter(|event_id| !base_by_event_id.contains_key(**event_id))
            .copied()
            .collect();
        orphan_event_ids.sort_unstable();
        for event_id in orphan_event_ids {
            ops.push(SyncOp::Delete {
                event_id: event_id.to_string(),
            });
        }

        let moves_to_apply: Vec<(String, DateTime<Utc>, DateTime<Utc>)> = accepted_moves
            .iter()
            .filter_map(|(chunk_id, event_id, _)| {
                remote_by_event_id
                    .get(event_id.as_str())
                    .map(|r| (chunk_id.clone(), r.start, r.end))
            })
            .collect();

        // TODO(atomicity): chunk mutations and chunk_sync_state updates should be one
        // cross-store transaction once with_tx cross-method support exists.
        store.with_tx(&mut |tx| {
            for chunk_id in &chunks_to_delete {
                tx.delete_chunk(chunk_id)?;
                // chunk_sync_state cascades automatically via ON DELETE CASCADE.
            }
            for (chunk_id, new_start, new_end) in &moves_to_apply {
                if let Some(mut chunk) = tx.get_chunk(chunk_id)? {
                    chunk.start_time = *new_start;
                    chunk.end_time = *new_end;
                    chunk.is_fixed = true;
                    chunk.updated_at = now;
                    tx.update_chunk(&chunk)?;
                }
            }
            Ok(())
        })?;

        log::info!(
            "sync_cycle: merge decided {} push ops, {} accepted moves, {} local deletes",
            ops.len(),
            accepted_moves.len(),
            chunks_to_delete.len()
        );

        SyncDecision {
            ops,
            accepted_moves,
            reschedule_task_ids,
            chunk_updated_at_snapshot,
        }
    }; // _guard dropped

    let started_push = std::time::Instant::now();
    let results = if decision.ops.is_empty() {
        vec![]
    } else {
        sync.execute_sync_ops(now, &calendar_id, &decision.ops)?
    };
    // The push is all-or-nothing (an error above propagates), so the op list
    // is exactly what reached the provider.
    let mut push_counts = PushCounts::default();
    for op in &decision.ops {
        match op {
            SyncOp::Create(_) => push_counts.created += 1,
            SyncOp::Update { .. } => push_counts.updated += 1,
            SyncOp::Delete { .. } => push_counts.deleted += 1,
        }
    }
    log::info!(
        "sync_cycle: pushed {} ops ({} created, {} updated, {} deleted) in {}ms",
        decision.ops.len(),
        push_counts.created,
        push_counts.updated,
        push_counts.deleted,
        started_push.elapsed().as_millis()
    );

    {
        let _guard = trigger.mutation_guard()?;
        store.with_tx(&mut |tx| {
            for (_, result) in decision.ops.iter().zip(results.iter()) {
                match result {
                    SyncOpResult::Created { chunk_id, event_id, etag }
                    | SyncOpResult::Updated { chunk_id, event_id, etag } => {
                        // Staleness check: skip if chunk changed since Step 2.
                        if let Some(chunk) = tx.get_chunk(chunk_id)? {
                            if decision.chunk_updated_at_snapshot.get(chunk_id)
                                != Some(&chunk.updated_at)
                            {
                                log::info!(
                                    "sync_cycle: chunk {chunk_id} changed during push, skipping base update"
                                );
                                continue;
                            }
                            upsert_chunk_sync_base(
                                tx,
                                &chunk,
                                event_id,
                                etag.as_deref(),
                                now,
                            )?;
                        }
                    }
                    SyncOpResult::Deleted => {
                        // Chunk already deleted in Step 2 → base cascaded. Nothing to do.
                    }
                }
            }

            // Update bases for accepted remote moves (chunk was moved locally; no SyncOp issued).
            for (chunk_id, event_id, etag) in &decision.accepted_moves {
                if let Some(chunk) = tx.get_chunk(chunk_id)? {
                    upsert_chunk_sync_base(tx, &chunk, event_id, etag.as_deref(), now)?;
                }
            }

            Ok(())
        })?;
    } // _guard dropped

    // Trigger incremental reschedule for tasks affected by remote changes.
    if !decision.reschedule_task_ids.is_empty() {
        let _guard = trigger.mutation_guard()?;
        let task_ids: Vec<String> = decision.reschedule_task_ids.into_iter().collect();
        crate::services::scheduling::reschedule_incremental(store, scheduler, &task_ids, now)?;
    }

    Ok(push_counts)
}

/// Sync bookkeeping surfaced to the Settings UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SyncStatus {
    /// Completion time of the last successful [`sync_now`], if any.
    pub last_sync_at: Option<DateTime<Utc>>,
    /// Message of the last failed sync; `None` after a successful sync.
    pub last_sync_error: Option<String>,
}

/// Read the sync bookkeeping recorded by [`sync_now`].
///
/// Lenient on a malformed `last_sync_at` (returns `None` rather than an
/// error): the value is display-only and self-heals on the next successful
/// sync. A blank `last_sync_error` (the migration seed) maps to `None`.
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
pub fn get_sync_status(store: &dyn Store) -> Result<SyncStatus, AppError> {
    let last_sync_at = store
        .get_config_value("last_sync_at")?
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    let last_sync_error = store
        .get_config_value("last_sync_error")?
        .filter(|s| !s.trim().is_empty());
    Ok(SyncStatus {
        last_sync_at,
        last_sync_error,
    })
}

/// Result of a manual sync surfaced to the UI: the full-reschedule result
/// plus how many provider events the push touched.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncOutcome {
    /// Full-reschedule result (placed chunks + warnings).
    pub schedule: ScheduleResult,
    /// Provider events created/updated/deleted by the push.
    pub pushed: PushCounts,
}

/// Manual "Sync now": pull the external mirror, fully reschedule, then push.
///
/// Composition order matters: the full reschedule runs BEFORE the push so the
/// events pushed to the provider reflect final chunk placements. Remote edits
/// made since the last sync are absorbed by [`sync_cycle`]'s three-way merge
/// afterwards; its incremental reschedule (remote deletes) may still adjust
/// chunks post-push — those adjustments reach the provider on the next sync.
///
/// Records bookkeeping when the provider is available: `last_sync_at` +
/// cleared `last_sync_error` on success, the error message on failure. While
/// disconnected the bookkeeping is untouched — "last sync" means last provider
/// sync, not last local reschedule.
///
/// Returns a [`SyncOutcome`]: the full reschedule's
/// [`ScheduleResult`](crate::traits::scheduling::ScheduleResult) plus the
/// pushed-op counts (all zero while disconnected).
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if the pull or any push step fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on scheduling,
/// lock, or bookkeeping-write failure.
pub fn sync_now(
    store: &dyn Store,
    sync: &dyn CalendarSync,
    scheduler: &dyn Scheduler,
    trigger: &RescheduleTrigger,
    now: DateTime<Utc>,
) -> Result<SyncOutcome, AppError> {
    let started = std::time::Instant::now();
    let outcome = pull_and_reschedule(store, sync, scheduler, trigger, now).and_then(|schedule| {
        sync_cycle(store, sync, scheduler, trigger, now)
            .map(|pushed| SyncOutcome { schedule, pushed })
    });

    if sync.is_available() {
        match &outcome {
            Ok(_) => {
                store.set_config_value("last_sync_at", &now.to_rfc3339())?;
                store.set_config_value("last_sync_error", "")?;
            }
            Err(e) => store.set_config_value("last_sync_error", &e.to_string())?,
        }
    }
    log::info!("sync_now: total {}ms", started.elapsed().as_millis());
    outcome
}

#[cfg(test)]
mod test_util;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod sync_cycle_tests;

#[cfg(test)]
mod sync_now_tests;

#[cfg(test)]
mod user_event_tests;
