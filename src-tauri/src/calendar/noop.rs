// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Noop calendar-sync provider used when no integration is configured.

use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::traits::calendar_sync::{
    AuthStatus, CalendarSync, ExternalCalendar, ExternalEvent, RemoteChunkEvent, SyncOp,
    SyncOpResult, UserEventPayload,
};

const NO_CALENDAR_PROVIDER: &str = "no calendar provider is configured";

fn noop_err<T>() -> Result<T, AppError> {
    Err(AppError::CalendarSync(NO_CALENDAR_PROVIDER.into()))
}

//
// The `Noop` prefix identifies this as the offline/disabled impl among other
// calendar providers in `crate::calendar`.
#[allow(clippy::module_name_repetitions)]
pub struct NoopCalendarSync;

impl CalendarSync for NoopCalendarSync {
    fn begin_auth(&self, _now: DateTime<Utc>, _now_instant: Instant) -> Result<String, AppError> {
        noop_err()
    }

    fn auth_status(&self, _now_instant: Instant) -> AuthStatus {
        AuthStatus::NotConnected
    }

    fn disconnect(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn is_available(&self) -> bool {
        false
    }

    fn list_calendars(&self, _now: DateTime<Utc>) -> Result<Vec<ExternalCalendar>, AppError> {
        Ok(vec![])
    }

    fn list_events(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEvent>, AppError> {
        Ok(vec![])
    }

    fn ensure_app_calendar(&self, _now: DateTime<Utc>) -> Result<String, AppError> {
        noop_err()
    }

    fn list_app_calendar_events(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<RemoteChunkEvent>, AppError> {
        Ok(vec![])
    }

    fn execute_sync_ops(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        _ops: &[SyncOp],
    ) -> Result<Vec<SyncOpResult>, AppError> {
        noop_err()
    }

    fn create_user_event(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        _payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError> {
        noop_err()
    }

    fn update_user_event(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        _event_id: &str,
        _payload: &UserEventPayload,
    ) -> Result<ExternalEvent, AppError> {
        noop_err()
    }

    fn delete_user_event(
        &self,
        _now: DateTime<Utc>,
        _calendar_id: &str,
        _event_id: &str,
    ) -> Result<(), AppError> {
        noop_err()
    }
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;
    use crate::test_support::utc;
    use crate::traits::calendar_sync::{AuthStatus, CalendarSync};

    use super::NoopCalendarSync;

    #[test]
    fn begin_auth_returns_calendar_sync_error() {
        let noop = NoopCalendarSync;
        let err = noop
            .begin_auth(
                crate::test_support::test_now(),
                crate::test_support::test_instant_now(),
            )
            .unwrap_err();
        match err {
            AppError::CalendarSync(msg) => {
                assert!(
                    msg.contains("no calendar provider"),
                    "unexpected message: {msg}"
                );
            }
            other => panic!("expected CalendarSync error, got: {other:?}"),
        }
    }

    #[test]
    fn auth_status_is_not_connected() {
        let noop = NoopCalendarSync;
        assert_eq!(
            noop.auth_status(crate::test_support::test_instant_now()),
            AuthStatus::NotConnected
        );
    }

    #[test]
    fn disconnect_is_ok() {
        let noop = NoopCalendarSync;
        assert!(noop.disconnect().is_ok());
    }

    #[test]
    fn is_available_is_false() {
        let noop = NoopCalendarSync;
        assert!(!noop.is_available());
    }

    #[test]
    fn list_methods_return_empty() {
        let noop = NoopCalendarSync;
        let now = utc(2026, 7, 11, 0, 0);
        let calendars = noop.list_calendars(now).expect("list_calendars");
        assert!(calendars.is_empty(), "expected empty calendars");
        let events = noop
            .list_events(now, "any", utc(2026, 7, 11, 0, 0), utc(2026, 7, 12, 0, 0))
            .expect("list_events");
        assert!(events.is_empty(), "expected empty events");
    }

    #[test]
    fn ensure_app_calendar_returns_calendar_sync_error() {
        let noop = NoopCalendarSync;
        let now = utc(2026, 7, 11, 0, 0);
        assert!(
            matches!(
                noop.ensure_app_calendar(now),
                Err(AppError::CalendarSync(_))
            ),
            "ensure_app_calendar must return CalendarSync error"
        );
    }

    #[test]
    fn list_app_calendar_events_returns_empty_ok() {
        let noop = NoopCalendarSync;
        let now = utc(2026, 7, 11, 10, 0);
        let result = noop
            .list_app_calendar_events(now, "cal-id", now, now)
            .expect("list_app_calendar_events ok");
        assert!(
            result.is_empty(),
            "noop must return empty app calendar events"
        );
    }

    #[test]
    fn execute_sync_ops_returns_calendar_sync_error() {
        use crate::traits::calendar_sync::SyncOp;
        let noop = NoopCalendarSync;
        let now = utc(2026, 7, 11, 0, 0);
        assert!(
            matches!(
                noop.execute_sync_ops(now, "cal-id", &[]),
                Err(AppError::CalendarSync(_))
            ),
            "execute_sync_ops must return CalendarSync error"
        );
        // Also verify empty ops slice returns the same error (no short-circuit for empty).
        let dummy_delete = SyncOp::Delete {
            event_id: "ev1".to_owned(),
        };
        assert!(
            matches!(
                noop.execute_sync_ops(now, "cal-id", &[dummy_delete]),
                Err(AppError::CalendarSync(_))
            ),
            "execute_sync_ops with ops must return CalendarSync error"
        );
    }

    #[test]
    fn user_event_writes_return_calendar_sync_error() {
        use crate::traits::calendar_sync::UserEventPayload;
        let noop = NoopCalendarSync;
        let now = utc(2026, 7, 11, 0, 0);
        let payload = UserEventPayload {
            title: "Dentist".to_owned(),
            description: None,
            start: utc(2026, 7, 20, 9, 0),
            end: utc(2026, 7, 20, 10, 0),
            all_day: false,
        };
        assert!(
            matches!(
                noop.create_user_event(now, "cal-id", &payload),
                Err(AppError::CalendarSync(_))
            ),
            "create_user_event must return CalendarSync error"
        );
        assert!(
            matches!(
                noop.update_user_event(now, "cal-id", "evt-1", &payload),
                Err(AppError::CalendarSync(_))
            ),
            "update_user_event must return CalendarSync error"
        );
        assert!(
            matches!(
                noop.delete_user_event(now, "cal-id", "evt-1"),
                Err(AppError::CalendarSync(_))
            ),
            "delete_user_event must return CalendarSync error"
        );
    }
}
