// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

//! Reschedule trigger coordinator.
//!
//! [`RescheduleTrigger`] is a stateful coordinator that sits in the command
//! layer. After every mutation, command surfaces construct the [`Mutation`]
//! that happened and call [`RescheduleTrigger::trigger_mutation`];
//! [`policy_for`] — the ONE materialization of the DESIGN §5.2.2 trigger
//! table — maps it to a [`RescheduleMode`] and timing (immediate vs.
//! debounced). Pending debounced reschedules are coalesced and flushed once
//! their deadline has passed.
//!
//! The trigger is NOT a service (services are stateless). It holds shared
//! references to the store and an executor, and manages a debounce timer.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::traits::scheduling::ScheduleResult;
use crate::traits::storage::Store;

pub const BACKGROUND_CHECK_INTERVAL: Duration = Duration::from_secs(300);

/// How often the background timer polls for pending debounced reschedules that
/// are ready to flush.
const PENDING_FLUSH_CHECK_INTERVAL: Duration = Duration::from_millis(250);

/// How far in the past a chunk's `end_time` must be before it is considered
/// past-due (1 hour grace period).
const PAST_DUE_GRACE_HOURS: i64 = 1;

/// Which kind of reschedule to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RescheduleMode {
    /// Re-run the full scheduling pipeline for all tasks.
    Full,
    /// Re-run the pipeline for a specific subset of tasks.
    Incremental { task_ids: Vec<String> },
}

impl RescheduleMode {
    /// Coalesce two modes into one.
    ///
    /// - Full + anything  → Full
    /// - anything + Full  → Full
    /// - Incremental + Incremental → Incremental with merged, deduplicated IDs
    fn coalesce(self, other: Self) -> Self {
        match (self, other) {
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::Incremental { task_ids: mut a }, Self::Incremental { task_ids: b }) => {
                for id in b {
                    if !a.contains(&id) {
                        a.push(id);
                    }
                }
                Self::Incremental { task_ids: a }
            }
        }
    }
}

/// A completed domain mutation that may require a reschedule.
///
/// Command surfaces (Tauri commands, REST handlers) construct the variant
/// describing what happened and pass it to
/// [`RescheduleTrigger::trigger_mutation`] — they never pick a
/// [`RescheduleMode`] or immediacy themselves. [`policy_for`] holds that
/// mapping in one place so the two surfaces cannot drift.
#[derive(Debug, Clone)]
pub enum Mutation {
    /// A task was created.
    TaskCreated {
        /// Id of the created task.
        task_id: String,
    },
    /// A task was updated.
    TaskUpdated {
        /// Id of the updated task.
        task_id: String,
        /// The update transitions the task into `Backlog`, which frees its
        /// auto-scheduled slots (see `services::task::update_task`).
        to_backlog: bool,
    },
    /// A task was deleted.
    TaskDeleted,
    /// A task was cancelled.
    TaskCancelled,
    /// A task was completed via its scheduled chunks.
    TaskCompleted {
        /// Id of the completed task.
        task_id: String,
    },
    /// A fixed (manually placed) chunk was created.
    FixedChunkCreated,
    /// A chunk was marked completed.
    ChunkCompleted {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A previously completed chunk was reopened.
    ChunkReopened {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A chunk was moved to a new time slot.
    ChunkMoved {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A chunk was resized.
    ChunkResized {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A chunk was locked in place (marked fixed) without moving.
    ChunkLocked {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A scheduler-locked chunk was unlocked.
    ChunkUnlocked {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A fixed (manually placed) chunk was deleted.
    FixedChunkDeleted {
        /// Id of the chunk's task.
        task_id: String,
    },
    /// A recurring template was created.
    TemplateCreated,
    /// A recurring template was updated.
    TemplateUpdated,
    /// A recurring template was deleted.
    TemplateDeleted,
    /// A schedule was created.
    ScheduleCreated,
    /// A schedule was updated.
    ScheduleUpdated,
    /// A schedule was deleted.
    ScheduleDeleted,
    /// The application configuration was updated.
    ConfigUpdated,
}

/// Map a completed [`Mutation`] to its `(mode, immediate)` reschedule policy.
///
/// This is the single code definition of the DESIGN §5.2.2 trigger table
/// (architecture invariant: one definition per policy). Change the table
/// here and in DESIGN.md together.
#[must_use]
pub fn policy_for(mutation: Mutation) -> (RescheduleMode, bool) {
    fn incremental(task_id: String) -> RescheduleMode {
        RescheduleMode::Incremental {
            task_ids: vec![task_id],
        }
    }
    match mutation {
        // Debounced incremental: self-contained placements where a short
        // coalescing window absorbs rapid successive edits (e.g. dragging).
        Mutation::TaskCreated { task_id }
        | Mutation::TaskUpdated {
            task_id,
            to_backlog: false,
        }
        | Mutation::ChunkMoved { task_id }
        | Mutation::ChunkResized { task_id }
        | Mutation::ChunkLocked { task_id }
        | Mutation::ChunkUnlocked { task_id }
        | Mutation::FixedChunkDeleted { task_id } => (incremental(task_id), false),

        // Immediate incremental: completion state must be reflected in the
        // schedule before the command returns.
        Mutation::ChunkCompleted { task_id } | Mutation::ChunkReopened { task_id } => {
            (incremental(task_id), true)
        }

        // Debounced full: the mutation freed slots other tasks may claim,
        // but nothing is urgent about reclaiming them.
        Mutation::TaskUpdated {
            to_backlog: true, ..
        }
        | Mutation::TaskDeleted
        | Mutation::TaskCancelled => (RescheduleMode::Full, false),

        // Immediate full: completing a task frees its future auto-chunks for
        // waiting tasks; slot geometry or instance parameters changed —
        // anything may move, and views refetch right after these commands.
        Mutation::TaskCompleted { .. }
        | Mutation::FixedChunkCreated
        | Mutation::TemplateCreated
        | Mutation::TemplateUpdated
        | Mutation::TemplateDeleted
        | Mutation::ScheduleCreated
        | Mutation::ScheduleUpdated
        | Mutation::ScheduleDeleted
        | Mutation::ConfigUpdated => (RescheduleMode::Full, true),
    }
}

struct PendingReschedule {
    mode: RescheduleMode,
    /// Earliest moment at which the pending reschedule may be flushed.
    deadline: Instant,
}

/// Abstraction over the actual scheduling functions.
///
/// Exists primarily to allow injection of a mock in tests. The production
/// implementation ([`DefaultExecutor`]) delegates to the free functions in
/// [`crate::services::scheduling`].
pub trait RescheduleExecutor: Send + Sync {
    /// Run a full reschedule.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying scheduling service.
    fn execute_full(
        &self,
        store: &dyn Store,
        now: DateTime<Utc>,
    ) -> Result<ScheduleResult, AppError>;

    /// Run an incremental reschedule limited to the given task IDs.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying scheduling service.
    fn execute_incremental(
        &self,
        store: &dyn Store,
        task_ids: &[String],
        now: DateTime<Utc>,
    ) -> Result<ScheduleResult, AppError>;
}

/// Production executor that delegates to the scheduling service functions.
pub struct DefaultExecutor {
    scheduler: Arc<dyn crate::traits::scheduling::Scheduler>,
}

impl DefaultExecutor {
    /// Create a new executor wrapping the given scheduler.
    pub fn new(scheduler: Arc<dyn crate::traits::scheduling::Scheduler>) -> Self {
        Self { scheduler }
    }
}

impl RescheduleExecutor for DefaultExecutor {
    fn execute_full(
        &self,
        store: &dyn Store,
        now: DateTime<Utc>,
    ) -> Result<ScheduleResult, AppError> {
        crate::services::scheduling::reschedule(store, self.scheduler.as_ref(), now)
    }

    fn execute_incremental(
        &self,
        store: &dyn Store,
        task_ids: &[String],
        now: DateTime<Utc>,
    ) -> Result<ScheduleResult, AppError> {
        crate::services::scheduling::reschedule_incremental(
            store,
            self.scheduler.as_ref(),
            task_ids,
            now,
        )
    }
}

/// Stateful coordinator for reschedule triggers.
///
/// Commands call [`RescheduleTrigger::trigger`] after each mutation.
/// Immediate triggers execute synchronously; debounced ones are stored and
/// coalesced. [`RescheduleTrigger::flush_if_ready`] should be called at the
/// end of every command handler to drain any pending reschedule whose deadline
/// has passed.
pub struct RescheduleTrigger {
    store: Arc<dyn Store + Send + Sync>,
    executor: Arc<dyn RescheduleExecutor>,
    debounce_duration: Duration,
    pending: Mutex<Option<PendingReschedule>>,
    /// Serializes mutation+reschedule pipelines across entry points (Tauri
    /// commands, REST handlers, background timer). Lock order: this lock
    /// first, then the store's connection lock. Non-reentrant: a mutating
    /// command must drop its guard before calling [`Self::trigger`] /
    /// [`Self::flush_if_ready`], which re-acquire it around execution.
    mutation_lock: Mutex<()>,
}

impl RescheduleTrigger {
    /// Create a new trigger backed by the given store and executor.
    pub fn new(store: Arc<dyn Store + Send + Sync>, executor: Arc<dyn RescheduleExecutor>) -> Self {
        Self::with_debounce_duration(store, executor, Duration::ZERO)
    }

    /// Create a new trigger with an explicit debounce duration for delayed
    /// reschedules.
    pub fn with_debounce_duration(
        store: Arc<dyn Store + Send + Sync>,
        executor: Arc<dyn RescheduleExecutor>,
        debounce_duration: Duration,
    ) -> Self {
        Self {
            store,
            executor,
            debounce_duration,
            pending: Mutex::new(None),
            mutation_lock: Mutex::new(()),
        }
    }

    fn lock_pending(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<PendingReschedule>>, AppError> {
        self.pending
            .lock()
            .map_err(|_| AppError::Internal("reschedule trigger mutex poisoned".into()))
    }

    /// Acquire the pipeline-wide mutation lock.
    ///
    /// Mutating commands hold the returned guard around their service call
    /// only, and drop it before calling [`Self::trigger`] /
    /// [`Self::flush_if_ready`] — those re-acquire the lock internally around
    /// the actual reschedule execution, and the lock is non-reentrant.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Internal`] if the lock is poisoned.
    pub fn mutation_guard(&self) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        self.mutation_lock
            .lock()
            .map_err(|_| AppError::Internal("mutation pipeline mutex poisoned".into()))
    }

    /// Queue or execute the reschedule mandated by [`policy_for`] for a
    /// completed mutation, then flush any pending entry whose deadline has
    /// passed.
    ///
    /// This is the single entry point command surfaces use after a mutation —
    /// they never pick a [`RescheduleMode`] or immediacy themselves.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying executor.
    pub fn trigger_mutation(&self, mutation: Mutation) -> Result<(), AppError> {
        let (mode, immediate) = policy_for(mutation);
        self.trigger(mode, immediate)?;
        self.flush_if_ready()?;
        Ok(())
    }

    /// Run `op` under the mutation guard, then report `mutation(&result)` via
    /// [`Self::trigger_mutation`]. Shared by mutating command surfaces (Tauri
    /// commands, REST handlers) that perform one guarded service call and
    /// report a single resulting mutation.
    ///
    /// # Errors
    ///
    /// Propagates any error from the guard, `op`, or `trigger_mutation`.
    pub fn run_guarded<T>(
        &self,
        op: impl FnOnce() -> Result<T, AppError>,
        mutation: impl FnOnce(&T) -> Mutation,
    ) -> Result<T, AppError> {
        let result = {
            let _guard = self.mutation_guard()?;
            op()?
        };
        self.trigger_mutation(mutation(&result))?;
        Ok(result)
    }

    /// Queue or execute a reschedule.
    ///
    /// When `immediate` is `true`, any pending debounced reschedule is first
    /// coalesced with `mode`, and the result is executed right away. When
    /// `immediate` is `false`, the mode is merged with any existing pending
    /// entry and the deadline is (re-)set to now + `self.debounce_duration`.
    ///
    /// Returns the [`ScheduleResult`] when a reschedule was executed, or
    /// `None` when only a pending entry was stored.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying executor.
    pub fn trigger(
        &self,
        mode: RescheduleMode,
        immediate: bool,
    ) -> Result<Option<ScheduleResult>, AppError> {
        if immediate {
            let final_mode = {
                let mut guard = self.lock_pending()?;
                let existing = guard.take();
                match existing {
                    None => mode,
                    Some(p) => p.mode.coalesce(mode),
                }
            };
            let result = self.execute(final_mode)?;
            Ok(Some(result))
        } else {
            if self.debounce_duration.is_zero() {
                let result = self.execute(mode)?;
                return Ok(Some(result));
            }

            let mut guard = self.lock_pending()?;
            let new_pending = match guard.take() {
                None => PendingReschedule {
                    mode,
                    deadline: Instant::now() + self.debounce_duration,
                },
                Some(existing) => PendingReschedule {
                    mode: existing.mode.coalesce(mode),
                    deadline: Instant::now() + self.debounce_duration,
                },
            };
            *guard = Some(new_pending);
            Ok(None)
        }
    }

    /// Check whether a pending debounced reschedule's deadline has passed and,
    /// if so, execute it.
    ///
    /// Returns the [`ScheduleResult`] when a reschedule was executed, or
    /// `None` when there was nothing pending or the deadline has not yet passed.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying executor.
    pub fn flush_if_ready(&self) -> Result<Option<ScheduleResult>, AppError> {
        let mode = {
            let mut guard = self.lock_pending()?;
            match guard.as_ref() {
                None => return Ok(None),
                Some(p) if Instant::now() < p.deadline => return Ok(None),
                Some(_) => {}
            }
            // Deadline has passed — take the entry. The `None` branch is
            // unreachable: we just matched `Some(_)` above inside the same
            // lock guard with no intervening mutation.
            let Some(entry) = guard.take() else {
                return Ok(None);
            };
            entry.mode
        };
        let result = self.execute(mode)?;
        Ok(Some(result))
    }

    /// Execute a reschedule mode immediately.
    ///
    /// Holds the pipeline mutation lock for the duration of the reschedule so
    /// no mutation can interleave with the pipeline's read→compute→apply
    /// sequence. Callers must not already hold the guard (non-reentrant).
    fn execute(&self, mode: RescheduleMode) -> Result<ScheduleResult, AppError> {
        let _guard = self.mutation_guard()?;
        let now = Utc::now();
        match mode {
            RescheduleMode::Full => self.executor.execute_full(self.store.as_ref(), now),
            RescheduleMode::Incremental { task_ids } => {
                self.executor
                    .execute_incremental(self.store.as_ref(), &task_ids, now)
            }
        }
    }

    /// Check for past-due scheduled chunks and trigger a full reschedule if any
    /// exist.
    ///
    /// A chunk is "past due" when its `end_time` is more than
    /// [`PAST_DUE_GRACE_HOURS`] before `now` and it still has
    /// `status = Scheduled`.
    ///
    /// Returns `true` when a reschedule was triggered, `false` when the store
    /// contained no past-due chunks.
    ///
    /// # Errors
    ///
    /// Propagates any storage or scheduling error.
    pub fn check_past_due(&self, now: DateTime<Utc>) -> Result<bool, AppError> {
        let cutoff = now
            .checked_sub_signed(chrono::Duration::hours(PAST_DUE_GRACE_HOURS))
            .ok_or_else(|| {
                AppError::Internal("timestamp underflow computing past-due cutoff".into())
            })?;
        let past_due = self.store.get_past_due_scheduled_chunks(cutoff)?;
        if past_due.is_empty() {
            return Ok(false);
        }
        self.trigger(RescheduleMode::Full, true)?;
        Ok(true)
    }
}

/// Returns `true` if the wall-clock interval `[prev, now]` crossed midnight in
/// UTC.
///
/// Using UTC is a reasonable approximation for a desktop app and avoids a
/// dependency on a runtime timezone database.
#[must_use]
pub fn crossed_midnight_utc(prev: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    prev.date_naive() != now.date_naive()
}

fn flush_pending_reschedule(trigger: &RescheduleTrigger) {
    match trigger.flush_if_ready() {
        Ok(Some(_)) => {
            log::info!("background timer: flushed pending debounced reschedule");
        }
        Ok(None) => {}
        Err(e) => log::warn!("background timer: debounced reschedule flush failed: {e}"),
    }
}

fn run_background_maintenance(
    trigger: &RescheduleTrigger,
    last_check: &mut DateTime<Utc>,
    last_maintenance_tick: &mut Instant,
    now: DateTime<Utc>,
) {
    flush_pending_reschedule(trigger);

    if last_maintenance_tick.elapsed() < BACKGROUND_CHECK_INTERVAL {
        return;
    }

    let past_due_triggered = match trigger.check_past_due(now) {
        Ok(triggered) => {
            if triggered {
                log::info!("background timer: triggered full reschedule due to past-due chunks");
            }
            triggered
        }
        Err(e) => {
            log::warn!("background timer: past-due check failed: {e}");
            false
        }
    };

    // Check for midnight crossing (skip if past-due already triggered a
    // full reschedule this tick — it would be redundant).
    if !past_due_triggered && crossed_midnight_utc(*last_check, now) {
        match trigger.trigger(RescheduleMode::Full, true) {
            Ok(_) => {
                log::info!("background timer: triggered full reschedule after midnight");
            }
            Err(e) => log::warn!("background timer: midnight reschedule failed: {e}"),
        }
    }

    *last_check = now;
    *last_maintenance_tick = Instant::now();
}

/// One timer tick: resolve the currently active trigger and run maintenance
/// against it. `None` (no profile active, or mid-switch) skips the tick.
fn timer_tick<F>(
    resolve: &F,
    last_check: &mut DateTime<Utc>,
    last_maintenance_tick: &mut Instant,
    now: DateTime<Utc>,
) where
    F: Fn() -> Option<Arc<RescheduleTrigger>>,
{
    if let Some(trigger) = resolve() {
        run_background_maintenance(trigger.as_ref(), last_check, last_maintenance_tick, now);
    }
}

/// Spawn a background thread that periodically checks for past-due chunks and
/// midnight crossings.
///
/// The thread is process-scoped: each tick it calls `resolve` for the
/// currently active profile's trigger, so an in-process profile switch simply
/// makes subsequent ticks operate on the new profile (no thread restart).
/// `None` — no profile unlocked yet, or a switch in flight — skips the tick.
///
/// Every [`BACKGROUND_CHECK_INTERVAL`] the thread:
/// 1. Calls [`RescheduleTrigger::check_past_due`]; logs warnings on error.
/// 2. Checks if midnight crossed since the last iteration; if so, triggers a
///    full reschedule immediately.
///
/// Errors are logged with [`log::warn!`] and never panic the thread. The thread
/// is a daemon thread — it is killed automatically when the main process exits.
pub fn start_background_timer<F>(resolve: F)
where
    F: Fn() -> Option<Arc<RescheduleTrigger>> + Send + 'static,
{
    std::thread::spawn(move || {
        let mut last_check = Utc::now();
        let mut last_maintenance_tick = Instant::now();
        loop {
            std::thread::sleep(PENDING_FLUSH_CHECK_INTERVAL);
            timer_tick(
                &resolve,
                &mut last_check,
                &mut last_maintenance_tick,
                Utc::now(),
            );
        }
    });
}

#[cfg(test)]
mod tests;
