// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use crate::error::AppError;
use crate::profiles::ActiveProfile;
use crate::services::trigger::RescheduleTrigger;
use crate::traits::backup::BackupTarget;
use crate::traits::calendar_sync::CalendarSync;
use crate::traits::scheduling::Scheduler;
use crate::traits::storage::Store;

/// Shared application state managed by Tauri.
///
/// Contains the storage backend, the scheduling engine, the reschedule
/// trigger coordinator, the calendar-sync provider, the backup target, and
/// the identity of the profile this state serves. Managed only after the
/// profile gate clears (see `profiles::activate`); `Clone` shares the same
/// `Arc`s so the Tauri runtime and the REST server operate on one state.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn Store + Send + Sync>,
    pub scheduler: Arc<dyn Scheduler>,
    pub trigger: Arc<RescheduleTrigger>,
    /// Calendar-sync provider. `CalendarSync` is `Send + Sync`-bound, so no
    /// extra bounds are needed at the field level (unlike `store`).
    pub calendar_sync: Arc<dyn CalendarSync>,
    /// Backup target paired with `calendar_sync` by `providers_from_config`.
    pub backup: Arc<dyn BackupTarget>,
    /// The active profile's data directory (zip staging, DB swap paths).
    pub profile_dir: PathBuf,
    /// Set when the startup restore replaced the local DB from a backup:
    /// the backup's `last_mutation` as RFC 3339, or `""` when unknown. The
    /// frontend surfaces it once as a toast via `get_backup_status`.
    pub restore_notice: Option<String>,
    pub profile: ActiveProfile,
}

/// Process-scoped handle to the currently active profile's [`AppState`].
///
/// Managed by Tauri once at startup — before any profile is unlocked — so
/// command handlers, the REST server, and the background timers can resolve
/// the active state per call instead of holding a fixed reference. That
/// per-call resolution is what makes in-process profile switching possible:
/// `swap` replaces the slot and every subsequent resolution sees the new
/// profile. `Clone` shares the same slot.
#[derive(Clone)]
pub struct ActiveState {
    inner: Arc<RwLock<Option<Arc<AppState>>>>,
    /// Serializes activate/switch sequences (check → flush old → swap new)
    /// so two concurrent activations cannot interleave.
    activation: Arc<Mutex<()>>,
}

/// The four service handles a `spawn_blocking` sync-mutation call site (pull,
/// full sync, user-event create/update/delete) needs from the active
/// profile's state.
pub type SyncWriteHandles = (
    Arc<dyn Store + Send + Sync>,
    Arc<dyn CalendarSync>,
    Arc<dyn Scheduler>,
    Arc<RescheduleTrigger>,
);

/// The four handles a `spawn_blocking` manual-backup call site needs from the
/// active profile's state: store, target, the profile's data directory (zip
/// staging), and the restore notice to fold into the refreshed status.
pub type BackupWriteHandles = (
    Arc<dyn Store + Send + Sync>,
    Arc<dyn BackupTarget>,
    PathBuf,
    Option<String>,
);

impl ActiveState {
    /// An empty slot: no profile active yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
            activation: Arc::new(Mutex::new(())),
        }
    }

    /// The active state, or `None` while the profile gate is showing.
    #[must_use]
    pub fn get_opt(&self) -> Option<Arc<AppState>> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The active state, or a validation error for callers that require one
    /// (command handlers, REST endpoints).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Validation`] when no profile is active.
    pub fn get(&self) -> Result<Arc<AppState>, AppError> {
        self.get_opt()
            .ok_or_else(|| AppError::Validation("No profile is active.".to_owned()))
    }

    /// Clone [`SyncWriteHandles`] out of the active state. Shared by
    /// `spawn_blocking` sync-mutation call sites (Tauri commands and the REST
    /// server) that need owned handles to move into a worker-thread closure.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Validation`] when no profile is active.
    pub fn sync_write_handles(&self) -> Result<SyncWriteHandles, AppError> {
        let state = self.get()?;
        Ok((
            state.store.clone(),
            state.calendar_sync.clone(),
            state.scheduler.clone(),
            state.trigger.clone(),
        ))
    }

    /// Clone [`BackupWriteHandles`] out of the active state. Shared by the
    /// `backup_now` Tauri command and the REST `POST /api/backup/now`
    /// handler, each of which moves owned handles into a `spawn_blocking`
    /// worker-thread closure.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Validation`] when no profile is active.
    pub fn backup_write_handles(&self) -> Result<BackupWriteHandles, AppError> {
        let state = self.get()?;
        Ok((
            state.store.clone(),
            state.backup.clone(),
            state.profile_dir.clone(),
            state.restore_notice.clone(),
        ))
    }

    /// Replaces the slot, returning the previous occupant (for flushing).
    pub fn swap(&self, next: Option<Arc<AppState>>) -> Option<Arc<AppState>> {
        let mut slot = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        std::mem::replace(&mut *slot, next)
    }

    /// Guard held across an activate/switch sequence. The slot itself stays
    /// readable while held — only competing activations are excluded.
    pub fn activation_guard(&self) -> MutexGuard<'_, ()> {
        self.activation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for ActiveState {
    fn default() -> Self {
        Self::new()
    }
}

/// A pre-filled slot. Lets tests and the REST router builder accept a plain
/// `Arc<AppState>` where production passes the shared swappable handle.
impl From<Arc<AppState>> for ActiveState {
    fn from(state: Arc<AppState>) -> Self {
        let active = Self::new();
        active.swap(Some(state));
        active
    }
}

impl fmt::Debug for ActiveState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let occupied = self.get_opt().is_some();
        f.debug_struct("ActiveState")
            .field("occupied", &occupied)
            .finish()
    }
}

impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState")
            .field("store", &"<dyn Store>")
            .field("scheduler", &"<dyn Scheduler>")
            .field("trigger", &"<RescheduleTrigger>")
            .field("calendar_sync", &"<dyn CalendarSync>")
            .field("backup", &"<dyn BackupTarget>")
            .field("profile_dir", &self.profile_dir)
            .field("restore_notice", &self.restore_notice)
            .field("profile", &self.profile)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::backup::noop::NoopBackupTarget;
    use crate::calendar::noop::NoopCalendarSync;
    use crate::db::sqlite::SqliteStore;
    use crate::test_support::make_scheduler_stack;

    use super::{ActiveState, AppState};

    const TEST_PROFILE_DIR: &str = "/tmp/p-test";

    fn test_state() -> AppState {
        let store = Arc::new(SqliteStore::new(":memory:").expect("in-memory store"));
        let (scheduler, trigger) = make_scheduler_stack(store.clone());
        AppState {
            store,
            scheduler,
            trigger,
            calendar_sync: Arc::new(NoopCalendarSync),
            backup: Arc::new(NoopBackupTarget),
            profile_dir: std::path::PathBuf::from(TEST_PROFILE_DIR),
            restore_notice: None,
            profile: crate::profiles::ActiveProfile {
                id: "p-test".to_owned(),
                name: "Test".to_owned(),
            },
        }
    }

    #[test]
    fn debug_includes_all_field_names() {
        let state = test_state();
        let debug = format!("{state:?}");
        for field in [
            "store",
            "scheduler",
            "trigger",
            "calendar_sync",
            "backup",
            "profile_dir",
            "restore_notice",
        ] {
            assert!(debug.contains(field), "Debug should include {field}");
        }
        assert!(
            debug.contains("p-test"),
            "Debug should include the profile identity"
        );
    }

    #[test]
    fn clone_shares_the_same_store() {
        let state = test_state();
        let cloned = state.clone();
        // addr_eq: wide-pointer comparison of trait-object Arcs is lint-prone;
        // the data address alone proves the store is shared.
        assert!(
            std::ptr::addr_eq(Arc::as_ptr(&state.store), Arc::as_ptr(&cloned.store)),
            "clone must share the store, not duplicate it"
        );
        assert_eq!(cloned.profile, state.profile);
    }

    #[test]
    fn active_state_starts_empty_and_get_is_a_validation_error() {
        let active = ActiveState::new();
        assert!(active.get_opt().is_none());
        let err = active.get().expect_err("empty slot should error");
        assert!(matches!(err, crate::error::AppError::Validation(_)));
        assert!(
            err.to_string().contains("No profile is active"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn active_state_swap_installs_and_returns_previous() {
        let active = ActiveState::new();
        let first = Arc::new(test_state());
        assert!(active.swap(Some(first.clone())).is_none());
        let resolved = active.get().expect("slot should be occupied");
        assert_eq!(resolved.profile, first.profile);

        let second = Arc::new(test_state());
        let previous = active.swap(Some(second)).expect("previous occupant");
        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&previous.store),
            Arc::as_ptr(&first.store)
        ));

        let taken = active.swap(None).expect("occupant taken out");
        assert!(active.get_opt().is_none());
        drop(taken);
    }

    #[test]
    fn active_state_clone_shares_the_slot() {
        let active = ActiveState::default();
        let cloned = active.clone();
        active.swap(Some(Arc::new(test_state())));
        assert!(
            cloned.get_opt().is_some(),
            "clone must observe swaps on the original"
        );
    }

    #[test]
    fn active_state_from_arc_is_prefilled() {
        let state = Arc::new(test_state());
        let active = ActiveState::from(state.clone());
        let resolved = active.get().expect("prefilled slot");
        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&resolved.store),
            Arc::as_ptr(&state.store)
        ));
    }

    #[test]
    fn active_state_debug_reports_occupancy() {
        let active = ActiveState::new();
        assert!(format!("{active:?}").contains("occupied: false"));
        active.swap(Some(Arc::new(test_state())));
        assert!(format!("{active:?}").contains("occupied: true"));
    }

    #[test]
    fn activation_guard_excludes_competing_activations() {
        let active = ActiveState::new();
        let guard = active.activation_guard();
        assert!(active.get_opt().is_none());
        drop(guard);
        // Re-acquiring after drop must not deadlock.
        drop(active.activation_guard());
    }
}
