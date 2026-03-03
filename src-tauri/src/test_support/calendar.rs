// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Shared test fixture: `MockCalendarSync` for in-process mocking of the
//! `CalendarSync` trait across service and HTTP server test modules.

use std::collections::HashMap;
use std::sync::Mutex;

use std::time::Instant;

use chrono::{DateTime, Duration, Utc};

use crate::error::AppError;
use crate::traits::calendar_sync::{
    AuthStatus, CalendarSync, ExternalCalendar, ExternalEvent, RemoteChunkEvent, SyncOp,
    SyncOpResult, UserEventPayload,
};

/// A recorded user-owned-event write against [`MockCalendarSync`], captured in
/// call order for assertions in service tests.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UserEventOp {
    Create {
        calendar_id: String,
        payload: UserEventPayload,
    },
    Update {
        calendar_id: String,
        event_id: String,
        payload: UserEventPayload,
    },
    Delete {
        calendar_id: String,
        event_id: String,
    },
}

/// Configurable in-process `CalendarSync` double for unit and integration tests.
///
/// All methods that would touch a real provider are backed by in-process fields.
/// Use the builder-style `with_*` methods to configure desired behaviour.
pub(crate) struct MockCalendarSync {
    available: bool,
    events: HashMap<String, Vec<ExternalEvent>>,
    /// Per-calendar errors injected for `list_events`.
    errors: HashMap<String, AppError>,
    /// Recorded (`calendar_id`, start, end) calls to `list_events`.
    // The tuple is a compact call record; aliasing it would not aid clarity.
    #[allow(clippy::type_complexity)]
    calls: Mutex<Vec<(String, DateTime<Utc>, DateTime<Utc>)>>,
    /// When `Some`, `begin_auth()` returns `Ok(url)`; otherwise a `CalendarSync` error.
    begin_url: Option<String>,
    status: AuthStatus,
    /// When `true`, `disconnect()` returns an error.
    disconnect_error: bool,
    calendars: Vec<ExternalCalendar>,
    /// ID returned by `ensure_app_calendar` (default: `"mock-calendar-id"`).
    pub(crate) app_calendar_id: Option<String>,
    pub(crate) app_calendar_events: HashMap<String, Vec<RemoteChunkEvent>>,
    /// When `true`, `ensure_app_calendar()` returns an error.
    ensure_calendar_error: bool,
    /// When `Some`, `execute_sync_ops()` returns an error with this message.
    execute_ops_error: Option<String>,
    /// Monotonically incrementing counter for generated mock event IDs.
    next_event_counter: Mutex<u32>,
    /// All `SyncOp` values passed to `execute_sync_ops`, in order.
    pub(crate) recorded_sync_ops: Mutex<Vec<SyncOp>>,
    /// When `Some`, the user-event write methods return an error with this message.
    user_event_error: Option<String>,
    /// All user-owned-event writes, in call order.
    pub(crate) recorded_user_event_ops: Mutex<Vec<UserEventOp>>,
}

impl Default for MockCalendarSync {
    fn default() -> Self {
        Self::new(false, HashMap::new())
    }
}

impl MockCalendarSync {
    pub(crate) fn new(available: bool, events: HashMap<String, Vec<ExternalEvent>>) -> Self {
        Self {
            available,
            events,
            errors: HashMap::new(),
            calls: Mutex::new(vec![]),
            begin_url: None,
            status: AuthStatus::NotConnected,
            disconnect_error: false,
            calendars: vec![],
            app_calendar_id: None,
            app_calendar_events: HashMap::new(),
            ensure_calendar_error: false,
            execute_ops_error: None,
            next_event_counter: Mutex::new(0),
            recorded_sync_ops: Mutex::new(vec![]),
            user_event_error: None,
            recorded_user_event_ops: Mutex::new(vec![]),
        }
    }

    /// Builder: inject a `CalendarSync` error for a specific calendar.
    pub(crate) fn with_calendar_error(mut self, cal_id: &str, err: AppError) -> Self {
        self.errors.insert(cal_id.to_owned(), err);
        self
    }

    fn locked_clone<T: Clone>(mutex: &Mutex<T>) -> T {
        mutex.lock().expect("lock").clone()
    }

    pub(crate) fn recorded_calls(&self) -> Vec<(String, DateTime<Utc>, DateTime<Utc>)> {
        Self::locked_clone(&self.calls)
    }

    pub(crate) fn with_begin_url(mut self, url: &str) -> Self {
        self.begin_url = Some(url.to_owned());
        self
    }

    pub(crate) fn with_status(mut self, status: AuthStatus) -> Self {
        self.status = status;
        self
    }

    pub(crate) fn with_disconnect_error(mut self) -> Self {
        self.disconnect_error = true;
        self
    }

    pub(crate) fn with_app_calendar(mut self, id: &str, events: Vec<RemoteChunkEvent>) -> Self {
        let id_owned = id.to_owned();
        self.app_calendar_id = Some(id_owned.clone());
        self.app_calendar_events.insert(id_owned, events);
        self
    }

    pub(crate) fn with_ensure_calendar_error(mut self) -> Self {
        self.ensure_calendar_error = true;
        self
    }

    pub(crate) fn with_execute_ops_error(mut self, msg: &str) -> Self {
        self.execute_ops_error = Some(msg.to_owned());
        self
    }

    pub(crate) fn get_recorded_sync_ops(&self) -> Vec<SyncOp> {
        Self::locked_clone(&self.recorded_sync_ops)
    }

    pub(crate) fn with_user_event_error(mut self, msg: &str) -> Self {
        self.user_event_error = Some(msg.to_owned());
        self
    }

    pub(crate) fn get_recorded_user_event_ops(&self) -> Vec<UserEventOp> {
        Self::locked_clone(&self.recorded_user_event_ops)
    }

    /// Fail with the configured `user_event_error`, if any; otherwise record
    /// `op`. Shared by the three user-event write methods below — each still
    /// builds its own success return value.
    fn record_user_event_op(&self, op: UserEventOp) -> Result<(), AppError> {
        if let Some(msg) = &self.user_event_error {
            return Err(AppError::CalendarSync(msg.clone()));
        }
        self.recorded_user_event_ops.lock().expect("lock").push(op);
        Ok(())
    }
}

impl CalendarSync for MockCalendarSync {
    fn begin_auth(&self, _now: DateTime<Utc>, _now_instant: Instant) -> Result<String, AppError> {
        match &self.begin_url {
            Some(url) => Ok(url.clone()),
            None => Err(AppError::CalendarSync(
                "mock: begin_auth not configured".into(),
            )),
        }
    }

    fn auth_status(&self, _now_instant: Instant) -> AuthStatus {
        self.status.clone()
    }

    fn disconnect(&self) -> Result<(), AppError> {
        if self.disconnect_error {
            Err(AppError::CalendarSync("mock: disconnect failed".into()))
        } else {
            Ok(())
        }
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn list_calendars(&self, _now: DateTime<Utc>) -> Result<Vec<ExternalCalendar>, AppError> {
        Ok(self.calendars.clone())
    }

    fn list_events(
        &self,
        _now: DateTime<Utc>,
        calendar_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEvent>, AppError> {
        self.calls
            .lock()
            .expect("lock")
            .push((calendar_id.to_owned(), start, end));
        if let Some(err) = self.errors.get(calendar_id) {
            return Err(err.clone());
        }
        Ok(self.events.get(calendar_id).cloned().unwrap_or_default())
    }

    fn ensure_app_calendar(&self, _now: DateTime<Utc>) -> Result<String, AppError> {
        if self.ensure_calendar_error {
            return Err(AppError::CalendarSync(
                "mock: ensure_app_calendar failed".into(),
            ));
        }
        Ok(self
            .app_calendar_id
            .clone()
            .unwrap_or_else(|| "mock-calendar-id".to_owned()))
    }

    fn list_app_calendar_events(
        &self,
        _now: DateTime<Utc>,
        calendar_id: &str,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<RemoteChunkEvent>, AppError> {
        Ok(self
            .app_calendar_events
            .get(calendar_id)
            .cloned()
            .unwrap_or_default())
    }

    fn execute_sync_ops(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        ops: &[SyncOp],
    ) -> Result<Vec<SyncOpResult>, AppError> {
        if let Some(msg) = &self.execute_ops_error {
            return Err(AppError::CalendarSync(msg.clone()));
        }
        let mut recorded = self.recorded_sync_ops.lock().expect("lock");
        let mut counter = self.next_event_counter.lock().expect("lock");
        let mut results = Vec::with_capacity(ops.len());
        for op in ops {
            recorded.push(op.clone());
            let n = *counter;
            *counter += 1;
            let result = match op {
                SyncOp::Create(payload) => SyncOpResult::Created {
                    chunk_id: payload.chunk_id.clone(),
                    event_id: format!("mock-ev-{n}"),
                    etag: Some("mock-etag".into()),
                },
                SyncOp::Update { event_id, payload } => SyncOpResult::Updated {
                    chunk_id: payload.chunk_id.clone(),
                    event_id: event_id.clone(),
                    etag: Some("mock-etag-v2".into()),
                },
                SyncOp::Delete { .. } => SyncOpResult::Deleted,
            };
            results.push(result);
        }
        Ok(results)
    }

    fn create_user_event(
        &self,
        _now: DateTime<Utc>,
        calendar_id: &str,
        payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError> {
        self.record_user_event_op(UserEventOp::Create {
            calendar_id: calendar_id.to_owned(),
            payload: payload.clone(),
        })?;
        let mut counter = self.next_event_counter.lock().expect("lock");
        let n = *counter;
        *counter += 1;
        Ok(echo_user_event(
            calendar_id,
            &format!("mock-ev-{n}"),
            payload,
        ))
    }

    fn update_user_event(
        &self,
        _now: DateTime<Utc>,
        calendar_id: &str,
        event_id: &str,
        payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError> {
        self.record_user_event_op(UserEventOp::Update {
            calendar_id: calendar_id.to_owned(),
            event_id: event_id.to_owned(),
            payload: payload.clone(),
        })?;
        Ok(echo_user_event(calendar_id, event_id, payload))
    }

    fn delete_user_event(
        &self,
        _now: DateTime<Utc>,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<(), AppError> {
        self.record_user_event_op(UserEventOp::Delete {
            calendar_id: calendar_id.to_owned(),
            event_id: event_id.to_owned(),
        })
    }
}

/// Build the [`ExternalEvent`] a real provider would echo for a just-written
/// user event: payload fields verbatim, `busy` (fresh own event), not declined.
fn echo_user_event(calendar_id: &str, event_id: &str, payload: &UserEventPayload) -> ExternalEvent {
    ExternalEvent {
        calendar_id: calendar_id.to_owned(),
        event_id: event_id.to_owned(),
        title: payload.title.clone(),
        description: payload.description.clone(),
        start: payload.start,
        end: payload.end,
        busy: true,
        declined: false,
        all_day: payload.all_day,
    }
}

/// One-hour event starting at `start` (each test injects its own `now`, so
/// event times stay deterministic relative to the pull window).
pub(crate) fn make_event(
    calendar_id: &str,
    event_id: &str,
    title: &str,
    busy: bool,
    start: DateTime<Utc>,
) -> ExternalEvent {
    ExternalEvent {
        calendar_id: calendar_id.to_owned(),
        event_id: event_id.to_owned(),
        title: title.to_owned(),
        description: None,
        start,
        end: start + Duration::hours(1),
        busy,
        declined: false,
        all_day: false,
    }
}
