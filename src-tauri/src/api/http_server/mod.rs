// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Axum HTTP server setup, router, and startup helpers.
//!
//! The server binds to `127.0.0.1:PORT` only. It is process-scoped: its
//! state is the swappable [`ActiveState`] slot shared with the Tauri
//! commands, and every handler resolves the active profile's `AppState` per
//! request — so an in-process profile switch redirects subsequent requests
//! to the new profile, and requests with no profile active get a `400`.
//!
//! Environment variables:
//! - `APRESWORK_API_PORT`    — TCP port (default `19532`)
//! - `APRESWORK_API_ENABLED` — `"false"` disables the server (default `"true"`)

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{FromRef, Path, Query, Request, State};
use axum::http::header::HOST;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::commands::profile_commands::ProfileInfo;
use crate::domain::enums::{Priority, TaskStatus};
use crate::domain::inputs::{
    CreateCommentInput, CreateTaskInput, TaskFilter, UpdateCommentInput, UpdateTaskInput,
};
use crate::error::AppError;
use crate::profiles::registry::{ProfilesRegistry, REGISTRY_VERSION};
use crate::profiles::ProfilesState;
use crate::services::comment::DEFAULT_AUTHOR;
use crate::services::trigger::Mutation;
use crate::state::ActiveState;
use crate::traits::calendar_sync::AuthStatus;

pub const DEFAULT_API_PORT: u16 = 19532;

/// HTTP response representation of [`AppError`].
///
/// # Security
///
/// 5xx errors (`Database`, `Internal`, `CalendarSync`, `Backup`) intentionally return
/// only a generic message.  The real error is logged server-side so it never reaches
/// the client.  This prevents information leakage of internal state or SQL.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match &self {
            AppError::NotFound { entity, id } => (
                StatusCode::NOT_FOUND,
                "not_found",
                format!("Not found: {entity} with id {id}"),
            ),
            AppError::Validation(msg) => (StatusCode::BAD_REQUEST, "validation", msg.clone()),
            // SECURITY: log real error, return generic message to client.
            AppError::Database(_) => {
                log::error!("API database error: {self}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database",
                    "A database error occurred.".to_owned(),
                )
            }
            AppError::CalendarSync(_) => {
                log::error!("API calendar sync error: {self}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "calendar_sync",
                    "A calendar sync error occurred.".to_owned(),
                )
            }
            AppError::Backup(_) => {
                log::error!("API backup error: {self}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "backup",
                    "A backup error occurred.".to_owned(),
                )
            }
            AppError::Internal(_) => {
                log::error!("API internal error: {self}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "An internal server error occurred.".to_owned(),
                )
            }
            // 4xx: user-actionable — echo the message directly.
            AppError::ProfileMismatch(msg) => {
                (StatusCode::CONFLICT, "profile_mismatch", msg.clone())
            }
        };
        let body = json!({ "error": error_code, "message": message });
        (status, Json(body)).into_response()
    }
}

/// Combined Axum router state: the swappable active-profile slot plus the
/// process-scoped profiles registry.
///
/// `FromRef` projections let handlers and middleware each extract the sub-state
/// they need via `State<ActiveState>` or `State<Arc<ProfilesState>>` without
/// coupling them to the full struct.
#[derive(Clone)]
pub struct RouterState {
    pub(crate) active: ActiveState,
    pub(crate) profiles: Arc<ProfilesState>,
}

impl FromRef<RouterState> for ActiveState {
    fn from_ref(state: &RouterState) -> Self {
        state.active.clone()
    }
}

impl FromRef<RouterState> for Arc<ProfilesState> {
    fn from_ref(state: &RouterState) -> Self {
        state.profiles.clone()
    }
}

/// Build the Axum [`Router`] backed by [`RouterState`].
///
/// Production entry point: both the active-profile slot and the profiles
/// registry are accessible to handlers via `FromRef` projections
/// (`State<ActiveState>` and `State<Arc<ProfilesState>>`).
///
/// All mutating write endpoints support an optional `?expected_profile_id=`
/// guard enforced by [`profile_guard_middleware`] (grouped into a
/// `guarded_writes` sub-router so the guard runs before any handler on those
/// routes).  Read-only endpoints are in the main router and are unaffected.
pub fn build_router_with_profiles(
    active: impl Into<ActiveState>,
    profiles: Arc<ProfilesState>,
) -> Router {
    let state = RouterState {
        active: active.into(),
        profiles,
    };

    let guarded_writes = Router::new()
        .route("/api/tasks", post(create_task_handler))
        .route(
            "/api/tasks/{id}",
            delete(delete_task_handler).patch(update_task_handler),
        )
        .route("/api/tasks/{id}/complete", post(complete_task_handler))
        .route("/api/tasks/{id}/comments", post(create_comment_handler))
        .route(
            "/api/comments/{id}",
            delete(delete_comment_handler).patch(update_comment_handler),
        )
        .route("/api/chunks/{id}/move", post(move_chunk_handler))
        .layer(middleware::from_fn_with_state(
            state.active.clone(),
            profile_guard_middleware,
        ));

    Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/profile", get(get_profile_handler))
        .route("/api/profile/switch", post(switch_profile_handler))
        .route("/api/profiles", get(list_profiles_handler))
        .route("/api/tasks", get(list_tasks_handler))
        .route("/api/agenda", get(get_agenda_handler))
        .route("/api/labels", get(list_labels_handler))
        .route("/api/tasks/{id}", get(get_task_handler))
        .route("/api/tasks/{id}/comments", get(list_comments_handler))
        .route("/api/auth/google/begin", post(google_auth_begin_handler))
        .route("/api/auth/google/status", get(google_auth_status_handler))
        .route(
            "/api/auth/google/disconnect",
            post(google_auth_disconnect_handler),
        )
        .route("/api/calendar/pull", post(calendar_pull_handler))
        .route("/api/sync/now", post(sync_now_handler))
        .route("/api/sync/status", get(sync_status_handler))
        .route("/api/backup/now", post(backup_now_handler))
        .route("/api/backup/status", get(backup_status_handler))
        .merge(guarded_writes)
        .layer(middleware::from_fn(validate_host_header))
        .with_state(state)
}

/// Build the Axum [`Router`] with an empty profiles registry.
///
/// Convenience wrapper for tests; `GET /api/profiles` will return `[]`.
/// Production code should use [`build_router_with_profiles`].
pub fn build_router(state: impl Into<ActiveState>) -> Router {
    build_router_with_profiles(
        state,
        Arc::new(ProfilesState::new(
            std::path::PathBuf::new(),
            ProfilesRegistry {
                version: REGISTRY_VERSION,
                last_used: None,
                profiles: vec![],
            },
        )),
    )
}

/// Middleware that enforces the optional profile-write guard.
///
/// If the request carries `?expected_profile_id=<id>` and the active profile's
/// id does not match, this middleware short-circuits with `409 Conflict`
/// **before** the handler runs. An absent query parameter is a no-op.
///
/// Applied to the `guarded_writes` sub-router (all mutating endpoints: create /
/// update / delete task, complete task, create / update / delete comment, move
/// chunk) via [`build_router`].
///
/// # Security
///
/// Profile IDs are UUIDs (lowercase hex + hyphens); no special characters
/// require percent-decoding, so a simple `split_once` is safe.
async fn profile_guard_middleware(
    State(active): State<ActiveState>,
    request: Request,
    next: Next,
) -> Response {
    let expected_id = request.uri().query().and_then(|qs| {
        qs.split('&').find_map(|part| {
            let (k, v) = part.split_once('=')?;
            (k == "expected_profile_id").then_some(v.to_owned())
        })
    });
    if let Some(expected_id) = expected_id {
        if let Err(e) = check_profile_mismatch(&active, &expected_id) {
            return e.into_response();
        }
    }
    next.run(request).await
}

/// Validates the optional optimistic-concurrency guard `expected_profile_id`.
///
/// Returns `Ok(())` when the active profile ID matches `expected_id`, or
/// `Err` on mismatch or active-profile lock failure.
fn check_profile_mismatch(active: &ActiveState, expected_id: &str) -> Result<(), AppError> {
    match active.get() {
        Err(e) => Err(e),
        Ok(state) if state.profile.id != expected_id => Err(AppError::ProfileMismatch(format!(
            "expected profile '{}' but active profile is '{}'",
            expected_id, state.profile.id
        ))),
        Ok(_) => Ok(()),
    }
}

/// Reject requests whose `Host` header is not a loopback host.
///
/// # Security
///
/// Defends against browser-based DNS rebinding (REQUIREMENTS W6 decision, see
/// DESIGN §6.2): a malicious page re-resolves its own hostname to `127.0.0.1`
/// and issues same-origin `fetch`es that the browser delivers to this server —
/// carrying the attacker's hostname in `Host`. CORS never applies to
/// same-origin requests, so this allowlist is the boundary. Browsers always
/// send `Host` (HTTP/1.1 requires it), so a missing header is rejected too.
///
/// The port part is ignored: rebinding forges the host part only, and the
/// server binds loopback exclusively. IPv6 `::1` is deliberately absent from
/// the allowlist — the listener binds `127.0.0.1` (v4) only.
async fn validate_host_header(request: Request, next: Next) -> Response {
    let host_ok = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            let host = host.rsplit_once(':').map_or(host, |(name, _port)| name);
            host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost")
        });

    if host_ok {
        next.run(request).await
    } else {
        let body = json!({
            "error": "forbidden",
            "message": "Host header must be 127.0.0.1 or localhost."
        });
        (StatusCode::FORBIDDEN, Json(body)).into_response()
    }
}

/// Reports the server process, not a profile; answers before any profile is unlocked.
async fn health_handler() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

/// Endpoint for agents and scripts to assert whose profile data they are touching (e.g. `api.sh whoami`).
async fn get_profile_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    Ok(Json(state.profile.clone()))
}

/// Return all profiles from registry (does not require a profile to be unlocked; registry is always available).
async fn list_profiles_handler(
    State(profiles): State<Arc<ProfilesState>>,
) -> Result<impl IntoResponse, AppError> {
    let registry = profiles.lock_registry()?;
    let list: Vec<ProfileInfo> = registry.profiles.iter().map(ProfileInfo::from).collect();
    Ok(Json(list))
}

/// `POST /api/profile/switch` — switch the running app to a different profile.
///
/// Atomically flushes the old profile's backup and activates the new one.
/// Returns the new profile's identity `{ id, name }` on success, or:
///
/// - `409 Conflict` when `expected_profile_id` is provided and does not match
///   the active profile (optimistic-concurrency guard).
/// - `404 Not Found` when `profile_id` is not in the registry.
/// - `400 Bad Request` when `expected_profile_id` is provided but no profile
///   is currently active.
///
/// Switching to the already-active profile is a no-op and returns `200` with
/// the current identity.
async fn switch_profile_handler(
    State(active): State<ActiveState>,
    State(profiles): State<Arc<ProfilesState>>,
    Json(body): Json<SwitchProfileBody>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(expected_id) = &body.expected_profile_id {
        check_profile_mismatch(&active, expected_id)?;
    }

    if let Ok(state) = active.get() {
        if state.profile.id == body.profile_id {
            return Ok(Json(state.profile.clone()));
        }
    }

    let (entry, data_dir) = {
        let mut registry = profiles.lock_registry()?;
        let entry = registry
            .find(&body.profile_id)
            .ok_or_else(|| AppError::NotFound {
                entity: "Profile".to_owned(),
                id: body.profile_id.clone(),
            })?
            .clone();
        // mark_last_used before switch: on failure the profile gate handles
        // re-activation; next startup retries the target profile as intended.
        crate::profiles::service::mark_last_used(
            &mut registry,
            &profiles.data_dir,
            &body.profile_id,
        )?;
        (entry, profiles.data_dir.clone())
    };

    run_blocking("profile switch", move || {
        crate::profiles::activate::switch_active_profile_direct(
            &active,
            &data_dir,
            &entry,
            chrono::Utc::now(),
        )
    })
    .await
    .map(Json)
}

/// Deserialized query parameters for `GET /api/tasks`.
///
/// `status`/`statuses`, `priority`/`priorities`, `labels`, and
/// `excluded_labels` accept comma-separated values (e.g.
/// `?statuses=pending,scheduled`, `?priorities=High,Critical`). Multiple
/// `statuses` or `priorities` match any of them; multiple `labels` are
/// match-all (the task must carry every one); a task carrying any
/// `excluded_labels` entry is dropped. Priority values are `PascalCase`.
/// `unlabeled` is `true` (only tasks with no labels) or `false` (only labeled
/// tasks). `search_text` is a substring search on title and description.
#[derive(Debug, Default, Deserialize)]
struct TaskListQuery {
    #[serde(alias = "statuses")]
    status: Option<String>,
    #[serde(alias = "priorities")]
    priority: Option<String>,
    labels: Option<String>,
    excluded_labels: Option<String>,
    unlabeled: Option<String>,
    search_text: Option<String>,
}

/// Deserialized query parameters for `GET /api/agenda`.
#[derive(Debug, Deserialize)]
struct AgendaQuery {
    start: String,
    end: String,
    label: Option<String>,
}

fn parse_datetime(raw: &str, field: &str) -> Result<DateTime<Utc>, AppError> {
    raw.parse::<DateTime<Utc>>()
        .map_err(|_| AppError::Validation(format!("invalid {field} datetime: '{raw}'")))
}

fn tokenize_csv(raw: &str) -> Vec<&str> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a comma-separated string into `Vec<T>` where `T: Deserialize`.
///
/// Each token is trimmed and passed through `serde_json` string deserialization.
/// Returns a `Validation` error if any token is unrecognised.
fn parse_csv<T>(raw: &str, field: &str) -> Result<Vec<T>, AppError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    tokenize_csv(raw)
        .into_iter()
        .map(|token| {
            serde_json::from_value::<T>(json!(token))
                .map_err(|_| AppError::Validation(format!("invalid {field} value: '{token}'")))
        })
        .collect()
}

/// Parse a comma-separated label list; `None` when no non-empty tokens remain.
fn parse_label_csv(raw: &str) -> Option<Vec<String>> {
    let labels: Vec<String> = tokenize_csv(raw).into_iter().map(str::to_owned).collect();
    (!labels.is_empty()).then_some(labels)
}

/// Convert [`TaskListQuery`] to a [`TaskFilter`].
///
/// Returns a [`AppError::Validation`] error if any query parameter value is
/// unrecognised (e.g. an invalid status or priority string).
fn query_to_filter(q: TaskListQuery) -> Result<TaskFilter, AppError> {
    let statuses = q
        .status
        .as_deref()
        .map(|raw| parse_csv::<TaskStatus>(raw, "status"))
        .transpose()?;

    let priorities = q
        .priority
        .as_deref()
        .map(|raw| parse_csv::<Priority>(raw, "priority"))
        .transpose()?;

    let labels = q.labels.as_deref().and_then(parse_label_csv);
    let excluded_labels = q.excluded_labels.as_deref().and_then(parse_label_csv);

    let unlabeled = q
        .unlabeled
        .as_deref()
        .map(|raw| match raw.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(AppError::Validation(format!(
                "invalid unlabeled value: '{other}'"
            ))),
        })
        .transpose()?;

    Ok(TaskFilter {
        statuses,
        priorities,
        labels,
        excluded_labels,
        unlabeled,
        search_text: q.search_text,
        ..TaskFilter::default()
    })
}

async fn list_tasks_handler(
    State(active): State<ActiveState>,
    Query(query): Query<TaskListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let filter = query_to_filter(query)?;
    let tasks = crate::services::task::list_tasks(state.store.as_ref(), &filter)?;
    Ok(Json(tasks))
}

async fn get_agenda_handler(
    State(active): State<ActiveState>,
    Query(query): Query<AgendaQuery>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let start = parse_datetime(&query.start, "start")?;
    let end = parse_datetime(&query.end, "end")?;
    let label_vec = query.label.map(|label| vec![label]);
    let label_filter = label_vec.as_deref();
    let items = crate::services::task::get_agenda(state.store.as_ref(), start, end, label_filter)?;
    Ok(Json(items))
}

async fn create_task_handler(
    State(active): State<ActiveState>,
    Json(input): Json<CreateTaskInput>,
) -> Result<impl IntoResponse, AppError> {
    let task = crate::commands::task_commands::create_task_guarded(active.get()?.as_ref(), input)?;
    Ok((StatusCode::CREATED, Json(task)))
}

async fn get_task_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let task = crate::services::task::get_task(state.store.as_ref(), &id)?;
    Ok(Json(task))
}

/// `PATCH /api/tasks/:id` — apply a partial update to an existing task.
///
/// Mirrors the `update_task` Tauri command: both surfaces report the same
/// [`Mutation::TaskUpdated`] and the shared trigger policy decides the
/// reschedule (a transition to `Backlog` frees the task's auto-scheduled
/// slots, so it maps to a full reschedule).
async fn update_task_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTaskInput>,
) -> Result<impl IntoResponse, AppError> {
    let task =
        crate::commands::task_commands::update_task_guarded(active.get()?.as_ref(), &id, input)?;
    Ok(Json(task))
}

/// Labels are unioned across tasks and recurring templates; `task_count`
/// counts tasks only (`0` for template-only labels).
async fn list_labels_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let labels = crate::services::task::list_labels(state.store.as_ref())?;
    Ok(Json(labels))
}

/// `DELETE /api/tasks/:id` — delete a task; a recurring instance is cancelled
/// instead so its cadence slot stays occupied.
///
/// Mirrors the `delete_task` Tauri command: both surfaces report
/// [`Mutation::TaskDeleted`] and the shared trigger policy runs a full
/// reschedule so freed slots can be reallocated. Returns `204 No Content`.
async fn delete_task_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    crate::commands::task_commands::delete_task_guarded(active.get()?.as_ref(), &id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete_task_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let task = crate::commands::task_commands::complete_task_guarded(active.get()?.as_ref(), &id)?;
    Ok(Json(task))
}

/// Request body for `POST /api/profile/switch`.
///
/// `expected_profile_id` is an optional optimistic-concurrency guard: if
/// provided and the active profile's id does not match, the handler returns
/// `409 Conflict` before attempting the switch. Callers that do not need this
/// guard may omit the field entirely.
#[derive(Debug, Deserialize)]
struct SwitchProfileBody {
    profile_id: String,
    expected_profile_id: Option<String>,
}

/// Request body for `POST /api/chunks/{id}/move`.
///
/// Datetimes are RFC 3339 / ISO 8601 strings (e.g. `2026-06-28T20:04:05Z`),
/// parsed into UTC so a malformed value yields a `400` validation error rather
/// than an opaque deserialization failure.
#[derive(Debug, Deserialize)]
struct MoveChunkBody {
    new_start: String,
    new_end: String,
}

/// Move a chunk to a new time range, pinning it. Mirrors the `move_chunk`
/// Tauri command: a debounced incremental reschedule is triggered for its task.
/// Works on both scheduled and completed chunks (used to backfill mis-placed
/// completed work).
async fn move_chunk_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
    Json(body): Json<MoveChunkBody>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let new_start = parse_datetime(&body.new_start, "new_start")?;
    let new_end = parse_datetime(&body.new_end, "new_end")?;
    let chunk = {
        let _guard = state.trigger.mutation_guard()?;
        crate::services::task::move_chunk(
            state.store.as_ref(),
            &id,
            new_start,
            new_end,
            Utc::now(),
        )?
    };
    state.trigger.trigger_mutation(Mutation::ChunkMoved {
        task_id: chunk.task_id.clone(),
    })?;
    Ok(Json(chunk))
}

/// Request body for `POST /api/tasks/{id}/comments`.
///
/// `author` defaults to the shared default author when omitted; the reserved
/// system author is rejected by the service (M12.2/M12.10).
#[derive(Debug, Deserialize)]
struct CreateCommentBody {
    content: String,
    author: Option<String>,
}

async fn list_comments_handler(
    State(active): State<ActiveState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let comments = crate::services::comment::list_comments(state.store.as_ref(), &task_id)?;
    Ok(Json(comments))
}

/// Create a comment on a task. Comments never affect scheduling, so no
/// reschedule trigger fires.
async fn create_comment_handler(
    State(active): State<ActiveState>,
    Path(task_id): Path<String>,
    Json(body): Json<CreateCommentBody>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let input = CreateCommentInput {
        task_id,
        content: body.content,
        author: body.author,
    };
    let comment =
        crate::services::comment::create_comment(state.store.as_ref(), input, Utc::now())?;
    Ok((StatusCode::CREATED, Json(comment)))
}

/// Edit a comment's content as the default author. System comments and
/// comments by other authors are rejected (M12.3).
async fn update_comment_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateCommentInput>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let comment = crate::services::comment::update_comment(
        state.store.as_ref(),
        &id,
        &input,
        DEFAULT_AUTHOR,
        Utc::now(),
    )?;
    Ok(Json(comment))
}

/// Delete a comment as the default author. Returns `204 No Content`.
async fn delete_comment_handler(
    State(active): State<ActiveState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    crate::services::comment::delete_comment(state.store.as_ref(), &id, DEFAULT_AUTHOR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Run `f` on a blocking-safe worker thread (`spawn_blocking`) and await it,
/// mapping a cancelled/panicked task to [`AppError::Internal`]. `what` names
/// the task in the error message.
///
/// Uses `tokio::task::spawn_blocking` directly (not
/// `tauri::async_runtime::spawn_blocking`, unlike the Tauri-command-side
/// twin in `commands/auth_commands.rs`): this handler runs on the axum
/// server's own tokio runtime, outside Tauri's IPC dispatch.
async fn run_blocking<T>(
    what: &str,
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(format!("{what} task failed: {e}")))?
}

/// `POST /api/auth/google/begin` — start the `OAuth2` loopback flow.
///
/// Returns `{ "url": "<consent-url>" }`. The caller opens the URL in a
/// browser; the exchange completes in the background. No network call is made
/// here — `begin_auth` only starts a local listener and builds the URL.
async fn google_auth_begin_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let url = state.calendar_sync.begin_auth(Utc::now(), Instant::now())?;
    Ok(Json(json!({ "url": url })))
}

/// `GET /api/auth/google/status` — return current auth state.
///
/// Serialises as a discriminated union, e.g.
/// `{"type":"connected","email":"user@example.com"}`.
/// No network call; safe to poll frequently.
async fn google_auth_status_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let status: AuthStatus = state.calendar_sync.auth_status(Instant::now());
    Ok(Json(status))
}

/// `POST /api/auth/google/disconnect` — local-wipe disconnect.
///
/// Clears the token file + `google_auth` + `chunk_sync_state` +
/// `external_events`. Remote data is never touched. Returns `204 No Content`.
async fn google_auth_disconnect_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    crate::services::sync::disconnect_provider(state.store.as_ref(), state.calendar_sync.as_ref())?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/calendar/pull` — refresh the event mirror, then full reschedule.
///
/// Delegates to [`crate::services::sync::pull_and_reschedule`]: pull is outside
/// the mutation guard (blocking network; must not hold a std mutex across I/O),
/// and the guard is held only around the CPU-bound reschedule.
///
/// The provider `list_events` calls use blocking reqwest which must not run
/// on a tokio worker — the entire sequence runs in `spawn_blocking`.
async fn calendar_pull_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let handles = active.sync_write_handles()?;
    run_blocking("pull", move || {
        crate::commands::auth_commands::run_pull_and_reschedule(handles)
    })
    .await
    .map(Json)
}

/// `POST /api/sync/now` — manual full sync: pull mirror, reschedule, push.
///
/// Delegates to [`crate::services::sync::sync_now`]. Returns a `SyncOutcome`:
/// the full reschedule's `ScheduleResult` under `schedule` plus the pushed-op
/// counts under `pushed`. Blocking provider I/O runs in `spawn_blocking`,
/// matching the pull handler.
async fn sync_now_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let handles = active.sync_write_handles()?;
    run_blocking("sync", move || {
        crate::commands::auth_commands::run_sync_now(handles)
    })
    .await
    .map(Json)
}

/// `GET /api/sync/status` — last-sync bookkeeping for the Settings UI.
///
/// Returns `{ "last_sync_at": <ISO 8601 | null>, "last_sync_error": <string | null> }`.
/// No network call; safe to poll.
async fn sync_status_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let status = crate::services::sync::get_sync_status(state.store.as_ref())?;
    Ok(Json(status))
}

/// `POST /api/backup/now` — manual export to the backup target (bypasses the
/// dirty/interval gates, keeps the stale-writer guard). Returns the fresh
/// `BackupStatus`. Delegates to
/// [`crate::commands::backup_commands::run_backup_now`]; blocking upload runs
/// in `spawn_blocking`.
async fn backup_now_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let handles = active.backup_write_handles()?;
    run_blocking("backup", move || {
        crate::commands::backup_commands::run_backup_now(handles)
    })
    .await
    .map(Json)
}

/// `GET /api/backup/status` — backup bookkeeping for the Settings card.
/// No network call; safe to poll.
async fn backup_status_handler(
    State(active): State<ActiveState>,
) -> Result<impl IntoResponse, AppError> {
    let state = active.get()?;
    let status = crate::services::backup::get_backup_status(
        state.store.as_ref(),
        state.backup.as_ref(),
        state.restore_notice.as_deref(),
    )?;
    Ok(Json(status))
}

/// Configuration resolved from environment variables.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// TCP port to bind on `127.0.0.1`.
    pub port: u16,
    /// Whether the REST API server should start.
    pub enabled: bool,
}

impl ServerConfig {
    /// Resolve configuration from environment variables.
    ///
    /// - `APRESWORK_API_PORT`    → port (default `19532`)
    /// - `APRESWORK_API_ENABLED` → `"false"` disables (default `true`)
    ///
    /// Invalid `APRESWORK_API_PORT` values fall back to the default port and
    /// log a warning.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_env_with(|key| std::env::var(key))
    }

    /// Resolve configuration using a caller-supplied lookup function.
    ///
    /// Accepts any function with the same signature as [`std::env::var`] so
    /// that tests can inject controlled values without mutating the process
    /// environment (which would be racy in a parallel test suite).
    fn from_env_with<F>(env_var: F) -> Self
    where
        F: Fn(&str) -> Result<String, std::env::VarError>,
    {
        let port = env_var("APRESWORK_API_PORT")
            .ok()
            .and_then(|v| {
                v.parse::<u16>()
                    .map_err(|_| {
                        log::warn!(
                            "APRESWORK_API_PORT value '{v}' is not a valid port number; \
                         falling back to default {DEFAULT_API_PORT}"
                        );
                    })
                    .ok()
            })
            .unwrap_or(DEFAULT_API_PORT);

        let enabled = env_var("APRESWORK_API_ENABLED")
            .map(|v| v.to_lowercase() != "false")
            .unwrap_or(true);

        Self { port, enabled }
    }
}

/// Start the Axum server on a background tokio task.
///
/// Takes the process-scoped [`ActiveState`] slot (or anything convertible) and
/// the profiles registry so that profile-management endpoints are available.
/// If `APRESWORK_API_ENABLED` is `"false"` the function returns immediately
/// without spawning anything.
///
/// # Errors
///
/// Returns [`AppError::Internal`] if the TCP listener cannot be bound.
pub async fn start_server(
    active: impl Into<ActiveState>,
    profiles: Arc<ProfilesState>,
    config: ServerConfig,
) -> Result<(), AppError> {
    if !config.enabled {
        log::info!("REST API server disabled (APRESWORK_API_ENABLED=false)");
        return Ok(());
    }

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to bind API server on {addr}: {e}")))?;

    log::info!("REST API server listening on http://{addr}");

    let router = build_router_with_profiles(active, profiles);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            log::error!("REST API server stopped with error: {e}");
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests;
