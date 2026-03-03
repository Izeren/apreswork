// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Thin Tauri command wrappers for calendar auth, calendar picker persistence,
//! manual event pull, manual full sync, sync-status readout, and user-owned
//! event create/update/delete (G11).

// Tauri command signatures require by-value `State` and `Vec<String>` params;
// the `#[tauri::command]` macro handles extraction from IPC.
#![allow(clippy::needless_pass_by_value)]

use std::time::Instant;

use chrono::Utc;

use crate::domain::models::ExternalEventRecord;
use crate::error::AppError;
use crate::services::sync::SyncStatus;
use crate::state::{ActiveState, SyncWriteHandles};
use crate::traits::calendar_sync::{AuthStatus, ExternalCalendar, UserEventPayload};
use crate::traits::scheduling::ScheduleResult;

/// Run `f` on a blocking-safe worker thread (`spawn_blocking`) and await it,
/// mapping a cancelled/panicked task to [`AppError::Internal`]. `what` names
/// the task in the error message.
async fn run_blocking<T>(
    what: &str,
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(format!("{what} task failed: {e}")))?
}

/// Start the `OAuth2` loopback flow.
///
/// Returns the consent URL; the UI opens it in the system browser and polls
/// `google_auth_status`. The code→token exchange completes on a background
/// worker when the redirect arrives on the one-shot `127.0.0.1` listener.
///
/// # IMPORTANT
///
/// This command never opens anything itself. It only returns a URL string.
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if no provider is configured or the
/// flow cannot be started.
#[tauri::command]
pub fn begin_google_auth(active: tauri::State<'_, ActiveState>) -> Result<String, AppError> {
    let state = active.get()?;
    state.calendar_sync.begin_auth(Utc::now(), Instant::now())
}

/// Return the current authentication state without hitting the network.
///
/// The UI polls this while the browser flow is in flight to detect when the
/// exchange completes (`Connected`) or fails (`NotConnected`).
///
/// # Errors
///
/// Returns [`AppError::Validation`] when no profile is active.
#[tauri::command]
pub fn google_auth_status(active: tauri::State<'_, ActiveState>) -> Result<AuthStatus, AppError> {
    let state = active.get()?;
    Ok(state.calendar_sync.auth_status(Instant::now()))
}

/// Local-wipe disconnect.
///
/// Deletes the local token file and clears `google_auth`, `chunk_sync_state`,
/// and `external_events` in a single DB transaction. Remote data is never
/// touched. No reschedule is triggered — removing external events only relaxes
/// constraints, so freed slots are picked up at the next scheduled reschedule.
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if the provider-side disconnect fails
/// (the DB rows are then left intact so the user can retry).
/// Returns [`AppError::Database`] if the DB wipe fails.
#[tauri::command]
pub fn google_disconnect(active: tauri::State<'_, ActiveState>) -> Result<(), AppError> {
    let state = active.get()?;
    crate::services::sync::disconnect_provider(state.store.as_ref(), state.calendar_sync.as_ref())
}

/// Return the persisted calendar picker selection.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if the stored value is malformed.
/// Returns [`AppError::Database`] on storage failure.
#[tauri::command]
pub fn get_pull_calendars(active: tauri::State<'_, ActiveState>) -> Result<Vec<String>, AppError> {
    let state = active.get()?;
    crate::services::sync::get_pull_calendars(state.store.as_ref())
}

/// Persist the calendar picker selection.
///
/// No reschedule is triggered — the mirror is unchanged until the next pull.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if any calendar id is blank.
/// Returns [`AppError::Database`] on storage failure.
#[tauri::command]
pub fn set_pull_calendars(
    active: tauri::State<'_, ActiveState>,
    calendar_ids: Vec<String>,
) -> Result<(), AppError> {
    let state = active.get()?;
    crate::services::sync::set_pull_calendars(state.store.as_ref(), &calendar_ids)
}

/// List calendars visible to the connected account.
///
/// Uses `spawn_blocking` because the underlying provider makes blocking HTTP
/// calls that must not run on a tokio worker thread.
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] on provider or network error.
/// Returns [`AppError::Internal`] if the blocking task is cancelled.
#[tauri::command]
pub async fn google_list_calendars(
    active: tauri::State<'_, ActiveState>,
) -> Result<Vec<ExternalCalendar>, AppError> {
    let state = active.get()?;
    let sync = state.calendar_sync.clone();
    let now = chrono::Utc::now();
    run_blocking("calendar list", move || sync.list_calendars(now)).await
}

/// Manually refresh the external-event mirror then run a full reschedule.
///
/// Delegates to [`crate::services::sync::pull_and_reschedule`]: pull is outside
/// the mutation guard (network; must not hold a mutex across I/O), and the guard
/// is held only around the reschedule itself.
///
/// Uses `spawn_blocking` because the underlying provider makes blocking HTTP
/// calls that must not run on a tokio worker thread.
///
/// Shared by the `pull_external_events` Tauri command and the REST
/// `POST /api/calendar/pull` handler — each awaits this via its own
/// `spawn_blocking`-flavored `run_blocking` (see the "Provider / async lore"
/// note in `src-tauri/CLAUDE.md` for why those stay separate).
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if the pull or `list_events` call fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on scheduling
/// failure.
pub fn run_pull_and_reschedule(handles: SyncWriteHandles) -> Result<ScheduleResult, AppError> {
    let (store, sync, scheduler, trigger) = handles;
    crate::services::sync::pull_and_reschedule(
        store.as_ref(),
        sync.as_ref(),
        scheduler.as_ref(),
        trigger.as_ref(),
        Utc::now(),
    )
}

/// See [`run_pull_and_reschedule`] for the pull/reschedule behavior.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if the blocking task is cancelled.
/// Propagates any error from [`run_pull_and_reschedule`].
#[tauri::command]
pub async fn pull_external_events(
    active: tauri::State<'_, ActiveState>,
) -> Result<ScheduleResult, AppError> {
    let handles = active.sync_write_handles()?;
    run_blocking("pull", move || run_pull_and_reschedule(handles)).await
}

/// Manual full sync: pull the external mirror, fully reschedule, then push
/// chunks to the dedicated app calendar.
///
/// Delegates to [`crate::services::sync::sync_now`], which also records the
/// `last_sync_at` / `last_sync_error` bookkeeping read by `get_sync_status`.
///
/// Uses `spawn_blocking` because the underlying provider makes blocking HTTP
/// calls that must not run on a tokio worker thread.
///
/// Shared by the `sync_now` Tauri command and the REST `POST /api/sync/now`
/// handler — each awaits this via its own `spawn_blocking`-flavored
/// `run_blocking` (see the "Provider / async lore" note in
/// `src-tauri/CLAUDE.md` for why those stay separate).
///
/// # Errors
///
/// Returns [`AppError::CalendarSync`] if the pull or any push step fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on scheduling or
/// bookkeeping failure.
pub fn run_sync_now(
    handles: SyncWriteHandles,
) -> Result<crate::services::sync::SyncOutcome, AppError> {
    let (store, sync, scheduler, trigger) = handles;
    crate::services::sync::sync_now(
        store.as_ref(),
        sync.as_ref(),
        scheduler.as_ref(),
        trigger.as_ref(),
        Utc::now(),
    )
}

/// See [`run_sync_now`] for the sync behavior.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if the blocking task is cancelled.
/// Propagates any error from [`run_sync_now`].
#[tauri::command]
pub async fn sync_now(
    active: tauri::State<'_, ActiveState>,
) -> Result<crate::services::sync::SyncOutcome, AppError> {
    let handles = active.sync_write_handles()?;
    run_blocking("sync", move || run_sync_now(handles)).await
}

/// Return the last-sync bookkeeping for the Settings UI. No network call.
///
/// # Errors
///
/// Returns [`AppError::Database`] on storage failure.
#[tauri::command]
pub fn get_sync_status(active: tauri::State<'_, ActiveState>) -> Result<SyncStatus, AppError> {
    let state = active.get()?;
    crate::services::sync::get_sync_status(state.store.as_ref())
}

/// Create a user-owned calendar event, write it through to the provider, mirror
/// it locally, and reschedule. Returns the mirrored event record.
///
/// Delegates to [`crate::services::sync::create_user_event`]: the provider write
/// runs first (network; outside the mutation guard), then the local mirror write
/// and full reschedule run under the guard.
///
/// Uses `spawn_blocking` because the underlying provider makes blocking HTTP
/// calls that must not run on a tokio worker thread.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `calendar_id` is blank or the payload is
/// invalid (blank title, or start not before end).
/// Returns [`AppError::CalendarSync`] if the provider write fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on mirror-write,
/// scheduling, or lock failure, or if the blocking task is cancelled.
#[tauri::command]
pub async fn create_user_event(
    active: tauri::State<'_, ActiveState>,
    calendar_id: String,
    payload: UserEventPayload,
) -> Result<ExternalEventRecord, AppError> {
    let (store, sync, scheduler, trigger) = active.sync_write_handles()?;

    run_blocking("create user event", move || {
        crate::services::sync::create_user_event(
            store.as_ref(),
            sync.as_ref(),
            scheduler.as_ref(),
            trigger.as_ref(),
            &calendar_id,
            &payload,
            Utc::now(),
        )
    })
    .await
}

/// Update a user-owned calendar event, write through, re-mirror, and reschedule.
/// Returns the re-mirrored event record.
///
/// Delegates to [`crate::services::sync::update_user_event`]. The event must
/// already exist in the local mirror (trust-boundary guard); an unknown id yields
/// [`AppError::NotFound`] before any provider call.
///
/// Uses `spawn_blocking` because the underlying provider makes blocking HTTP
/// calls that must not run on a tokio worker thread.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `calendar_id`/`event_id` are blank or the
/// payload is invalid.
/// Returns [`AppError::NotFound`] if the event is not mirrored locally.
/// Returns [`AppError::CalendarSync`] if the provider write fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on mirror-write,
/// scheduling, or lock failure, or if the blocking task is cancelled.
#[tauri::command]
pub async fn update_user_event(
    active: tauri::State<'_, ActiveState>,
    calendar_id: String,
    event_id: String,
    payload: UserEventPayload,
) -> Result<ExternalEventRecord, AppError> {
    let (store, sync, scheduler, trigger) = active.sync_write_handles()?;

    run_blocking("update user event", move || {
        crate::services::sync::update_user_event(
            store.as_ref(),
            sync.as_ref(),
            scheduler.as_ref(),
            trigger.as_ref(),
            &calendar_id,
            &event_id,
            &payload,
            Utc::now(),
        )
    })
    .await
}

/// Delete a user-owned calendar event, remove it from the mirror, and reschedule.
///
/// Delegates to [`crate::services::sync::delete_user_event`]. The event must
/// already exist in the local mirror (trust-boundary guard); an unknown id yields
/// [`AppError::NotFound`] before any provider call.
///
/// Uses `spawn_blocking` because the underlying provider makes blocking HTTP
/// calls that must not run on a tokio worker thread.
///
/// # Errors
///
/// Returns [`AppError::Validation`] if `calendar_id`/`event_id` are blank.
/// Returns [`AppError::NotFound`] if the event is not mirrored locally.
/// Returns [`AppError::CalendarSync`] if the provider delete fails.
/// Returns [`AppError::Database`] or [`AppError::Internal`] on mirror-write,
/// scheduling, or lock failure, or if the blocking task is cancelled.
#[tauri::command]
pub async fn delete_user_event(
    active: tauri::State<'_, ActiveState>,
    calendar_id: String,
    event_id: String,
) -> Result<(), AppError> {
    let (store, sync, scheduler, trigger) = active.sync_write_handles()?;

    run_blocking("delete user event", move || {
        crate::services::sync::delete_user_event(
            store.as_ref(),
            sync.as_ref(),
            scheduler.as_ref(),
            trigger.as_ref(),
            &calendar_id,
            &event_id,
            Utc::now(),
        )
    })
    .await
}
