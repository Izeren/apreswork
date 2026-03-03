# Apreswork — Implementation Design

## 1. Architecture Overview

Apreswork is a local-first desktop app built as a **monolith with modular internals**. All code ships in a single binary, but internally follows clean separation via Rust module boundaries, traits, and a layered architecture.

### Layers

```
┌─────────────────────────────────────────────────┐
│  Frontend (Svelte 5 + TypeScript)               │
│  ├── Calendar View    ├── Task List View        │
│  └── Settings         └── Status View           │
├─────────────────────────────────────────────────┤
│  Commands Layer (Tauri IPC + REST API)           │
│  Thin wrappers: deserialize → call service →    │
│  serialize response                             │
├─────────────────────────────────────────────────┤
│  Service Layer (Business Logic)                 │
│  services::{task, scheduling, recurring,        │
│  schedule, sync, comment, trigger, backup}      │
├─────────────────────────────────────────────────┤
│  Traits (Contracts)                             │
│  dyn Store, dyn Scheduler, dyn CalendarSync,    │
│  dyn BackupTarget                               │
├─────────────────────────────────────────────────┤
│  Implementations                                │
│  SqliteStore, DefaultScheduler,                 │
│  GoogleCalendarSync / NoopCalendarSync,         │
│  GoogleDriveBackup / NoopBackupTarget           │
└─────────────────────────────────────────────────┘
```

### SOLID Principles Applied

- **Single Responsibility**: Commands handle serialization. Services contain business logic. Traits define contracts. Implementations handle infrastructure.
- **Open/Closed**: New scheduling strategies, storage backends, or calendar integrations are added by implementing a trait — no modification of existing code.
- **Liskov Substitution**: Any `dyn Store` implementation is interchangeable (`SqliteStore` on disk, the same `SqliteStore` on an in-memory database in tests via `memory_db()`, future backends).
- **Interface Segregation**: Storage is split into 10 sub-traits: `TaskStore`, `ChunkStore`, `ScheduleStore`, `RecurringTemplateStore`, `LabelStore`, `ConfigStore`, `CommentStore`, `ExternalEventStore`, `GoogleAuthStore`, `ChunkSyncStateStore`.
- **Dependency Inversion**: Services depend on trait abstractions, never on concrete types. Concrete implementations are assembled only at the composition root (`lib.rs`).

---

## 2. Rust Backend — Module Structure

Single crate at `src-tauri/`. No workspace or sub-crates.

```
src-tauri/src/
  main.rs                      # Entry point, calls app_lib::run()
  lib.rs                       # Composition root: Tauri builder, command registration, plugin setup

  domain/
    mod.rs
    models.rs                  # Task, Chunk, RecurringTemplate, Schedule, ScheduleWindow, AppConfig, Comment, ExternalEventRecord, GoogleAuthState, ChunkSyncState
    enums.rs                   # Priority, TaskStatus, ChunkStatus
    cadence.rs                 # Cadence { period, interval, windows }, Period, Window, Occurrence; occurrence generator
    inputs.rs                  # CreateTaskInput, UpdateTaskInput, CreateTemplateInput, etc., AgendaItem, LabelCount, TaskFilter
    date_utils.rs              # Recurrence date math: start_of_week, start_of_month, end_of_day, start_of_day
    validation.rs              # Input validation rules, schedule-capacity predicates

  traits/
    mod.rs
    storage.rs                 # TaskStore, ChunkStore, ScheduleStore, RecurringTemplateStore, LabelStore, ConfigStore, CommentStore, ExternalEventStore, GoogleAuthStore, ChunkSyncStateStore, Store
    scheduling.rs              # Scheduler trait, ScheduleInput, ScheduleResult, WarningKind, scheduling_order (shared comparator)
    calendar_sync.rs           # CalendarSync trait (item-level #[allow] — "Sync" conflicts with std); AuthStatus, ExternalCalendar, ExternalEvent, SyncOp, SyncOpResult, RemoteChunkEvent, ChunkEventPayload, UserEventPayload
    backup.rs                  # BackupTarget trait + RemoteBackupMeta (freshness probe/upload/download)

  db/
    mod.rs
    migrations.rs              # Version-tracked migration runner
    migration_001.rs           # Initial release schema
    sqlite/                    # SQLite implementation of all Store traits
      mod.rs                   # SqliteStore (Mutex<Connection>), TxStore, Store::with_tx, shared helpers
      task.rs                  # TaskStore impl (SqliteStore + TxStore) + task mappers
      chunk.rs                 # ChunkStore impl + chunk mappers
      schedule.rs              # ScheduleStore impl + window mappers
      template.rs              # RecurringTemplateStore impl + cadence (de)serialization
      config_busy.rs           # ConfigStore impl
      sync_state.rs            # ExternalEventStore + GoogleAuthStore + ChunkSyncStateStore impls
      comment.rs               # CommentStore impl + comment mapper
      tests/                   # Store test suite (shared fixtures in tests/mod.rs)

  scheduler/
    mod.rs
    engine.rs                  # DefaultScheduler (priority-based greedy)
    engine/tests/              # Engine test suite: placement, breaks (shared fixtures in tests/mod.rs)
    slot_finder.rs             # Expand schedule windows into concrete time slots (DST-aware); minute-grid slot alignment

  calendar/
    mod.rs                     # providers_from_config: one policy selecting the calendar-sync + backup pair
    google.rs                  # GoogleCalendarSync impl — loopback PKCE flow, token refresh
    google_http.rs             # Google Calendar REST list/CRUD calls (list_events, user-event write-through), 401 refresh-once, 403/429 backoff (BackoffPolicy)
    google_http/batch.rs       # Multipart batch push (batch_sync_ops, ≤250 ops/req = BATCH_MAX_OPS), per-part backoff
    google_token.rs            # KeyringStore (OS keyring via keyring crate); StoredToken (OAuth exchange result, in-memory only; TokenFile test-only legacy)
    noop.rs                    # NoopCalendarSync impl (item-level #[allow] — follows trait naming)

  backup/
    mod.rs
    google_drive.rs            # GoogleDriveBackup impl — Drive files.list/create/update/get, multipart upload
    noop.rs                    # NoopBackupTarget impl (no provider configured)

  profiles/
    mod.rs                     # ProfilesState (registry + data dir), ActiveProfile
    registry.rs                # profiles.json load/save (atomic), ProfileEntry, path helpers
    adoption.rs                # First-run adoption of the legacy single-profile data dir
    service.rs                 # Testable command cores: create, rename, delete, last-used
    activate.rs                # Post-unlock composition: build AppState, timers, REST server; in-process profile switch (flush old → activate new → swap slot)

  services/
    mod.rs
    task/                      # Task/chunk business logic
      mod.rs                   #   Re-exports: CRUD, lifecycle, chunk ops, agenda, labels
      crud.rs                  #   create_task, get_task, list_tasks, update_task, delete_task
      lifecycle.rs             #   complete_task, complete_chunk, reopen_chunk, cancel_task
      chunks.rs                #   create_fixed_chunk, delete_fixed_chunk, move_chunk, resize_chunk, lock_chunk, unlock_chunk
      agenda.rs                #   get_agenda, list_labels
    scheduling.rs              # Orchestrates scheduler + storage + recurring generation: reschedule, reschedule_incremental, diff_chunks
    scheduling/tests/          # Scheduling test suite: reschedule, diff, incremental, integration, stale_locks, warnings
    recurring/                 # Template → instance lifecycle
      mod.rs                   #   create_template, get_template, update_template, delete_template, get_orphaned_template_instances
      reconcile/               #   reconcile (instance generation + diff), auto_cancel_overdue
    schedule.rs                # Schedule CRUD with deletion guard + task reassignment
    sync.rs                    # disconnect_provider, pull_external_events, pull_and_reschedule, sync_cycle (push), sync_now, get_sync_status, user-event CRUD (primary-cal write-through)
    comment.rs                 # Comment CRUD + system comment generation
    trigger.rs                 # RescheduleTrigger: Mutation → mode/timing policy, debounced execution, pipeline lock, background maintenance timer
    backup/                    # Backup orchestration: gated exports, backup-wins restore, staged import
      mod.rs                   #   status/toggle, export_now + triggers, restore_check, stage/apply import
      archive.rs               #   zip build (VACUUM INTO snapshot, token excluded), verify, DB swap-in

  commands/
    mod.rs
    task_commands.rs           # Tauri commands: CRUD tasks + complete_task
    chunk_commands.rs          # Tauri commands: complete, reopen, move, resize, lock, unlock, delete_fixed_chunk
    schedule_commands.rs       # Tauri commands: schedule CRUD
    scheduler_commands.rs      # Tauri commands: trigger_reschedule, trigger_reschedule_incremental
    recurring_commands.rs      # Tauri commands: template CRUD
    comment_commands.rs        # Tauri commands: comment CRUD, list for task
    config_commands.rs         # Tauri commands: get/update config
    auth_commands.rs           # Tauri commands: google auth, calendar picker, manual pull, sync-now/status, user-event CRUD
    profile_commands.rs        # Tauri commands: profile status/unlock/create/rename/delete/switch
    backup_commands.rs         # Tauri commands: backup status/toggle/now, file export/import

  api/
    mod.rs
    http_server/               # Axum localhost REST API (router + handler functions)
      mod.rs                   #   build_router_with_profiles, start_server, host-header middleware, profile-guard middleware

  error.rs                     # AppError enum with Tauri serialization
  state.rs                     # AppState struct (8 fields) + ActiveState (swappable active-profile slot; commands/REST resolve it per call)
```

---

## 3. Domain Models

### 3.1 Core Types (`domain/models.rs`)

All entity IDs are `String` (UUID v7 for time-ordering). All timestamps are `chrono::DateTime<Utc>`.

```rust
struct Task {
    id: EntityId,
    title: String,
    description: Option<String>,
    duration_minutes: i64,
    time_logged_minutes: i64,
    priority: Priority,
    status: TaskStatus,
    start_date: Option<DateTime<Utc>>,
    deadline: Option<DateTime<Utc>>,       // Always required for persisted tasks (enforced by validation). Option only because the Rust type is reused for virtual orphaned-template instances (M9.5), which also compute a deadline but are never persisted.
    schedule_id: EntityId,
    min_chunk_minutes: i64,                // Default 30, min 5
    no_split: bool,
    recurring_template_id: Option<EntityId>,
    expire_at: Option<DateTime<Utc>>,      // Period-aware auto-cancel deadline for recurring instances; NULL = never auto-expires
    is_pinned: bool,                       // User has manually placed (pinned) at least one fixed chunk. Pinned recurring instances are sticky — reconcile never repositions or deletes them, and auto-cancellation skips them.
    labels: Vec<String>,                   // Denormalized from join table on read
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct Chunk {
    id: EntityId,
    task_id: EntityId,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    status: ChunkStatus,                   // Scheduled | Completed
    is_fixed: bool,
    logged_minutes: Option<i64>,           // Actual time logged on completion (override or scheduled duration). Used by reopen_chunk to subtract the correct amount.
    completed_at: Option<DateTime<Utc>>,
    google_event_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct RecurringTemplate {
    id: EntityId,
    title: String,
    description: Option<String>,
    duration_minutes: i64,
    priority: Priority,
    schedule_id: EntityId,
    cadence: Cadence,
    labels: Vec<String>,
    is_active: bool,
    start_date: DateTime<Utc>,             // Reconcile anchor; defaults to created_at on old data
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct Schedule {
    id: EntityId,
    name: String,
    is_default: bool,
    windows: Vec<ScheduleWindow>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct ScheduleWindow {
    id: EntityId,
    schedule_id: EntityId,
    day_of_week: chrono::Weekday,
    start_time: NaiveTime,
    end_time: NaiveTime,
}

struct AppConfig {
    planning_horizon_days: i64,            // Default 30
    timezone: String,                      // IANA timezone string
    max_continuous_minutes: i64,           // Default 120. Max back-to-back scheduled time before a break is required.
    min_break_minutes: i64,                // Default 5. Minimum break duration inserted between continuous blocks.
    last_reschedule: Option<DateTime<Utc>>,
    last_mutation: Option<DateTime<Utc>>,  // Tracks when chunks were last changed (for sync debounce)
    last_sync: Option<DateTime<Utc>>,      // Tracks when GCal chunk sync last ran
    last_busy_sync: Option<DateTime<Utc>>, // Tracks when GCal busy times were last cached
}
```

```rust
struct Comment {
    id: EntityId,
    task_id: EntityId,
    author: String,                        // "SYSTEM" for auto-generated, "User" for human (until auth)
    content: String,                       // Markdown
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
```

```rust
// Persisted mirror of a Google Calendar event for a specific calendar window.
// Written by the pull-sync pipeline; the scheduler reads these for conflict avoidance.
struct ExternalEventRecord {
    id: EntityId,
    calendar_id: String,
    event_id: String,           // Google Calendar event id; UNIQUE(calendar_id, event_id) in the table
    title: String,
    description: Option<String>,
    start_time: DateTime<Utc>,  // Stored as start_utc in DB
    end_time: DateTime<Utc>,    // Stored as end_utc in DB
    busy: bool,
    declined: bool,
    all_day: bool,              // True for a date-only (all-day) event; start/end are local-midnight day boundaries
    updated_at: DateTime<Utc>,
}

// Non-secret Google auth metadata (calendar selection + connection timestamp).
// OAuth tokens are stored on disk, not in the DB.
struct GoogleAuthState {
    calendar_id: Option<String>,
    connected_at: Option<DateTime<Utc>>,
}

// Per-chunk sync base for three-way merge (push phase).
// Cascade-deleted when the chunk is deleted.
struct ChunkSyncState {
    chunk_id: EntityId,
    event_id: String,
    etag: Option<String>,
    synced_start: DateTime<Utc>,
    synced_end: DateTime<Utc>,
    synced_title: String,
    synced_description: String,
    updated_at: DateTime<Utc>,
}
```

### 3.2 Enums (`domain/enums.rs`)

```rust
enum Priority { Low = 0, Medium = 1, High = 2, Critical = 3 }
enum TaskStatus { Backlog, Pending, Scheduled, Completed, Cancelled }
enum ChunkStatus { Scheduled, Completed }
```

### 3.3 Cadence (`domain/cadence.rs`)

A cadence is the recurrence *rule*, independent of when a template starts. `Cadence::occurrences` expands it against a separate anchor (`start_date`), mirroring iCalendar's RRULE/DTSTART split. Each `Window` is a contiguous span of in-period days and yields one occurrence per active period. Weekly and monthly differ only in how a period is located and advanced (`Period`); window resolution and the anchor filter are uniform.

```rust
/// Base recurrence period — locates and advances periods.
enum Period { Weekly, Monthly }

/// Contiguous range of 0-indexed in-period day offsets (inclusive).
/// Weekly: 0=Monday..6=Sunday. Monthly: 0=1st..27=28th (capped at 28th
/// so every month is guaranteed to have the day).
struct Window { start: u8, end: u8 }

/// Recurrence cadence: a base Period, an interval multiplier, and the
/// in-period day Windows to schedule (one instance per window).
/// Construction validates and canonicalizes windows (sorted, non-overlapping).
struct Cadence {
    period: Period,        // (private, accessor: period())
    interval: u8,          // (private) — e.g., 2 = every other week/month
    windows: Vec<Window>,  // (private, accessor: windows())
}

/// A single generated occurrence: the schedulable span of one Window in one
/// active period.
struct Occurrence {
    start: DateTime<Utc>,    // 00:00 (local) of the window's first day, in UTC
    deadline: DateTime<Utc>, // 23:59:59 (local) of the window's last day, in UTC
}

// Key methods:
// Cadence::new(period, interval, windows) → Result<Self, AppError>
// Cadence::occurrences(start_date, tz) → impl Iterator<Item = Occurrence>
// Cadence::expiry_for_occurrence(occ, next_start, tz) → Option<DateTime<Utc>>
```

---

## 4. Core Traits

### 4.1 Storage (`traits/storage.rs`)

Split into sub-traits per entity. Combined via `Store` supertrait. None of the sub-traits carry `Send + Sync` bounds — `Store::with_tx`'s closure receives a transaction-scoped `TxStore` that borrows a `&Connection` directly and is neither `Send` nor `Sync`. Long-lived cross-thread store handles add the bound at their own type: `AppState::store: Arc<dyn Store + Send + Sync>`.

```rust
trait TaskStore {
    // All CRUD methods persist/read task_labels join table alongside the task.
    // Labels on the Task model are denormalized from the join table on read,
    // and written to the join table on create/update.
    fn create_task(&self, task: &Task) -> Result<(), AppError>;
    fn get_task(&self, id: &str) -> Result<Option<Task>, AppError>;
    fn update_task(&self, task: &Task) -> Result<(), AppError>;
    fn delete_task(&self, id: &str) -> Result<(), AppError>;
    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, AppError>;
    /// Returns tasks eligible for scheduling: status IN (Pending, Scheduled)
    /// AND time_logged_minutes < duration_minutes (remaining time > 0).
    /// Excludes Backlog, Completed, and Cancelled tasks.
    fn get_schedulable_tasks(&self) -> Result<Vec<Task>, AppError>;
}

trait ChunkStore {
    fn create_chunk(&self, chunk: &Chunk) -> Result<(), AppError>;
    fn get_chunk(&self, id: &str) -> Result<Option<Chunk>, AppError>;
    fn update_chunk(&self, chunk: &Chunk) -> Result<(), AppError>;
    fn delete_chunk(&self, id: &str) -> Result<(), AppError>;
    fn get_chunks_for_task(&self, task_id: &str) -> Result<Vec<Chunk>, AppError>;
    fn get_chunks_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<Chunk>, AppError>;
    /// Chunks enriched with task info (title, priority, labels, deadline) in [start, end).
    fn get_agenda_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<AgendaItem>, AppError>;
    fn get_auto_chunks(&self) -> Result<Vec<Chunk>, AppError>;  // Non-fixed, non-completed (for reschedule diff)
    fn get_all_fixed_and_completed(&self) -> Result<Vec<Chunk>, AppError>;  // Immovable during reschedule
    fn get_fixed_scheduled_chunks(&self) -> Result<Vec<Chunk>, AppError>;   // Fixed + Scheduled (stale-lock release candidates)
    fn get_past_due_scheduled_chunks(&self, cutoff: DateTime<Utc>) -> Result<Vec<Chunk>, AppError>; // Scheduled with end_time < cutoff
}

trait ScheduleStore {
    fn create_schedule(&self, schedule: &Schedule) -> Result<(), AppError>;
    fn get_schedule(&self, id: &str) -> Result<Option<Schedule>, AppError>;
    fn get_default_schedule(&self) -> Result<Schedule, AppError>;
    fn update_schedule(&self, schedule: &Schedule) -> Result<(), AppError>;
    fn delete_schedule(&self, id: &str) -> Result<(), AppError>;
    fn list_schedules(&self) -> Result<Vec<Schedule>, AppError>;
}

trait RecurringTemplateStore {
    // All CRUD methods persist/read template_labels join table alongside the template.
    // Labels on the RecurringTemplate model are denormalized from the join table on read,
    // and written to the join table on create/update (same pattern as TaskStore + task_labels).
    fn create_template(&self, template: &RecurringTemplate) -> Result<(), AppError>;
    fn get_template(&self, id: &str) -> Result<Option<RecurringTemplate>, AppError>;
    fn update_template(&self, template: &RecurringTemplate) -> Result<(), AppError>;
    fn delete_template(&self, id: &str) -> Result<(), AppError>;
    fn list_templates(&self) -> Result<Vec<RecurringTemplate>, AppError>;
}

trait LabelStore {
    // Labels are not an aggregate root — they live denormalized in the task_labels
    // and template_labels join tables owned by TaskStore / RecurringTemplateStore.
    // This trait covers the cross-entity reads that belong to neither owner.
    /// Every distinct label (unioned across tasks and templates) with its task
    /// usage count, ordered by label. task_count counts tasks only (0 for
    /// template-only labels) so task-list filter chips match the filter results.
    fn list_labels(&self) -> Result<Vec<LabelCount>, AppError>;
}

trait ConfigStore {
    fn get_config(&self) -> Result<AppConfig, AppError>;
    fn update_config(&self, config: &AppConfig) -> Result<(), AppError>;
    /// Read a single raw config key. Returns None if the key is absent.
    fn get_config_value(&self, key: &str) -> Result<Option<String>, AppError>;
    /// Write a single raw config key (INSERT OR REPLACE).
    fn set_config_value(&self, key: &str, value: &str) -> Result<(), AppError>;
}

trait CommentStore {
    fn create_comment(&self, comment: &Comment) -> Result<(), AppError>;
    fn get_comment(&self, id: &str) -> Result<Option<Comment>, AppError>;
    fn update_comment(&self, comment: &Comment) -> Result<(), AppError>;
    fn delete_comment(&self, id: &str) -> Result<(), AppError>;
    fn list_comments_for_task(&self, task_id: &str) -> Result<Vec<Comment>, AppError>;
}

/// Persisted mirror of Google Calendar events for a specific calendar.
/// Written by the pull-sync pipeline; read by the reschedule pipeline for conflict avoidance.
trait ExternalEventStore {
    /// Upsert incoming events and delete removed events within [window_start, window_end)
    /// for the given calendar. The row id is preserved on conflict (event_id is the natural key).
    fn replace_external_events_in_window(
        &self,
        calendar_id: &str,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        events: &[ExternalEventRecord],
    ) -> Result<(), AppError>;
    fn get_external_events_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<ExternalEventRecord>, AppError>;
    fn clear_all_external_events(&self) -> Result<(), AppError>;
    /// Delete all mirrored rows for a single calendar (deselected from pull list).
    fn delete_external_events_for_calendar(&self, calendar_id: &str) -> Result<(), AppError>;
    /// Distinct calendar_id values present in the mirror (cleanup of deselected calendars).
    fn get_mirrored_calendar_ids(&self) -> Result<Vec<String>, AppError>;
    /// Insert or update a single mirror row by (calendar_id, event_id) — used for
    /// user-event write-through to echo the write into the local mirror immediately.
    fn upsert_external_event(&self, event: &ExternalEventRecord) -> Result<(), AppError>;
    fn get_external_event(&self, calendar_id: &str, event_id: &str) -> Result<Option<ExternalEventRecord>, AppError>;
    fn delete_external_event(&self, calendar_id: &str, event_id: &str) -> Result<(), AppError>;
}

/// Non-secret Google auth metadata. OAuth tokens live on disk, not here.
trait GoogleAuthStore {
    fn get_google_auth(&self) -> Result<Option<GoogleAuthState>, AppError>;
    fn set_google_auth(&self, auth: &GoogleAuthState) -> Result<(), AppError>;
    fn clear_google_auth(&self) -> Result<(), AppError>;
}

/// Per-chunk sync bases for three-way merge (push phase).
trait ChunkSyncStateStore {
    fn get_chunk_sync_states_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<ChunkSyncState>, AppError>;
    fn upsert_chunk_sync_state(&self, state: &ChunkSyncState) -> Result<(), AppError>;
    fn delete_chunk_sync_state(&self, chunk_id: &str) -> Result<(), AppError>;
    fn clear_all_chunk_sync_state(&self) -> Result<(), AppError>;
}

/// Combined trait — a single storage backend implements all sub-traits.
trait Store: TaskStore + ChunkStore + ScheduleStore + RecurringTemplateStore
    + LabelStore + ConfigStore + CommentStore + ExternalEventStore
    + GoogleAuthStore + ChunkSyncStateStore
{
    /// Run `f` inside a single storage transaction, committing on Ok and
    /// rolling back on Err. `f` receives a transaction-scoped `&dyn Store` —
    /// all store methods inside must use that parameter, never the outer `self`
    /// (the connection mutex is non-reentrant). Nested with_tx calls through
    /// the closure's store join the already-open transaction.
    fn with_tx(&self, f: &mut dyn FnMut(&dyn Store) -> Result<(), AppError>) -> Result<(), AppError>;

    /// Write a consistent single-file snapshot (VACUUM INTO) to `dest`.
    /// Default implementation refuses — only the top-level SqliteStore supports it.
    fn vacuum_into(&self, dest: &std::path::Path) -> Result<(), AppError>;
}
```

**TaskFilter** for queries (all fields use AND between them; `labels` is match-all within — a task matches only if it carries **every** listed label; `excluded_labels` is match-none — a task carrying **any** listed label is dropped):

```rust
struct TaskFilter {
    search_text: Option<String>,              // Case-insensitive substring match on title and description
    statuses: Option<Vec<TaskStatus>>,
    labels: Option<Vec<String>>,              // match-all: task must carry every listed label
    excluded_labels: Option<Vec<String>>,     // match-none: task must carry none of the listed labels
    unlabeled: Option<bool>,                  // true = only tasks with no labels; false = only labeled tasks
    priorities: Option<Vec<Priority>>,        // match-any, like statuses
    deadline_before: Option<DateTime<Utc>>,
    deadline_after: Option<DateTime<Utc>>,
    schedule_id: Option<EntityId>,
    recurring_template_id: Option<EntityId>,
}
```

### 4.1a Input DTOs (`domain/inputs.rs`)

Separate input types for create/update operations. Update fields are `Option`-wrapped — only provided fields are modified (patch semantics).

```rust
struct CreateTaskInput {
    title: String,
    description: Option<String>,
    duration_minutes: i64,
    priority: Option<Priority>,             // Default: Medium
    start_date: Option<DateTime<Utc>>,
    deadline: DateTime<Utc>,
    schedule_id: Option<EntityId>,          // Default: default schedule
    min_chunk_minutes: Option<i64>,         // Default: 30
    no_split: Option<bool>,                 // Default: false
    labels: Option<Vec<String>>,            // Default: []
    status: Option<TaskStatus>,             // Default: Pending (or Backlog if specified)
}

struct UpdateTaskInput {
    title: Option<String>,
    description: Option<Option<String>>,    // Some(None) clears description
    duration_minutes: Option<i64>,
    priority: Option<Priority>,
    start_date: Option<Option<DateTime<Utc>>>,
    deadline: Option<DateTime<Utc>>,
    schedule_id: Option<EntityId>,
    min_chunk_minutes: Option<i64>,
    no_split: Option<bool>,
    labels: Option<Vec<String>>,            // Replaces all labels
    status: Option<TaskStatus>,             // For backlog ↔ pending transitions
}

struct CreateTemplateInput {
    title: String,
    description: Option<String>,
    duration_minutes: i64,
    priority: Option<Priority>,             // Default: Medium
    schedule_id: Option<EntityId>,          // Default: default schedule
    cadence: Cadence,
    labels: Option<Vec<String>>,            // Default: []
    start_date: Option<DateTime<Utc>>,      // Reconcile anchor; defaults to now when None
    // No is_active — templates are always created as active.
    // Use UpdateTemplateInput.is_active to pause/resume instance generation.
}

struct UpdateTemplateInput {
    title: Option<String>,
    description: Option<Option<String>>,
    duration_minutes: Option<i64>,
    priority: Option<Priority>,
    schedule_id: Option<EntityId>,
    cadence: Option<Cadence>,
    labels: Option<Vec<String>>,
    is_active: Option<bool>,
    start_date: Option<DateTime<Utc>>,
}

struct CreateScheduleInput {
    name: String,
    windows: Vec<ScheduleWindowInput>,
}

struct UpdateScheduleInput {
    name: Option<String>,
    windows: Option<Vec<ScheduleWindowInput>>,  // Replaces all windows
}

struct ScheduleWindowInput {
    day_of_week: chrono::Weekday,
    start_time: NaiveTime,
    end_time: NaiveTime,
    // Validation: start_time < end_time (overnight windows like 23:00–02:00 are not supported;
    // users model these as two separate windows: 23:00–00:00 and 00:00–02:00).
    // Validation: windows for the same day_of_week must not overlap within a schedule.
    // Overlapping windows are rejected with a Validation error (not merged) to keep behavior explicit.
}

struct CreateCommentInput {
    task_id: EntityId,
    content: String,                       // Markdown
    author: Option<String>,                // Defaults to "User" when None; "SYSTEM" is reserved for auto-generated and rejected here
}

struct UpdateCommentInput {
    content: Option<String>,
}

struct UpdateConfigInput {
    planning_horizon_days: Option<i64>,
    timezone: Option<String>,
    max_continuous_minutes: Option<i64>,
    min_break_minutes: Option<i64>,
    // last_reschedule, last_mutation, last_sync, last_busy_sync are internal — not user-editable
}

/// Response type for GET /api/agenda
struct AgendaItem {
    chunk: Chunk,
    task_title: String,
    task_priority: Priority,
    task_labels: Vec<String>,
    task_recurring_template_id: Option<String>,  // Some ⇒ recurring instance (calendar "Edit template")
    task_deadline: Option<DateTime<Utc>>,        // Task deadline — lets the calendar mark chunks scheduled past it
}

/// Response type for GET /api/labels
struct LabelCount {
    label: String,
    task_count: i64,    // Tasks only; 0 for template-only labels
}
```

### 4.2 Scheduler (`traits/scheduling.rs`)

Pure algorithm — receives all data as parameters, returns results. No storage knowledge.

**Scheduling order**: Tasks are sorted by (1) higher priority first, (2) earlier deadline first, (3) shorter remaining duration first, (4) lexicographic by title. The scheduler places tasks in this order using a greedy first-fit approach.

**Chunk splitting**: For each task, the scheduler computes remaining duration as `duration_minutes - time_logged_minutes - sum(fixed chunk durations for this task)` using `existing_fixed_chunks`. If remaining ≤ 0, the task is fully allocated and skipped. If `no_split = true`, the entire remaining duration must fit in a single available slot; if no slot is large enough → `Unschedulable` warning. If `no_split = false`, the scheduler uses a **greedy approach** to minimize chunk count — chunks are made as large as possible (up to `max_continuous_minutes` from config), placed into the largest matching slots first until the remaining duration is fully allocated. Minimum chunk size is `min_chunk_minutes`. **Exception**: the final chunk may be smaller than `min_chunk_minutes` if the remaining unallocated time is less than `min_chunk_minutes` — this ensures tasks can always be fully scheduled.

**Validation boundary**: The service layer rejects task/template configurations that can never fit their assigned schedule. Effectively unsplittable work — explicit `no_split=true`, short tasks auto-promoted to `no_split` (`duration_minutes <= min_chunk_minutes`), and recurring instances/templates — must have `duration_minutes` no greater than the largest window in the assigned schedule. For splittable work, `min_chunk_minutes` must not exceed that largest window (the duration itself is irrelevant: it splits). This keeps permanently invalid work out of the scheduler. The boundary also rejects blank task/template titles (trim-checked) and tasks whose `start_date` falls after their `deadline`. Both rules are enforced on create and on non-terminal update (using effective post-patch values). Terminal tasks skip re-validation so cosmetic edits to history records are never blocked by legacy bad data.

**Warning semantics**: Warnings are horizon-aware. Work that merely extends beyond `horizon_end` remains `Pending` without a warning unless it has a deadline on or before `horizon_end`. `DeadlineViolation` is the warning for in-horizon deadlines that cannot be met. `Unschedulable` is reserved for genuine in-horizon impossibility under the current schedule/capacity, not backlog that may become schedulable in a later horizon.

**Break enforcement**: The scheduler tracks cumulative back-to-back scheduled time across all chunks (same or different tasks). When placing a chunk that would cause the continuous block to exceed `max_continuous_minutes`, the scheduler inserts a gap of at least `min_break_minutes` before placing the next chunk. Rules:

- A `no_split` task is placed as a single block (even if longer than `max_continuous_minutes`), but a break is enforced **after** it before the next chunk.
- Within a splittable task: if remaining time > `max_continuous_minutes`, the chunk is capped at `max_continuous_minutes` and a break gap is inserted before the next chunk of the same or different task.
- The break is modeled as consumed slot time (reduces available slot capacity), not as a separate entity.

**Start date constraint**: Tasks with a `start_date` must not be placed in slots that begin before that date. The scheduler skips earlier slots for these tasks.

**Schedule affinity** (M6.3): Each task has a `schedule_id`. The scheduler **must only** place a task into `AvailableSlot`s whose `schedule_id` matches the task's `schedule_id`. Implementations must enforce this — placing a task into a non-matching slot is a bug.

**Fixed chunk handling**: The scheduler receives `existing_fixed_chunks` and uses them for two purposes: (1) **time budget**: deduct each task's fixed chunk durations from its remaining time before placing auto-chunks, and (2) **conflict avoidance**: must not place auto-scheduled chunks that overlap with any fixed chunk's time range. Note: fixed chunks themselves _can_ overlap each other (user-placed intentionally) — the scheduler only avoids overlapping fixed chunks with _new auto-scheduled_ chunks.

```rust
struct AvailableSlot {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    schedule_id: EntityId,
}

struct ScheduleResult {
    placed_chunks: Vec<Chunk>,
    warnings: Vec<ScheduleWarning>,
}

struct ScheduleWarning {
    task_id: EntityId,
    task_title: String,
    kind: WarningKind,
}

enum WarningKind {
    DeadlineViolation { deadline: DateTime<Utc>, earliest_completion: DateTime<Utc> }, // deadline is inside the active horizon, but completion slips past it
    Unschedulable { reason: String }, // genuine in-horizon impossibility, never mere post-horizon spillover
}

/// Input struct wraps all scheduling data. Enriched over time
/// (e.g., task dependency graph for C1) without breaking the trait signature.
struct ScheduleInput {
    tasks: Vec<Task>,
    existing_fixed_chunks: Vec<Chunk>,
    available_slots: Vec<AvailableSlot>,
    horizon_end: DateTime<Utc>,
    now: DateTime<Utc>,                    // Wall-clock time; used for created_at/updated_at on new chunks
    max_continuous_minutes: i64,           // From AppConfig
    min_break_minutes: i64,                // From AppConfig
    // Future: dependencies: HashMap<EntityId, Vec<EntityId>>,  // C1
}

trait Scheduler: Send + Sync {
    fn schedule(&self, input: ScheduleInput) -> Result<ScheduleResult, AppError>;
}
```

### 4.3 Calendar Sync (`traits/calendar_sync.rs`)

Abstract external calendar integration. Pull phase: fetch all remote events for the planning horizon per selected calendar; the scheduler reads the local mirror for conflict avoidance. Push phase: three-way merge (local chunks vs. remote app-owned events vs. `ChunkSyncState` bases) produces `SyncOp`s executed in batches.

**Client credentials**: `client_id` and `client_secret` are compiled into the binary at build time from the `APRESWORK_GOOGLE_CLIENT_ID` / `APRESWORK_GOOGLE_CLIENT_SECRET` environment variables (`option_env!` in `GoogleCredentials::compiled`); the repository holds no credential, and a build without them yields a provider that reports itself unavailable. For Google OAuth "Desktop" app type, the client_secret is not truly secret (Google documents this) — it is embedded in every distributed copy of the binary. This is industry-standard for desktop OAuth apps (same as Chrome, VS Code, gcloud CLI, etc.).

Auth is a loopback-redirect flow: `begin_auth` returns the consent URL and the exchange
completes in the background; there is no `complete_auth(code)` (providers removed the oob
copy-paste flow). Event pull is full-content per selected calendar.

```rust
enum AuthStatus {
    NotConnected,
    Pending,
    Connected { email: Option<String> },
}

struct ExternalCalendar {
    id: String,
    title: String,
    primary: bool,
}

struct ExternalEvent {
    calendar_id: String,
    event_id: String,
    title: String,
    description: Option<String>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    busy: bool,
    declined: bool,
    all_day: bool,             // True for date-only events; start/end are full local-day UTC span
}

/// An event read from the dedicated app calendar (with etag for three-way merge).
/// Only returns events bearing the apreswork_chunk_id extended property.
struct RemoteChunkEvent {
    event_id: String,
    etag: Option<String>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    title: String,
    description: Option<String>,
}

/// Content to write for a chunk event (provider-neutral).
struct ChunkEventPayload {
    chunk_id: String,
    title: String,
    description: String,       // Multi-chunk: "Part N of M — ..."; single-chunk: "Après Work\n\n..."
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

/// Content to write for a user-owned calendar event (provider-neutral).
struct UserEventPayload {
    title: String,
    description: Option<String>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    all_day: bool,
}

enum SyncOp {
    Create(ChunkEventPayload),
    Update { event_id: String, payload: ChunkEventPayload },
    Delete { event_id: String },
}

enum SyncOpResult {
    Created { chunk_id: String, event_id: String, etag: Option<String> },
    Updated { chunk_id: String, event_id: String, etag: Option<String> },
    Deleted,
}

trait CalendarSync: Send + Sync {
    fn begin_auth(&self, now: DateTime<Utc>, now_instant: Instant) -> Result<String, AppError>;
    fn auth_status(&self, now_instant: Instant) -> AuthStatus;
    fn disconnect(&self) -> Result<(), AppError>;
    fn is_available(&self) -> bool;
    fn list_calendars(&self, now: DateTime<Utc>) -> Result<Vec<ExternalCalendar>, AppError>;
    fn list_events(&self, now: DateTime<Utc>, calendar_id: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<ExternalEvent>, AppError>;
    /// Get or create the dedicated "Après Work" calendar; return its provider ID.
    fn ensure_app_calendar(&self, now: DateTime<Utc>) -> Result<String, AppError>;
    /// List app-owned chunk events (bearing the apreswork_chunk_id marker) on [start, end).
    fn list_app_calendar_events(&self, now: DateTime<Utc>, calendar_id: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<RemoteChunkEvent>, AppError>;
    /// Execute sync ops via the multipart batch API (≤250 ops/req). Returns one SyncOpResult per op.
    fn execute_sync_ops(&self, now: DateTime<Utc>, calendar_id: &str, ops: &[SyncOp]) -> Result<Vec<SyncOpResult>, AppError>;
    /// Create a user-owned event on the given calendar (write-through to the mirror).
    fn create_user_event(&self, now: DateTime<Utc>, calendar_id: &str, payload: &UserEventPayload) -> Result<ExternalEvent, AppError>;
    fn update_user_event(&self, now: DateTime<Utc>, calendar_id: &str, event_id: &str, payload: &UserEventPayload) -> Result<ExternalEvent, AppError>;
    fn delete_user_event(&self, now: DateTime<Utc>, calendar_id: &str, event_id: &str) -> Result<(), AppError>;
}
```

### 4.4 Backup (`traits/backup.rs`)

Provider-generic backup storage. One backup file per target; retention is the target's own version history.

```rust
struct RemoteBackupMeta {
    last_mutation: Option<DateTime<Utc>>,   // The exporting DB's last_mutation at upload time
    schema_version: i64,                    // Restore refuses a backup newer than the running binary
}

trait BackupTarget: Send + Sync {
    fn is_available(&self) -> bool;         // Cheap probe (no network): credentials present
    fn get_meta(&self, now: DateTime<Utc>) -> Result<Option<RemoteBackupMeta>, AppError>;
    fn upload(&self, now: DateTime<Utc>, zip_bytes: &[u8], meta: &RemoteBackupMeta) -> Result<(), AppError>;
    fn download(&self, now: DateTime<Utc>) -> Result<Vec<u8>, AppError>;
}
```

---

## 5. Service Layer

Services contain business logic. Commands are thin wrappers.

### 5.1 TaskService (`services/task.rs`)

```
task::create_task(store, input)
  → Resolve schedule (explicit or default)
  → Validate basic fields: duration>0, min_chunk>=5, deadline required
  → If duration_minutes <= min_chunk_minutes → auto-set no_split=true
    (a task shorter than its minimum chunk size can't be split anyway)
  → Validate target schedule capacity using the resolved schedule's largest window.
    Effective no_split = (no_split || duration_minutes ≤ min_chunk_minutes).
    If effective_no_split: duration_minutes ≤ largest window (task cannot be split).
    Otherwise (splittable): min_chunk_minutes ≤ largest window.
    A bogus explicit schedule_id → NotFound (not a DB FK error).
  → Generate UUID, set defaults (priority=Medium, schedule=default, status=Pending/Backlog)
  → store.create_task()

task::update_task(store, task_id, input)
  → Apply patch semantics (only update provided fields)
  → If duration_minutes provided: validate duration_minutes >= task.time_logged_minutes.
    If violated, return Validation error suggesting the user resize individual chunks
    instead (resize_chunk handles reducing completed chunk duration directly).
  → Validate the resulting task against the target schedule's largest window
    using the same effective-no-split rule as create. Validation is skipped when
    the resulting status is terminal (Completed/Cancelled). A bogus schedule
    reassignment → NotFound.
  → If status provided: allowed transitions are Backlog↔Pending and Scheduled→Backlog.
    Scheduled→Backlog: remove all non-fixed, non-completed chunks for the task and
    set status=Backlog. Fixed chunks are retained (the user explicitly placed them).
    Other status changes use dedicated operations: cancel_task, complete_chunk, etc.
  → If title changed → set config.last_mutation = now (sync trigger)

task::delete_task(store, task_id)
  → If task has recurring_template_id: set status=Cancelled instead of deleting.
    Rationale: deleting removes the row entirely, so reconcile would see
    no instance for that cadence period and regenerate it. Cancelling preserves the
    row (preventing regeneration) while hiding it from scheduling. Users who want
    to truly purge history can use a separate "purge cancelled" operation (future).
  → Otherwise: store.delete_task() (CASCADE deletes chunks)
  → Set config.last_mutation = now (sync trigger — next diff cleans up orphaned GCal events)
  → Trigger full reschedule (freed slots may benefit other tasks)
  → Note on M2.7 ("permanent delete"): for recurring instances, cancellation IS the
    correct "delete" — the cancelled row prevents the single-pass reconciliation
    from re-creating the instance. True row deletion would cause the instance to
    reappear on the next reconcile. From the user's perspective, the instance
    disappears from active views (satisfying M2.7 intent).

task::create_fixed_chunk(store, task_id, start_time, end_time)
  → Get task, validate it exists and is not Completed/Cancelled
  → Get existing chunks for task (store.get_chunks_for_task)
  → Compute allocated = sum of FIXED chunk durations only (auto-scheduled chunks are
    excluded because they will be recomputed during the next reschedule — this matches
    the scheduler's own remaining-time calculation in §4.2)
  → Validate chunk duration <= remaining (duration_minutes - time_logged_minutes - allocated)
  → No overlap validation — fixed chunks can be placed anywhere, including on top of
    auto-scheduled or other fixed chunks. Overlapping auto-chunks are displaced on the
    next reschedule; overlapping fixed chunks are shown as conflicts in the calendar
    (similar to Google Calendar behavior).
  → Generate UUID, create Chunk with is_fixed=true, status=Scheduled
  → If task.status == Pending → set task.status = Scheduled
  → Set config.last_mutation = now (sync trigger)
  → Trigger a full reschedule (FixedChunkCreated is Full/Immediate in the §5.2.2 table:
    the new block can displace auto-chunks of any task)
  → Used by: calendar click-drag creation (M8.5), pre-scheduling fixed chunks (M5.4)

task::complete_chunk(store, chunk_id, duration_override?)
  → Get chunk + task
  → logged = duration_override or (chunk.end_time - chunk.start_time) in minutes
  → Set chunk.status=Completed, chunk.completed_at=now, chunk.logged_minutes=logged
  → Add logged to task.time_logged_minutes
  → If time_logged >= duration → task.status=Completed
  → Update both
  → Set config.last_mutation = now (sync trigger)

task::reopen_chunk(store, chunk_id)
  → Get chunk + task
  → Subtract chunk.logged_minutes from task.time_logged_minutes
  → Set chunk.status=Scheduled, chunk.completed_at=None, chunk.logged_minutes=None
  → If task.status == Completed → set task.status = Scheduled
  → Update both
  → Set config.last_mutation = now (sync trigger)

task::move_chunk(store, chunk_id, new_start, new_end)
  → No overlap validation — fixed chunks can overlap with any other chunk.
    Overlapping auto-scheduled chunks are displaced on the next reschedule.
    Overlapping fixed chunks are shown as conflicts in the calendar UI.
  → Update times, set is_fixed=true
  → Set config.last_mutation = now (sync trigger)
  → Scheduler should be triggered in cascading mode for any other chunks that may be overlapping.
    The trigger layer supports debouncing, but local reschedules currently run with
    zero delay by default for immediate feedback.

task::lock_chunk(store, chunk_id)
  → Get chunk, validate it exists and status == Scheduled (completed chunks can't be locked)
  → Set chunk.is_fixed = true — times untouched (the honest counterpart of unlock;
    no move_chunk round-trip to the chunk's own times)
  → store.update_chunk(), then sync the task's is_pinned flag
  → Debounced incremental reschedule for the task (same policy as unlock)

task::unlock_chunk(store, chunk_id)
  → Get chunk, validate it exists and status == Scheduled (completed chunks can't be unlocked)
  → Set chunk.is_fixed = false
  → store.update_chunk()
  → Once chunk is unlocked, scheduler should handle it in cascading mode.
    The trigger layer keeps an optional debounce path, but local reschedules
    currently default to zero delay.

task::complete_task(store, task_id)
  → Get task
  → Set task.time_logged_minutes = task.duration_minutes
  → Set task.status=Completed
  → Mark all scheduled (non-completed) chunks for this task as Completed
    (logged_minutes = their scheduled duration, completed_at = now)
  → Set config.last_mutation = now (sync trigger)
  → Trigger full reschedule (freed slots may benefit other tasks)

task::cancel_task(store, task_id)
  → Get task
  → Set task.status=Cancelled
  → Delete all scheduled (non-completed) chunks for this task
  → Completed chunks are retained in history
  → Set config.last_mutation = now (sync trigger)
  → Trigger full reschedule (freed slots may benefit other tasks)

task::delete_fixed_chunk(store, chunk_id)
  → Get chunk, validate it exists, is_fixed=true, status=Scheduled
  → Delete the chunk
  → Sync the task's is_pinned flag (may clear if no more fixed chunks)
  → Set config.last_mutation = now (sync trigger)
  → Trigger incremental reschedule for the task

task::get_agenda(store, start, end, label_filter?)
  → Get chunks in date range (via store.get_chunks_in_range)
  → Join with task data (title, priority, labels)
  → If label_filter provided, keep only chunks whose task has a matching label
  → Return Vec<AgendaItem>

task::resize_chunk(store, chunk_id, new_end)
  → Resize is end-edge only (standard calendar behavior). To change a chunk's start
    time, use move_chunk (which also makes it fixed). Budget adjustment happens on
    the next reschedule in that case.
  → Set chunk.end_time = new_end, chunk.is_fixed = true
  → If completed chunk:
      delta = new_duration - chunk.logged_minutes
      task.time_logged_minutes += delta
      chunk.logged_minutes = new_duration
  → Set config.last_mutation = now (sync trigger)
  → Queue incremental reschedule for task.id.
    The trigger layer supports debouncing, but local reschedules currently run
    with zero delay by default for immediate feedback.
  → Note: the duration invariant (duration - time_logged == sum(scheduled chunks))
    is **eventually consistent** — the incremental reschedule fixes it within seconds.
    Between resize and reschedule, total committed time may temporarily exceed
    task.duration_minutes. This is safe because no user-facing operation depends on
    the invariant (complete_chunk only checks time_logged vs duration).
```

### 5.2 SchedulingService (`services/scheduling.rs`)

Two scheduling modes: **full reschedule** (recompute everything) and
**incremental reschedule** (cascading, priority-aware, minimal disruption).

#### 5.2.1 Full Reschedule

```
scheduling::reschedule(store, scheduler, now)
  1. Get config (horizon, timezone)
  2. Reconcile recurring instances within the horizon (`recurring::reconcile` per
     template, idempotent — existing occurrences are matched, missing ones created)
  3. Auto-cancel overdue recurring instances (M4.5)
  3a. Release stale fixed locks (`release_stale_fixed_locks`): a fixed chunk still
      `scheduled` more than 4 hours after its end time is unlocked back into an
      auto-chunk and the owning task's `is_pinned` is re-derived (SCHEDULER_ALGORITHM.md).
      The incremental path runs the same step.
  4. Get all schedulable tasks (pending + scheduled with remaining time)
  5. Get all existing non-completed auto-chunks (the "old schedule")
  6. Get all fixed chunks (to avoid conflicts)
  7. Compute available slots from all schedules (via slot_finder, using config.timezone
     to convert local schedule windows to concrete UTC time ranges).
     DST handling: slot_finder iterates each calendar day in the planning horizon,
     converts each ScheduleWindow's (day_of_week, start_time, end_time) from the
     configured timezone to UTC using chrono-tz. Because DST offset varies by date,
     the same local window (e.g., 18:00–23:00) maps to different UTC ranges on
     different days. Each day is converted independently — no caching of offsets.
  7a. [S6] Read the local `external_events` mirror for the horizon (busy=1 only;
      transparent and declined events carry busy=false and never subtract from slots).
      Subtract these busy external events from available_slots before passing to the
      scheduler. If the mirror is empty (offline, never pulled), proceed without
      conflict checking. The mirror is refreshed by `pull_external_events` — the
      reschedule pipeline never makes network calls.
  7b. Align free slots to the minute grid (`align_slots_to_grid`, SCHEDULER_ALGORITHM.md
      §4.1): starts round up, ends round down, sub-minute slots dropped. This makes
      every generated chunk minute-precise; ragged edges enter only from the `now`
      horizon clip and second-precision busy-event boundaries.
  8. Call scheduler.schedule(ScheduleInput { tasks, fixed_chunks, slots, horizon_end })
  9. Diff old auto-chunks vs. new placed_chunks (greedy matching):
     - Group old and new chunks by task_id
     - For each task, pair old↔new by closest start time (greedy)
     - Paired + identical → KEEP (preserves google_event_id, no sync needed)
     - Paired + times differ → UPDATE in place (preserves google_event_id)
     - Unpaired old → DELETE
     - Unpaired new → CREATE
  10. Update task statuses:
      - pending → scheduled (where chunks were placed)
      - scheduled → pending (where no chunks remain after diff AND no fixed chunks exist for that task)
  11. Update config.last_reschedule = now, config.last_mutation = now
  12. Return ScheduleResult with warnings:
      - DeadlineViolation only for deadlines on/before horizon_end
      - no warning for tasks that simply remain pending beyond horizon_end
      - Unschedulable only for genuine in-horizon impossibility
```

#### 5.2.2 Incremental Reschedule (Cascading)

Reschedules specific tasks with priority-aware cascading. Higher-priority tasks
can displace lower-priority auto-chunks, which cascade into the reschedule set.
Typical case: 1–3 tasks touched. Worst case degrades gracefully to full reschedule.

```
scheduling::reschedule_incremental(store, scheduler, initial_task_ids, now)
  1. Get config (horizon, timezone)
  2. Fix invariant for affected tasks:
       for task_id in initial_task_ids:
         task = store.get_task(task_id)
         fixed_durations = sum(fixed chunk durations for this task)
         total_committed = task.time_logged_minutes + fixed_durations
         if total_committed > task.duration_minutes:
           task.duration_minutes = total_committed
           store.update_task(task)
  3. Get ALL schedulable tasks sorted by priority order
  4. Get all fixed chunks
  5. Compute available slots (same as full reschedule steps 7/7a)
  6. Cascading placement:
       affected = set(initial_task_ids)
       old_auto_by_task = {}    // snapshot of auto-chunks before changes
       new_auto_by_task = {}    // new placements
       occupied = available_slots  // starts as full schedule windows minus fixed

       for task in all_tasks_by_priority:
         if task.id in affected:
           // Save old auto-chunks for diff
           old_auto_by_task[task.id] = get_auto_chunks(task.id)
           // Place this task — may claim slots from lower-priority tasks
           result = scheduler.schedule(ScheduleInput {
             tasks: [task], fixed_chunks, slots: occupied, ...
           })
           new_auto_by_task[task.id] = result.placed_chunks
           // Check for displaced chunks from not-yet-processed tasks
           for chunk in result.placed_chunks:
             for other_task_id, other_chunks in existing_auto_chunks:
               if other_task_id not yet processed
                  and any(overlaps(chunk, oc) for oc in other_chunks):
                 affected.add(other_task_id)
           // Consume placed slots from occupied
           occupied = subtract(occupied, result.placed_chunks)
         else:
           // Unaffected task — keep existing auto-chunks, consume their slots
           occupied = subtract(occupied, get_auto_chunks(task.id))

  7. Diff old vs new for all affected tasks (same algorithm as full reschedule step 9)
  8. Update task statuses for affected tasks
  9. Update config.last_mutation = now
  10. Return ScheduleResult with warnings using the same horizon-aware semantics
      as full reschedule (no generic "spillover beyond horizon" warning)
```

**Convergence guarantee**: Tasks are processed in strict priority order (same
ordering as full reschedule). A processed task can only displace unprocessed
(lower-priority) tasks. Each task is processed at most once. No cycles possible.

**Trigger mapping**:

The mutation rows below are materialized exactly once, in
`policy_for(mutation)` next to `RescheduleMode` in `services/trigger.rs`.
Command handlers (Tauri and REST) report the fact of what happened as a
`Mutation` value via `RescheduleTrigger::trigger_mutation`; they never pick a
mode or timing themselves. "Debounced" is the policy classification — the
composition root currently configures zero debounce, so debounced actions
also execute immediately in practice (see the zero-delay notes in §5.1).

| User action                       | Mode                    | Timing    |
| --------------------------------- | ----------------------- | --------- |
| Create task                       | Incremental (that task) | Debounced |
| Update task (not to Backlog)      | Incremental (that task) | Debounced |
| Update task → Backlog             | Full                    | Debounced |
| Delete task                       | Full                    | Debounced |
| Cancel task                       | Full                    | Debounced |
| Complete task                     | Full                    | Immediate |
| Create fixed chunk                | Full                    | Immediate |
| Complete/reopen chunk             | Incremental (that task) | Immediate |
| Move chunk (drag)                 | Incremental (that task) | Debounced |
| Resize chunk                      | Incremental (that task) | Debounced |
| Lock chunk (in place)             | Incremental (that task) | Debounced |
| Unlock chunk                      | Incremental (that task) | Debounced |
| Delete fixed chunk                | Incremental (that task) | Debounced |
| Template create/update/delete     | Full                    | Immediate |
| Schedule window create/update/del | Full                    | Immediate |
| Config update                     | Full                    | Immediate |
| Manual "Reschedule All" button    | Full                    | Immediate |
| Startup (>24h stale)              | Full                    | Immediate |

The last two rows are not `Mutation` values: the manual button calls the
scheduler commands directly, and the startup check runs during profile
activation (`profiles::activate`) whenever `config.last_reschedule` is unset or
at least 24 hours old. The trigger's background maintenance path (§7) handles the
other periodic cases: past-due chunks and midnight crossings.

#### 5.2.3 Duration Invariant

The invariant `task.duration_minutes - task.time_logged_minutes == sum(scheduled chunk durations)`
is **eventually consistent**. Operations like resize_chunk and move_chunk may temporarily
break it. The invariant is repaired by the incremental reschedule orchestrator (step 2 above)
before any scheduling decisions are made. This is safe because:

- `complete_chunk` only checks `time_logged >= duration` (not affected by stale chunk sizes)
- `reopen_chunk` only subtracts `chunk.logged_minutes` (locally correct)
- The UI may briefly show over-allocation, resolved within seconds

### 5.3 RecurringService (`services/recurring.rs`)

```
recurring::create_template(store, input)
  → Resolve schedule (explicit or default)
  → Validate title and positive duration (`domain::validation::validate_create_template`).
    No cadence-level validation exists: the `Cadence { period, interval, windows }`
    value is accepted as deserialized (**not implemented** — REQUIREMENTS.md M4.1
    records the model change from the original per-week count rule)
  → Validate duration_minutes <= largest window in assigned schedule
    (recurring instances are always created as no_split=true)
  → Generate UUID, set defaults
  → store.create_template()

recurring::reconcile(store, template, horizon_start, horizon_end)
  → Generate occurrences via `Cadence::occurrences(template.start_date, tz)`:
      the infinite lazy iterator yields one Occurrence per window per active period.
      interval counts on-cadence periods forward from the anchor (template.start_date).
    → Single-pass reconciliation matches existing instances to occurrences by
      id/order — no cadence_period_key or unique index.
    → Skip if instance already exists for this occurrence.
    → deadline derived from occurrence.deadline; "end of day" = 23:59:59 in the
      configured timezone, converted to UTC.
    → expire_at via `Cadence::expiry_for_occurrence(occ, next_start, tz)` — period-aware
      (M4.5): for weekly, end of the first day of the next occurrence's window;
      for monthly, the occurrence's own widened deadline.
    → Instance inherits: duration, priority, labels, schedule, no_split=true
    → Pinned instances (is_pinned=true) are never repositioned or deleted by reconcile

recurring::auto_cancel_overdue(store, now)
  → For each pending/scheduled recurring instance with expire_at set: if now > expire_at,
    set status=Cancelled and remove its scheduled chunks.

recurring::update_template(store, template_id, updates)
  → Update template fields
  → Validate the resulting template against the target schedule's largest window
    (duration_minutes must fit; checked on every update, including reactivation)
  → Delete the future open *unpinned* instances (pending/scheduled with deadline
    after now), hand-tweaked ones included; pinned, completed, cancelled and
    overdue instances survive (see §5.5 and REQUIREMENTS.md M4.2a)
  → reconcile regenerates the missing occurrences within the planning horizon
  → Set config.last_mutation = now (sync trigger — deleted instances may have GCal events)

recurring::get_orphaned_template_instances(store, now)
  → For each active template with no pending/scheduled instances within the
    planning horizon, compute the next cadence period's deadline and return
    a virtual (non-persisted) task-like object so the template remains
    accessible in the task list (M9.5)

recurring::delete_template(store, template_id)
  → Delete all pending/scheduled instances
  → Set recurring_template_id=NULL on completed/cancelled instances (de-link)
  → Delete template
  → Set config.last_mutation = now (sync trigger — deleted instances may have GCal events)
```

### 5.4 ScheduleService (`services/schedule.rs`)

```
schedule::create_schedule(store, input, now)
  → Validate: name non-empty, windows non-empty, each window start_time < end_time,
    no overlapping windows on the same day_of_week
  → Generate UUID, store.create_schedule()

schedule::update_schedule(store, schedule_id, input, now)
  → Reject if schedule.is_default AND name is being changed (default schedule name is immutable)
  → Validate windows (same rules as create)
  → When windows are replaced, reject if any non-terminal (Backlog/Pending/Scheduled)
    task or active template would no longer fit the new largest window.
    Uses the same effective-no-split rule as task create/update. Inactive templates
    are re-validated at reactivation by update_template.
  → store.update_schedule()
  → Set config.last_mutation = now (sync trigger)
  → Note: caller should trigger reschedule after update — existing chunks may
    no longer fit the updated time windows

schedule::delete_schedule(store, schedule_id, now)
  → Reject if schedule.is_default (return Validation error)
  → Reassign all tasks with this schedule_id to the default schedule
  → Reassign all recurring templates with this schedule_id to the default schedule
  → Delete the schedule (CASCADE deletes its windows)
  → Set config.last_mutation = now (sync trigger)
  → Note: caller should trigger reschedule after deletion — reassigned tasks
    may need chunks repositioned into the default schedule's windows
```

### 5.5 SyncService (`services/sync.rs`)

Currently implemented:

- `disconnect_provider(store, calendar_sync)` — deletes the local provider token
  (calls `calendar_sync.disconnect()`; remote data is never touched) and atomically
  clears all Google auth state from the DB (`google_auth`, `external_events`,
  `chunk_sync_state`) in a single `store.with_tx(…)` transaction.

- `pull_external_events(store, calendar_sync, now)` — refreshes the local
  `external_events` mirror. For each calendar id in `pull_calendar_ids`, calls
  `list_events(id, now-7d, now+horizon)` and applies the result via
  `replace_external_events_in_window` (upsert + delete-removed). Returns `Ok(())`
  immediately when the provider is unavailable or no calendars are selected.

- `sync_cycle(store, calendar_sync, scheduler, trigger, now)` — push sync to the
  dedicated app calendar via three-way merge (local chunks vs. remote app-owned
  events vs. `chunk_sync_state` bases). Four steps: fetch (no lock), merge +
  local apply (guard + tx), push ops (no lock), record bases (guard + tx), then
  an incremental reschedule for tasks whose chunks were remotely deleted.
  Merge time comparisons truncate to whole seconds (the provider stores second
  precision, so sub-second diffs are echo, not change). Conflict policy: app
  wins; valid remote moves are accepted and pin the chunk (`is_fixed`); moves
  past the deadline are reverted; a mass-delete guard aborts when remote drops
  more than `max(5, synced/2)` synced chunks. Returns `PushCounts`
  (created/updated/deleted provider ops). (Three-way merge, not a simple
  google_event_id diff.)

- `sync_now(store, calendar_sync, scheduler, trigger, now)` — manual "Sync now":
  `pull_and_reschedule` then `sync_cycle`, so pushed events reflect final chunk
  placements. Records `last_sync_at` / `last_sync_error` config bookkeeping when
  the provider is available; returns `SyncOutcome { schedule: ScheduleResult,
  pushed: PushCounts }` (counts are zero while disconnected).

- `get_sync_status(store)` — reads that bookkeeping (`SyncStatus { last_sync_at,
  last_sync_error }`); lenient on malformed timestamps (display-only value).

> **Not implemented** — an automatic sync trigger. `sync_cycle` runs only on demand (Sync button, REST sync-now) and pulls run only on demand (Pull button, REST calendar-pull, or the first phase of sync-now). Config seeds `sync_debounce_minutes` (2) and `sync_poll_minutes` (60) but no timer reads them yet. The intended design:

```
Sync trigger: debounced, mutation-driven.
  → Any chunk mutation (complete, reopen, move, resize, reschedule) OR
    task title/description change (affects GCal event content) sets
    config.last_mutation = now
  → A background timer checks periodically:
    - Chunk sync: if last_mutation > last_sync AND (now - last_mutation) >= sync_debounce_minutes → sync_cycle
    - Event pull: every sync_poll_minutes; the manual pull button in Settings
      forces an immediate refresh.
  → This batches rapid edits while ensuring changes propagate within a few minutes.
  → Task deletion cleanup: the next sync diff sees remote events with no matching
    local chunk and deletes them automatically — no special handling needed.

Local reschedule trigger note:
  → The command-layer trigger retains a configurable debounce path so local
    scheduling can be tuned later, but the current app wiring sets the local
    reschedule delay to zero for immediate feedback during task/chunk edits.
  → Revisit this once calendar sync and other background work are fully landed,
    so local scheduling latency and sync batching can be tuned independently.

Note on recurring instance editability: individual recurring instances can be
edited via update_task (change description, duration, etc.). Editing the parent
template's cadence or anchor (M4.2a) deletes only the future open *unpinned*
instances (deadline after now) — including ones whose fields were tweaked by
hand — and reconcile regenerates them. Pinned (moved), completed, cancelled and
overdue instances survive. REQUIREMENTS.md records this as a divergence from the
original "erase all pending/scheduled instances" wording.

User-event CRUD — creating/editing Google Calendar events from within the app:
  → sync::create_user_event(store, calendar_sync, scheduler, trigger, calendar_id, payload, now)
    → Writes the event to the provider (calendar_sync.create_user_event)
    → Echoes the result into the local external_events mirror (upsert_external_event)
    → Triggers incremental reschedule: the new busy event displaces overlapping auto-chunks
  → sync::update_user_event(store, calendar_sync, scheduler, trigger, calendar_id, event_id, payload, now)
    → Updates the event on the provider
    → Echoes the result into the local mirror
    → Triggers incremental reschedule for affected tasks
  → sync::delete_user_event(store, calendar_sync, scheduler, trigger, calendar_id, event_id, now)
    → Deletes the event on the provider
    → Removes it from the local mirror
    → Triggers full reschedule (freed slot may benefit other tasks)

Calendar picker CRUD:
  → sync::get_pull_calendars(store) — read the persisted calendar picker selection
  → sync::set_pull_calendars(store, calendar_ids) — persist the selection; no reschedule
```

### 5.6 CommentService (`services/comment.rs`)

Thin CRUD wrappers. `create_comment` injects the acting author ("User") and `now` timestamp; "SYSTEM" as author is reserved for auto-generated comments and rejected on user input. Comments never trigger a reschedule.

### 5.7 BackupService (`services/backup/`)

Orchestrates gated exports, backup-wins restore, and staged import.

- `get_backup_status(store, backup)` — reads backup_enabled config + target availability + last timestamps.
- `set_backup_enabled(store, backup, enabled)` — toggles and immediately exports if enabling.
- `export_now(store, backup)` — VACUUM INTO snapshot, zip, upload to target, record timestamp.
- `export_to_file(store, path)` — file-based export (save-file dialog in Tauri, path in REST body).
- `restore_check(backup, schema_version)` — freshness probe before unlock; if remote backup is newer, the caller stages a backup-wins restore.
- `stage_import(path)` / `apply_pending_import(...)` — verify zip, swap DB file, re-activate.
- A background timer (`start_backup_timer`) periodically exports if dirty (export_if_dirty), and `export_on_exit` flushes on app shutdown.

### 5.8 TriggerService (`services/trigger.rs`)

`RescheduleTrigger` centralizes the mapping from mutations to reschedule mode/timing (§5.2 trigger table). Commands report a `Mutation` value via `trigger.trigger_mutation(mutation)` or use `trigger.run_guarded(op, mutation)` for the guarded pattern.

- **Pipeline lock** (`mutation_lock: Mutex<()>`): serializes mutation + reschedule pipelines across all entry points (Tauri commands, REST handlers, background timer).
- **Debounce**: configurable duration (the composition root sets `Duration::ZERO` — immediate execution). `flush_if_ready()` drains the pending batch.
- **Background timer** (`start_background_timer`): daemon thread polling every 250 ms; maintenance every 5 minutes checks past-due scheduled chunks (1-hour grace period → full reschedule) and midnight crossing (→ full reschedule).

---

## 6. Command Layer

### 6.1 Tauri Commands (`commands/`)

Each command is a `#[tauri::command]` function that extracts `State<'_, ActiveState>` (the swappable active-profile slot), resolves the current `AppState`, calls a service, and returns `Result<T, AppError>`. Profile commands take `AppHandle<R>` instead (they operate before/across profiles).

63 commands total in `generate_handler!`.

```rust
// commands/task_commands.rs
async fn create_task(active: State<'_, ActiveState>, input: CreateTaskInput) -> Result<Task, AppError>;
async fn get_task(active: State<'_, ActiveState>, id: String) -> Result<Task, AppError>;
async fn update_task(active: State<'_, ActiveState>, id: String, input: UpdateTaskInput) -> Result<Task, AppError>;
async fn delete_task(active: State<'_, ActiveState>, id: String) -> Result<(), AppError>;
async fn cancel_task(active: State<'_, ActiveState>, id: String) -> Result<Task, AppError>;
async fn complete_task(active: State<'_, ActiveState>, id: String) -> Result<Task, AppError>;
async fn list_tasks(active: State<'_, ActiveState>, filter: Option<TaskFilter>) -> Result<Vec<Task>, AppError>;
async fn list_labels(active: State<'_, ActiveState>) -> Result<Vec<LabelCount>, AppError>;
async fn get_orphaned_template_instances(active: State<'_, ActiveState>) -> Result<Vec<Task>, AppError>;

// commands/chunk_commands.rs
async fn list_chunks_for_task(active: State<'_, ActiveState>, task_id: String) -> Result<Vec<Chunk>, AppError>;
async fn list_chunks_in_range(active: State<'_, ActiveState>, start: String, end: String) -> Result<Vec<Chunk>, AppError>;
async fn get_agenda(active: State<'_, ActiveState>, start: String, end: String, label: Option<String>) -> Result<Vec<AgendaItem>, AppError>;
fn list_external_events(active: State<'_, ActiveState>, start: String, end: String) -> Result<Vec<ExternalEventRecord>, AppError>;
async fn create_fixed_chunk(active: State<'_, ActiveState>, task_id: String, start_time: String, end_time: String) -> Result<(Chunk, Task), AppError>;
async fn complete_chunk(active: State<'_, ActiveState>, chunk_id: String, duration_override: Option<i64>) -> Result<(Chunk, Task), AppError>;
async fn reopen_chunk(active: State<'_, ActiveState>, chunk_id: String) -> Result<(Chunk, Task), AppError>;
async fn move_chunk(active: State<'_, ActiveState>, chunk_id: String, new_start: String, new_end: String) -> Result<Chunk, AppError>;
async fn resize_chunk(active: State<'_, ActiveState>, chunk_id: String, new_end: String) -> Result<(Chunk, Task), AppError>;
async fn lock_chunk(active: State<'_, ActiveState>, chunk_id: String) -> Result<Chunk, AppError>;
async fn unlock_chunk(active: State<'_, ActiveState>, chunk_id: String) -> Result<Chunk, AppError>;
async fn delete_fixed_chunk(active: State<'_, ActiveState>, chunk_id: String) -> Result<Chunk, AppError>;

// commands/scheduler_commands.rs
async fn trigger_reschedule(active: State<'_, ActiveState>) -> Result<ScheduleResult, AppError>;
async fn trigger_reschedule_incremental(active: State<'_, ActiveState>, task_ids: Vec<String>) -> Result<ScheduleResult, AppError>;

// commands/recurring_commands.rs
async fn create_template(active: State<'_, ActiveState>, input: CreateTemplateInput) -> Result<RecurringTemplate, AppError>;
async fn get_template(active: State<'_, ActiveState>, id: String) -> Result<RecurringTemplate, AppError>;
async fn update_template(active: State<'_, ActiveState>, id: String, input: UpdateTemplateInput) -> Result<RecurringTemplate, AppError>;
async fn delete_template(active: State<'_, ActiveState>, id: String) -> Result<(), AppError>;
async fn list_templates(active: State<'_, ActiveState>) -> Result<Vec<RecurringTemplate>, AppError>;

// commands/schedule_commands.rs
async fn create_schedule(active: State<'_, ActiveState>, input: CreateScheduleInput) -> Result<Schedule, AppError>;
async fn get_schedule(active: State<'_, ActiveState>, id: String) -> Result<Schedule, AppError>;
async fn update_schedule(active: State<'_, ActiveState>, id: String, input: UpdateScheduleInput) -> Result<Schedule, AppError>;
async fn delete_schedule(active: State<'_, ActiveState>, id: String) -> Result<(), AppError>;
async fn list_schedules(active: State<'_, ActiveState>) -> Result<Vec<Schedule>, AppError>;

// commands/comment_commands.rs
// Sync commands (no await points); the acting author ("User") and `now` are
// injected here — comments never trigger a reschedule.
fn create_comment(active: State<'_, ActiveState>, input: CreateCommentInput) -> Result<Comment, AppError>;
fn update_comment(active: State<'_, ActiveState>, id: String, input: UpdateCommentInput) -> Result<Comment, AppError>;
fn delete_comment(active: State<'_, ActiveState>, id: String) -> Result<(), AppError>;
fn list_comments(active: State<'_, ActiveState>, task_id: String) -> Result<Vec<Comment>, AppError>;

// commands/config_commands.rs
async fn get_config(active: State<'_, ActiveState>) -> Result<AppConfig, AppError>;
async fn update_config(active: State<'_, ActiveState>, input: UpdateConfigInput) -> Result<AppConfig, AppError>;

// commands/auth_commands.rs
fn begin_google_auth(active: State<'_, ActiveState>) -> Result<String, AppError>;
fn google_auth_status(active: State<'_, ActiveState>) -> Result<AuthStatus, AppError>;
fn google_disconnect(active: State<'_, ActiveState>) -> Result<(), AppError>;
fn get_pull_calendars(active: State<'_, ActiveState>) -> Result<Vec<String>, AppError>;
fn set_pull_calendars(active: State<'_, ActiveState>, calendar_ids: Vec<String>) -> Result<(), AppError>;
async fn google_list_calendars(active: State<'_, ActiveState>) -> Result<Vec<ExternalCalendar>, AppError>;
async fn pull_external_events(active: State<'_, ActiveState>) -> Result<ScheduleResult, AppError>;
async fn sync_now(active: State<'_, ActiveState>) -> Result<SyncOutcome, AppError>;
fn get_sync_status(active: State<'_, ActiveState>) -> Result<SyncStatus, AppError>;
async fn create_user_event(active: State<'_, ActiveState>, calendar_id: String, payload: UserEventPayload) -> Result<ExternalEventRecord, AppError>;
async fn update_user_event(active: State<'_, ActiveState>, calendar_id: String, event_id: String, payload: UserEventPayload) -> Result<ExternalEventRecord, AppError>;
async fn delete_user_event(active: State<'_, ActiveState>, calendar_id: String, event_id: String) -> Result<(), AppError>;

// commands/profile_commands.rs — take AppHandle<R>, not State<ActiveState>
fn profile_status<R: Runtime>(app: AppHandle<R>) -> Result<ProfileStatusResponse, AppError>;
async fn unlock_profile<R: Runtime>(app: AppHandle<R>, id: String) -> Result<ActiveProfile, AppError>;
fn create_profile<R: Runtime>(app: AppHandle<R>, name: String) -> Result<ProfileInfo, AppError>;
fn rename_profile<R: Runtime>(app: AppHandle<R>, id: String, name: String) -> Result<ProfileInfo, AppError>;
fn delete_profile<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), AppError>;
async fn switch_profile<R: Runtime>(app: AppHandle<R>, id: String) -> Result<ActiveProfile, AppError>;

// commands/backup_commands.rs
fn get_backup_status(active: State<'_, ActiveState>) -> Result<BackupStatus, AppError>;
fn set_backup_enabled(active: State<'_, ActiveState>, enabled: bool) -> Result<BackupStatus, AppError>;
async fn backup_now(active: State<'_, ActiveState>) -> Result<BackupStatus, AppError>;
async fn export_backup_to_file(active: State<'_, ActiveState>, path: String) -> Result<(), AppError>;
async fn import_backup_from_file<R: Runtime>(app: AppHandle<R>, active: State<'_, ActiveState>, path: String) -> Result<(), AppError>;
```

### 6.2 REST API (`api/`)

Axum server on `localhost:PORT`, started in Tauri `setup` hook. Same services, different transport.

Port is configured via `APRESWORK_API_PORT` environment variable (default: `19532`). The REST API can be disabled entirely with `APRESWORK_API_ENABLED=false`.

**Security** (decision 2026-07, resolving the W6 contradiction — see REQUIREMENTS W6):

- **Bind address**: `127.0.0.1` only — never `0.0.0.0`. Prevents LAN access.
- **Host-header validation** (implemented): an axum middleware rejects any request whose
  `Host` header is not `127.0.0.1[:port]` / `localhost[:port]` (missing `Host` is rejected
  too) with `403 {"error":"forbidden"}`. This closes browser-based DNS rebinding — a
  malicious page re-resolving its own hostname to `127.0.0.1` sends same-origin fetches
  that carry the attacker's hostname in `Host`, and CORS never applies to same-origin
  requests. This is a trust-boundary check, not authentication, so it honors W6's no-auth
  stance for local processes and CLI tools.
- **No CORS layer**: not needed given the Host allowlist — cross-origin reads are already
  blocked by the browser, and rebinding (the attack CORS was meant to address here) is
  handled above.
- **No bearer token** (per W6): malicious *local* processes are explicitly out of scope.
  If agent multi-tenancy (M12.10) ever lands, revisit with a per-session 256-bit token in
  `~/.config/apreswork/api_token` (0600).
- The Svelte frontend does NOT use the REST API — it uses Tauri IPC (same process).

25 routes total, split into guarded-write routes (behind `profile_guard_middleware` — the profile-switch guard that rejects writes targeting a stale profile) and unguarded routes.

```
# Guarded writes
POST   /api/tasks                     → create_task
DELETE /api/tasks/{id}                → delete_task
PATCH  /api/tasks/{id}                → update_task
POST   /api/tasks/{id}/complete       → complete_task
POST   /api/tasks/{id}/comments       → create_comment
DELETE /api/comments/{id}             → delete_comment
PATCH  /api/comments/{id}             → update_comment
POST   /api/chunks/{id}/move          → move_chunk

# Unguarded
GET    /api/health                    → health (always 200; liveness probe)
GET    /api/profile                   → get_profile (active profile info)
POST   /api/profile/switch            → switch_profile
GET    /api/profiles                  → list_profiles
GET    /api/tasks                     → list_tasks (query params for filter)
GET    /api/tasks/{id}                → get_task
GET    /api/agenda                    → get_agenda (?start=&end=&label=)
GET    /api/labels                    → list_labels (distinct labels + task counts)
GET    /api/tasks/{id}/comments       → list_comments
POST   /api/auth/google/begin         → begin_google_auth
GET    /api/auth/google/status        → google_auth_status
POST   /api/auth/google/disconnect    → google_disconnect (local wipe)
POST   /api/calendar/pull             → pull_external_events (refresh mirror + full reschedule)
POST   /api/sync/now                  → sync_now (pull + reschedule + push chunks)
GET    /api/sync/status               → get_sync_status
POST   /api/backup/now                → backup_now
GET    /api/backup/status             → get_backup_status
```

The REST API is a subset of the Tauri command surface: chunk CRUD (create/complete/reopen/resize/lock/unlock/delete), scheduler commands, recurring template CRUD, schedule CRUD, config CRUD, user-event CRUD, list_chunks_in_range, list_external_events, and export/import are Tauri-only. The REST API exists primarily for CLI scripting (`scripts/api.sh`) and external automation.

### 6.3 Chat-Facing Surface

> **Not implemented** — the chat-facing surface (REQUIREMENTS.md M10.3) has no code in the tree and its mechanism is undecided: an MCP server over the REST API, a CLI, or an in-app chat. Agents drive the REST API through `scripts/api.sh` today.

---

## 7. Composition Root (`lib.rs` + `state.rs`)

### SQLite Concurrency

`db::sqlite::SqliteStore` wraps a single `Mutex<Connection>`. All access is serialized through the mutex. SQLite is opened in **WAL mode** (`PRAGMA journal_mode=WAL`) which allows concurrent readers with a single writer — but since this is a local single-user desktop app, the mutex alone is sufficient and WAL is a safety net.

This is deliberately simple. The `Store` trait abstraction means a pooled or async implementation can be swapped in later if needed (e.g., `r2d2` connection pool for higher concurrency), without changing any service or command code.

### Pipeline Mutation Lock

Above the connection mutex sits a coarser lock: `RescheduleTrigger.mutation_lock: Mutex<()>`. It serializes mutation + reschedule pipelines across all entry points (Tauri commands, REST handlers, the background timer), so a reschedule's read→compute→apply sequence never interleaves with a concurrent mutation's multi-call sequence.

- Every **mutating** command/handler holds `trigger.mutation_guard()` around its service call only, and drops the guard **before** calling `trigger()` / `flush_if_ready()` — those re-acquire the lock internally around the actual reschedule execution (`RescheduleTrigger::execute`), and the lock is non-reentrant.
- **Read-only** commands take no guard.
- Lock order everywhere: pipeline lock → connection lock. Multi-statement mutations additionally run inside `Store::with_tx` (§8.3) so crash/error atomicity holds even within a serialized pipeline.

```rust
// state.rs
pub struct AppState {
    pub store: Arc<dyn Store + Send + Sync>,
    pub scheduler: Arc<dyn Scheduler>,
    pub trigger: Arc<RescheduleTrigger>,
    pub calendar_sync: Arc<dyn CalendarSync>,
    pub backup: Arc<dyn BackupTarget>,
    pub profile_dir: PathBuf,
    pub restore_notice: Option<String>,
    pub profile: ActiveProfile,
}

/// Process-scoped handle to the currently active profile's AppState.
/// Commands, the REST server, and background timers resolve the active
/// state per call. In-process profile switching replaces the slot and
/// every subsequent resolution sees the new profile.
pub struct ActiveState {
    inner: Arc<RwLock<Option<Arc<AppState>>>>,
    activation: Arc<Mutex<()>>,
}
```

`ActiveState` is managed by Tauri once at startup (before any profile is unlocked). It provides:
- `get() → Result<Arc<AppState>>` — resolves the slot or returns a validation error.
- `swap(next) → Option<Arc<AppState>>` — replaces the slot (profile switch).
- `activation_guard()` — serializes activate/switch sequences.
- `From<Arc<AppState>>` — prefills the slot (tests, REST router builder).

```rust
// lib.rs (simplified)
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![/* 63 commands */])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let config_dir = app.path().app_config_dir()?;

            // Profile registry: load or adopt pre-profiles data on first run.
            let registry = profiles::adoption::load_or_adopt(&data_dir, &config_dir, now)?;
            app.manage(Arc::new(ProfilesState::new(data_dir.clone(), registry)));

            // Active-profile slot: the REST server and background timers resolve it per call.
            let active = ActiveState::new();
            app.manage(active.clone());

            // Background timers resolve the slot per tick — they survive profile switches.
            trigger::start_background_timer(/* closure resolving active.get_opt() */);
            backup::start_backup_timer(/* closure resolving active.get_opt() */);

            // REST server (§6.2)
            tauri::async_runtime::spawn(http_server::start_server(active, profiles_state, config));

            // Auto-open last-used profile (fast path); on failure the frontend
            // falls back to the profile gate.
            if let Some(entry) = registry.startup_profile() {
                profiles::activate::activate_profile(app.handle(), &data_dir, &entry, now)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .run(|app_handle, event| {
            // ExitRequested → graceful-exit backup (bounded final export)
        });
}
```

---

## 8. SQLite Schema

### 8.1 Migration Strategy

Hand-rolled migration runner. `schema_version` table tracks current version. Each migration is a Rust function compiled into the binary.

```rust
// db/migrations.rs
type Migration = fn(&Connection) -> Result<(), rusqlite::Error>;
pub const MIGRATIONS: &[Migration] = &[super::migration_001::migrate];

pub fn run_migrations(conn: &Connection) -> Result<()> {
    // Create schema_version if not exists
    // Check current version
    // Apply pending migrations sequentially
}
```

### 8.2 Full Schema (Migration 001)

The pre-release chain 001–008 was squashed into a single full-schema
`migration_001` before v0, so a fully migrated database sits at `schema_version`
1. The block below is the exact net schema that migration produces; databases
that had run the old chain were flipped to version 1 by hand at the squash
(their schema is byte-for-byte identical, verified structurally against the
chain before it was deleted).

```sql
CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Seeds: planning_horizon_days=30, timezone=UTC, max_continuous_minutes=120,
--        min_break_minutes=5, last_reschedule='', last_mutation='', last_sync='',
--        last_busy_sync='', sync_provider='google', sync_debounce_minutes='2',
--        sync_poll_minutes='60', last_sync_error='', pull_calendar_ids=''

CREATE TABLE schedules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE schedule_windows (
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL REFERENCES schedules(id) ON DELETE CASCADE,
    day_of_week INTEGER NOT NULL,          -- 0=Monday .. 6=Sunday (ISO 8601)
    start_time TEXT NOT NULL,              -- "HH:MM"
    end_time TEXT NOT NULL                 -- "HH:MM"
);
CREATE INDEX idx_schedule_windows_schedule ON schedule_windows(schedule_id);
-- Seed: default schedule with weekday 07-09, 18-23 and weekend 08-22

CREATE TABLE recurring_templates (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    duration_minutes INTEGER NOT NULL,
    priority INTEGER NOT NULL DEFAULT 1,
    schedule_id TEXT NOT NULL REFERENCES schedules(id),
    cadence_type TEXT NOT NULL,            -- 'weekly' or 'monthly'
    cadence_data TEXT NOT NULL,            -- JSON
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    start_date TEXT                        -- reconcile anchor; coalesces to created_at
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    duration_minutes INTEGER NOT NULL,
    time_logged_minutes INTEGER NOT NULL DEFAULT 0,
    priority INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'pending',
    start_date TEXT,
    deadline TEXT,
    schedule_id TEXT NOT NULL REFERENCES schedules(id),
    min_chunk_minutes INTEGER NOT NULL DEFAULT 30,
    no_split INTEGER NOT NULL DEFAULT 0,
    recurring_template_id TEXT REFERENCES recurring_templates(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    expire_at TEXT,                        -- NULL = never auto-expires
    is_pinned INTEGER NOT NULL DEFAULT 0   -- user-placed; reconcile leaves it alone
);
-- No cadence_period_key: recurring instances are matched by id/order in
-- single-pass reconciliation, not by an occurrence-date unique key.
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_deadline ON tasks(deadline);
CREATE INDEX idx_tasks_priority ON tasks(priority);
CREATE INDEX idx_tasks_recurring ON tasks(recurring_template_id);
CREATE INDEX idx_tasks_schedule ON tasks(schedule_id);

CREATE TABLE task_labels (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    PRIMARY KEY (task_id, label)
);
CREATE INDEX idx_task_labels_label ON task_labels(label);

CREATE TABLE template_labels (
    template_id TEXT NOT NULL REFERENCES recurring_templates(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    PRIMARY KEY (template_id, label)
);

CREATE TABLE chunks (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'scheduled',
    is_fixed INTEGER NOT NULL DEFAULT 0,
    logged_minutes INTEGER,
    completed_at TEXT,
    google_event_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_chunks_task ON chunks(task_id);
CREATE INDEX idx_chunks_time ON chunks(start_time, end_time);
CREATE INDEX idx_chunks_status ON chunks(status);
CREATE INDEX idx_chunks_google ON chunks(google_event_id);

-- Superseded by the external_events mirror (Decision 5). No code reads or writes
-- this table anymore; kept for schema parity with pre-squash databases. Dropping
-- it rides a future cleanup migration.
CREATE TABLE external_busy_times (
    id TEXT PRIMARY KEY,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'google'
);
CREATE INDEX idx_busy_times_range ON external_busy_times(start_time, end_time);

-- Google auth tokens are NOT stored in the database.
-- The refresh token is stored in the OS keyring (Secret Service on Linux,
-- Credential Manager on Windows), keyed by (service="com.apreswork.app",
-- username="google-oauth:<profile-id>"). The access token is held in memory
-- only and never written to disk. Only non-secret metadata is stored here
-- (calendar_id, connected_at).
CREATE TABLE google_auth (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    calendar_id TEXT,
    connected_at TEXT
);

-- Persisted mirror of Google Calendar events (pull-sync pipeline).
-- (calendar_id, event_id) is the natural key; row id is preserved on upsert.
-- all_day: primary-calendar events are editable in-app (write-through to Google);
-- all-day events must round-trip as all-day, so the mirror records the flag. When
-- set, start_utc/end_utc are the Local-midnight day boundaries (end exclusive),
-- mirroring Google's all-day `date` convention.
CREATE TABLE external_events (
    id TEXT PRIMARY KEY,
    calendar_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    start_utc TEXT NOT NULL,
    end_utc TEXT NOT NULL,
    busy INTEGER NOT NULL DEFAULT 1,
    declined INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    all_day INTEGER NOT NULL DEFAULT 0,
    UNIQUE(calendar_id, event_id)
);
CREATE INDEX idx_external_events_range ON external_events(start_utc, end_utc);

-- Tracks the last-synced state for chunks that have been pushed to Google Calendar.
-- Cascade-deleted when the chunk is deleted.
CREATE TABLE chunk_sync_state (
    chunk_id           TEXT PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    event_id           TEXT NOT NULL,
    etag               TEXT,
    synced_start       TEXT NOT NULL,
    synced_end         TEXT NOT NULL,
    synced_title       TEXT NOT NULL,
    synced_description TEXT NOT NULL DEFAULT '',
    updated_at         TEXT NOT NULL
);
CREATE INDEX idx_chunk_sync_event ON chunk_sync_state(event_id);

CREATE TABLE comments (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author TEXT NOT NULL,                    -- "SYSTEM" for auto-generated, "User" for human (until auth)
    content TEXT NOT NULL,                   -- Markdown
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_comments_task ON comments(task_id, created_at);
```

### 8.3 Schema Decisions

- **Singleton tables** — Tables that must contain exactly one row (e.g. `google_auth`, `schema_version`) use the `CHECK(id = 1)` pattern to enforce the invariant at the database level. Never rely on application-level enforcement alone for single-row tables.
- **Labels as join table** — enables efficient `WHERE label IN (...)` filtering. Denormalized into `Vec<String>` on model read.
- **Cadence as JSON** — avoids polymorphic table structures. `cadence_type` column enables queries without JSON parsing.
- **Status as TEXT** — readability in raw queries. Negligible performance impact at this data volume.
- **`ON DELETE SET NULL`** for `recurring_template_id` — preserves completed/cancelled instances when template is deleted (M4.2b).
- **Google auth as singleton** — `CHECK (id = 1)` ensures one row. Only non-secret metadata (calendar_id) stored in DB. The refresh token is stored in the OS keyring (Secret Service / kwallet on Linux, Credential Manager on Windows) under the key `(service="com.apreswork.app", username="google-oauth:<profile-id>")`. The access token is memory-only: cached in `Arc<Mutex<Option<CachedToken>>>` inside `GoogleCalendarSync`, never written to disk by production code. On disconnect, the keyring entry is deleted and the in-memory cache is cleared.
- **All times as ISO 8601 UTC TEXT** — consistent, human-readable, sortable.
- **Transaction scope** — All multi-step mutations must be wrapped in explicit transactions, including initialization/bootstrapping sequences (e.g. migration runner setup), not just runtime data mutations. When using `rusqlite::Transaction`, pass `&tx` (not the original `conn`) to all operations within the transaction scope so transactional intent is visually explicit.
- **Cross-method atomicity via `Store::with_tx`** — services wrap multi-call mutation sequences in `store.with_tx(|tx| { ... })`. `SqliteStore` implements it by locking the connection mutex, opening one transaction, and handing the closure a transaction-scoped `&dyn Store` view (`TxStore`); `Ok` commits, `Err` rolls back. Nested `with_tx` calls join the enclosing transaction. The connection mutex is non-reentrant: inside a `with_tx` closure, only use the store the closure receives — calling methods on the outer store self-deadlocks.

---

## 9. Svelte Frontend Architecture

### 9.1 Structure

```
src/
  main.ts                        # Mount App
  App.svelte                     # Keyed Shell (remounts all views on profile switch); profile gate as fallback
  app.css                        # Design system (CSS custom properties)

  lib/
    router.svelte.ts             # Hash-based router ($state rune)
    api.ts                       # Typed Tauri invoke wrappers
    types.ts                     # TypeScript interfaces (mirrors Rust DTOs)
    utils.ts                     # Date/duration formatting + week-boundary helpers (WeekStart type)
    markdown.ts                  # Markdown → sanitized HTML (the one render+sanitize policy; strict allowlist)
    weekStartPref.ts             # Persisted Mon/Sun week-start preference, validated localStorage load
    quickDateAnchorPref.ts       # Persisted quick-date week-end anchor (auto/fri/sat/sun), validated localStorage load
    shortcuts.svelte.ts          # Keyboard shortcut registry + dispatcher; views register bindings on mount, unregister on unmount
    app-clock.ts                 # Composition-root clock binding: exports appClock: () => Date (reads the wall clock; feeds the getNow prop chain)

    stores/
      tasks.svelte.ts            # Task list reactive state
      templates.svelte.ts        # Recurring template list state
      schedules.svelte.ts        # Schedule state
      calendarFocus.svelte.ts    # Calendar view focus date + mode (week/day)
      warnings.svelte.ts         # Scheduling warnings
      toast.svelte.ts            # Toast notification queue (error/success messages, auto-dismiss)
      profile.svelte.ts          # Profile status + in-place switch (resets profile-scoped stores, gate fallback on failure)

    actions/
      taskActions.ts             # Shared verb layer: each task/chunk verb defined once (api call + confirm rule + refetch + toast); all surfaces (context menus, kebabs, dropdowns) render these verbs
      confirmHost.svelte.ts      # ConfirmHost: per-surface confirm-dialog request/resolve state; its request fn is the TaskActionsHost.confirm callback, rendered by shared/ConfirmHostDialog
      rescheduleTrigger.ts       # runReschedule — the one definition of "run a reschedule and report" (warnings store + toast); RescheduleApiSubset DI seam; used by CalendarView and TaskListView
      syncTrigger.ts             # runSync — the one definition of the on-demand sync call and its result toast; SyncApiSubset DI seam; used by CalendarView and SettingsView
      suppressContextMenu.ts     # shouldSuppressContextMenu: main.ts blocks the native context menu except inside inputs, textareas and contenteditable

    components/
      layout/
        Shell.svelte             # Sidebar + main content area; attaches the ONE window keydown listener; registers Global shortcuts; hosts the status warnings modal (embedded StatusView)
        shellShared.ts           # Injectable ShellApi interface + defaultShellApi (getBackupStatus)
        Sidebar.svelte           # Nav links + warning-count badge (amber/red; its own button — opens the status modal) + profile switcher pinned at the bottom
        ShortcutOverlay.svelte   # Keyboard shortcut help overlay (Modal-based, groups by group field)

      status/
        StatusView.svelte        # Warning list with quick-resolution actions; page at #/status and embedded in the shell's status modal
        statusViewShared.ts      # Injectable StatusViewApi interface + defaultStatusViewApi (triggerReschedule, getTask, updateTask, completeTask, cancelTask, listChunksForTask, createFixedChunk, apiErrorMessage)
        ResolutionDropdown.svelte # Per-warning actions: dated deadline presets, custom-deadline calendar submenu, do now, complete, cancel task

      calendar/
        CalendarView.svelte      # Mode toggle, date nav, container, "Refresh Calendar" button (tooltip: last_busy_sync)
        WeekView.svelte          # 7-column time grid
        DayView.svelte           # Single-column time grid
        TimeGrid.svelte          # Hour lines, current time indicator
        ChunkBlock.svelte        # Chunk rendering (status color, resize handles, conflict indicator, right-click/kebab context menu)
        ChunkCreateLayer.svelte  # Empty-slot create gesture for one day column: crosshair hit area + drag selection (shared by Day/Week views)
        calendarViewShared.ts    # Shared Day/Week view prop contract + schedule-window scroll helpers
        DragOverlay.svelte       # Ghost element during drag operations
        dragState.svelte.ts      # Pointer drag/resize/create state; SNAP_MINUTES = 5 snap grid, DRAG_THRESHOLD_PX
        calendarLayout.ts        # The one pixel scale of the time grid (HOUR_HEIGHT_PX, block style + height helpers)
        overlapLayout.ts         # Overlap-lane layout for a day column (chunks and external events)
        weekEdgeFlip.ts          # Edge-dwell week flip while dragging a chunk across week boundaries
        ScheduleWindowOverlay.svelte # Shades a date's schedule windows behind the grid
        PastWash.svelte          # Wash over the elapsed part of a day column (height from calendarLayout)
        ExternalEventBlock.svelte    # Rendering of one pulled external (Google Calendar) event
        DayExternalsLayer.svelte     # Timed external events of one day column, laid out in overlap lanes
        externalEventInteractivity.ts # resolveEventOpenHandler — the one primary-calendar editability policy for external events
        EventDialog.svelte       # Create/edit dialog for user-owned Google Calendar events (timed or all-day)
        eventDialogShared.ts     # Pure all-day helpers (exclusive end date ↔ inclusive display date)
        GoogleReconnectHint.svelte   # role="status" hint with a Reconnect action when Google access is lost
        CompleteChunkDialog.svelte   # "Complete this chunk or the whole task?" modal (CompletionTarget)
        completionFlow.svelte.ts # Completion flow state behind CompleteChunkDialog; CompletionFlowApi seam (listChunksForTask, completeChunk, completeTask, reopenChunk)

      tasks/
        TaskListView.svelte      # Filters, sort controls, list; accepts apiClient?: TaskListViewApi, taskStore?: TaskState, templateStore?: TemplateState
        taskListViewShared.ts    # Injectable TaskListViewApi (RescheduleApiSubset & TaskActionsApiSubset) + defaultTaskListViewApi
        FilterDropdown.svelte    # Generic multi-select filter (summary button + checkbox popover); status + priority instances
        taskSort.ts              # Sort comparators + multi-key sort stack (sortTasks, clickSortField)
        taskListPrefs.ts         # Persisted view prefs (status/priority filters + sort stack), validated localStorage load
        labelFilter.ts           # Tri-state label chip policy (include/exclude transitions, clickLabelChip)
        TaskRow.svelte           # Single task row
        TaskDetail.svelte        # Slide-out detail panel (chunks + comments); accepts apiClient?: TaskDetailApi, taskStore?: TaskState
        taskDetailShared.ts      # Injectable TaskDetailApi interface + defaultTaskDetailApi (completeTask, cancelTask, listChunksForTask, listComments, createComment, updateComment, deleteComment)
        TaskForm.svelte          # Create/edit form (with recurring toggle)
        taskFormShared.ts        # Injectable TaskFormApi interface + defaultTaskFormApi (listComments, createComment, updateComment, deleteComment, listChunksForTask, unlockChunk, deleteFixedChunk)
        RecurringSection.svelte  # Cadence config sub-form
        RecurringListView.svelte # Recurring templates list + edit form; the canonical store-as-prop seam (templateStore?: TemplateState)
        SharedFormFields.svelte  # Title/description/duration/priority/schedule/labels fields (SharedFieldValues) shared by TaskForm and RecurringListView
        CommentSection.svelte    # Comment list + add/edit input (reverse chronological)
        ChunkList.svelte         # Chunk rows with a trailing per-row snippet; shared by TaskDetail and TaskForm
        chunkListLoad.ts         # Shared chunk-fetch helper (loadChunkList) used by TaskDetail and TaskForm; injectable fetchChunks for DI

      profile/
        ProfileGate.svelte       # Fallback profile unlock/create screen (startup auto-opens the last-used profile)
        ProfileSwitcher.svelte   # Sidebar dropdown: switch profiles in place (no confirm) + "Manage profiles…" link
        ProfilesView.svelte      # Profiles page ('profiles' route): create/rename/switch + red danger-zone delete card
        CreateProfileForm.svelte # Reusable create-profile form (name input + error + submit/cancel); used by ProfileGate and ProfilesView

      settings/
        SettingsView.svelte      # Google Calendar: connect/status/disconnect, pull-calendar picker, manual pull; hosts the sections below
        settingsViewShared.ts    # Injectable SettingsViewApi interface + defaultSettingsViewApi (googleAuthStatus, beginGoogleAuth, openExternalUrl, googleListCalendars, getPullCalendars, setPullCalendars, googleDisconnect, getSyncStatus, syncNow, syncErrorMessage)
        SchedulingSection.svelte # Scheduling config card (horizon, breaks, timezone; dirty-gated save) + quick-date anchor preference
        schedulingSectionShared.ts # Injectable SchedulingSectionApi interface + defaultSchedulingSectionApi (getConfig, updateConfig, apiErrorMessage)
        BackupSection.svelte     # Drive auto-backup toggle, back up now, export/import zip
        backupSectionShared.ts   # Injectable BackupSectionApi + BackupSectionDialog interfaces + defaults (getBackupStatus, setBackupEnabled, backupNow, exportBackupToFile, importBackupFromFile, apiErrorMessage, backupErrorMessage)

      shared/
        Modal.svelte
        ConfirmDialog.svelte
        ConfirmHostDialog.svelte        # Renders a ConfirmHost's pending request as a ConfirmDialog
        RescheduleButton.svelte         # Shared "Reschedule" button with busy state; used by CalendarView and TaskListView
        focusTrap.ts                    # Focus-trap helpers (focusFirst, getFocusableElements, handleTabTrap) for Modal and ContextMenu
        viewportReposition.svelte.ts    # repositionOnViewportChange: re-run popover positioning on viewport resize/scroll (DateTimePicker, TimeMenu)
        deadlinePresets.ts              # Deadline preset helpers (todayDeadline, tomorrowDeadline, customDeadlineIso)
        ContextMenu.svelte              # Positioned popover menu (focus-trapped, Esc/blur closes, role="menu"); items may open a hover/click submenu panel
        Toast.svelte                    # Error/success notification (auto-dismiss, stackable)
        PriorityBadge.svelte
        LabelChip.svelte
        StatusBadge.svelte
        DurationInput.svelte
        DateTimePicker.svelte           # Date+time picker popover (imports shared helpers + TimeMenu)
        MiniCalendar.svelte             # Bare month grid (no time, no footer) — emits the picked local date; used by the custom-deadline submenu
        TimeMenu.svelte                 # Time dropdown extracted from DateTimePicker (trigger + listbox)
        dateTimePickerShared.ts         # Pure date/format/build helpers + picker types/constants
        popoverPosition.ts              # Pure helper: computePositioningStyle builds the top/left/width/max-height inline-style string for fixed popovers
        MarkdownView.svelte             # Renders markdown.ts output (the only {@html}); links open via system browser
```

Test support is omitted above: `lib/testSetup.ts`, `lib/testFixtures.ts`, `lib/storageDoubles.ts`, `lib/storageStubHooks.ts`, the per-folder `testFixtures.ts` / `*TestSupport.ts` / `*.testHelpers.ts` / `profileFixtures.ts` modules and `shared/viewportRepositionFixture.svelte`. Every `*.test.ts` sits beside its subject.

### 9.2 Key Decisions

- **Routing**: Hash-based (`#/calendar`, `#/tasks`, `#/status`, `#/settings`, `#/profiles` — profiles is reached via the sidebar profile switcher, not a nav item). Reactive via `$state` rune. No SvelteKit dependency. Empty/invalid hash defaults to `'calendar'`.
- **State management**: Svelte 5 runes only — reactive classes with `$state` and `$derived`. No legacy stores.
- **Backend communication**: Tauri `invoke` for frontend ↔ backend (same process, zero HTTP overhead). REST API is for external clients only.
- **Calendar drag-and-drop**: Native pointer events (`pointerdown`/`pointermove`/`pointerup`) with `$state`-based drag tracking. 5-minute snap grid (M8.6). Implementation notes:
    - Use `element.setPointerCapture(e.pointerId)` on `pointerdown` to maintain tracking when the pointer leaves the element boundary during fast drags.
    - Move vs. resize disambiguation: resize activates from a bottom-edge handle zone (~8px); move activates from the chunk body.
    - Auto-scroll when dragging near the top/bottom edges of the scrollable time grid.
- **CSS**: Svelte scoped `<style>` blocks per component + CSS custom properties in `app.css` for theming (colors, spacing, typography). Custom property naming: `--color-*`, `--spacing-*`, `--font-*`. A dark theme is **not implemented**: `app.css` only reserves the `[data-theme="dark"]` override hook for Phase 6.
- **Error display**: `Toast.svelte` component for transient error/success notifications. Stores and views catch invoke errors and push messages to the toast queue. Every toast auto-dismisses after the default delay; `ToastState.push` accepts `autoMs = 0` for a persistent toast, but no caller uses it yet, so the intended persistent database-error toast is **not implemented**.
- **Types**: TypeScript interfaces mirror Rust DTOs exactly for type-safe IPC.

### 9.2a Serialization Contract (Rust ↔ TypeScript)

All data crosses the Tauri IPC boundary as JSON. Conventions:

| Rust type                          | JSON representation                                                                                                          | TypeScript type          |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| `DateTime<Utc>`                    | `"2026-03-15T18:00:00Z"` (ISO 8601, always UTC)                                                                              | `string`                 |
| `Option<T>`                        | value or `null` (use `#[serde(default)]` on deserialization)                                                                 | `T \| null`              |
| `Option<Option<T>>` (patch fields) | absent = don't change, `null` = clear                                                                                        | `T \| null \| undefined` |
| `chrono::Weekday`                  | `"Mon"`, `"Tue"`, ..., `"Sun"`                                                                                               | `string` (union type)    |
| `NaiveTime`                        | `"18:00:00"` (HH:MM:SS)                                                                                                      | `string`                 |
| `Priority`                         | `"Low"`, `"Medium"`, `"High"`, `"Critical"`                                                                                  | `string` (union type)    |
| `TaskStatus`                       | `"backlog"`, `"pending"`, `"scheduled"`, `"completed"`, `"cancelled"` (`#[serde(rename_all = "lowercase")]`)                 | `string` (union type)    |
| `ChunkStatus`                      | `"scheduled"`, `"completed"` (`#[serde(rename_all = "lowercase")]`)                                                          | `string` (union type)    |
| `Cadence`                          | `{"period":"Weekly","interval":1,"windows":[{"start":0,"end":6}]}` (struct, not tagged enum)                                 | object                   |
| `AuthStatus`                       | Internally tagged: `{"type":"not_connected"}`, `{"type":"pending"}`, `{"type":"connected","email":"..."}` etc.               | discriminated union      |

`Priority` uses default PascalCase serde. `TaskStatus` and `ChunkStatus` use `#[serde(rename_all = "lowercase")]`. `Cadence` serializes as a plain struct (§3.3). The frontend works with ISO date strings throughout — no conversion to `Date` objects at the API boundary (avoids timezone pitfalls).

### 9.3 Router (`router.svelte.ts`)

```typescript
type Route = 'calendar' | 'tasks' | 'status' | 'settings' | 'profiles';

// Module-level exported function (not a class method) — used by tests and the class.
export function parseHash(hash: string): Route { /* ... */ }

class Router {
    current: Route = $state(parseHash(window.location.hash));

    navigate(route: Route) {
        window.location.hash = `#/${route}`;
    }
    destroy() { /* removes hashchange listener */ }
}

export const router = new Router();
```

### 9.4 State Management Pattern (`stores/tasks.svelte.ts`)

```typescript
// DI: the constructor takes a client interface, not the concrete api module.
export interface TasksClient {
    listTasks: (filter?: TaskFilter) => Promise<Task[]>;
    createTask: (input: CreateTaskInput) => Promise<Task>;
    updateTask: (id: string, input: UpdateTaskInput) => Promise<Task>;
    deleteTask: (id: string) => Promise<void>;
}

export class TaskState {
    items: Task[] = $state([]);
    loading: boolean = $state(false);
    selectedId: string | null = $state(null);
    filter: TaskFilter = $state({});

    selected: Task | undefined = $derived.by(() =>
        this.items.find((t) => t.id === this.selectedId),
    );

    readonly #client: TasksClient;

    // Defaults to the api module; tests pass a vi.fn()-based client.
    constructor(client: TasksClient = defaultClient) {
        this.#client = client;
    }

    async load() {
        this.loading = true;
        try {
            this.items = await this.#client.listTasks(this.filter);
        } catch (e) {
            toastState.error(api.apiErrorMessage(e, 'Failed to load tasks'));
        } finally {
            this.loading = false;
        }
    }
}

// Production instance; tests construct their own with a stub client.
export const taskState = new TaskState();
```

Other stores (`ScheduleState`, `WarningsState`, `TemplateState`, `CalendarFocusState`, `ProfileState`, `ToastState`) follow the same pattern: `$state` fields, `$derived` getters, DI constructor where the store calls the backend.

### 9.5 API Layer (`api.ts`)

About 60 typed per-command wrapper functions plus the shared error-message helpers (`apiErrorMessage`, `syncErrorMessage`, `backupErrorMessage`). Each wrapper names one Tauri command, types its arguments and return value, and is the sole call site for that command name. Nothing else may import `invoke` from `@tauri-apps/api/core`.

```typescript
import { invoke } from '@tauri-apps/api/core';

export async function createTask(input: CreateTaskInput): Promise<Task> {
    return invoke<Task>('create_task', { input });
}
export async function getTask(id: string): Promise<Task> {
    return invoke<Task>('get_task', { id });
}
// ... one wrapper per remaining command
```

The contract (command name, camelCase argument keys mapping to snake_case Rust parameters) is verified statically by `scripts/check_invoke_contract.py` — no runtime test covers it.

---

## 10. Error Handling

Single `AppError` enum across the entire backend.

```rust
#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("Not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Calendar sync error: {0}")]
    CalendarSync(String),
    #[error("Backup error: {0}")]
    Backup(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Profile mismatch: {0}")]
    ProfileMismatch(String),
}

// serde::Serialize impl for Tauri IPC
// From<rusqlite::Error> impl for DB errors

// Axum IntoResponse impl for REST API:
//   NotFound         → 404
//   Validation       → 400
//   Database         → 500
//   CalendarSync     → 500
//   Backup           → 500
//   Internal         → 500
//   ProfileMismatch  → 409
// Response body: { "error": "<variant>", "message": "<display string>" }

// SECURITY: Error messages must NEVER include OAuth tokens, refresh tokens,
// HTTP Authorization headers, or other credentials. CalendarSync errors should
// only contain: HTTP status codes, error descriptions from the API response,
// and endpoint URLs (without query parameters that may contain tokens).
// Log output must also sanitize Authorization headers before writing.
```

---

## 11. Plugin Extension Points

Each pluggable component: **trait → default implementation → swap at composition root**.

| Extension Point      | Trait              | Default               | How to swap                                             |
| -------------------- | ------------------ | --------------------- | ------------------------------------------------------- |
| Storage backend      | `dyn Store`        | `SqliteStore`         | Replace `Arc::new(SqliteStore::new(...))` in `lib.rs`   |
| Scheduling algorithm | `dyn Scheduler`    | `DefaultScheduler`    | Replace `Arc::new(DefaultScheduler::new())` in `lib.rs` |
| Calendar integration | `dyn CalendarSync` | `NoopCalendarSync`    | Replace in `lib.rs` (e.g., `GoogleCalendarSync`)        |
| Backup storage       | `dyn BackupTarget` | `NoopBackupTarget`    | Replace in `lib.rs` (e.g., `GoogleDriveBackup`)         |

No dynamic plugin loading. Compile-time selection via trait objects behind `Arc`. This is the right level of extensibility for a desktop app.

**What makes it practical**: Traits deal only with domain types. No implementation-specific types leak. Tests run the real `SqliteStore` on an in-memory database, so no separate test backend exists or is needed. Alternative schedulers (`SpreadScheduler`, `CompactScheduler`) can be added without modifying existing code.

---

## 12. Implementation Phases

### Phase 1: Core Data Model + Storage + Basic CRUD

- Domain models, enums, validation
- SQLite schema (migration 001), migration runner
- Store traits + SqliteStore implementation
- TaskService, RecurringService (CRUD only)
- Tauri commands for task/template/schedule CRUD
- AppState setup in lib.rs
- **New deps**: `chrono`, `uuid`, `thiserror`, `serde` + `serde_json` (serialization for Tauri IPC and JSON columns)
- **Test**: Unit tests for validation, integration tests for SqliteStore (`:memory:`)
- **Deliverable**: Backend creates/reads/updates/deletes tasks, templates, schedules

### Phase 2: Scheduling Engine

- Scheduler trait + DefaultScheduler (priority-based greedy)
- Slot finder (expand schedule windows → concrete slots)
- Recurring instance generation with deadline derivation (M4.2)
- Auto-cancel overdue instances (M4.5)
- SchedulingService orchestration
- Auto-reschedule check on startup (M3.5a)
- **New deps**: `chrono-tz` (DST-aware timezone conversion for slot finder)
- **Test**: Unit tests for slot finder, scheduler algorithm, recurring generation
- **Deliverable**: `trigger_reschedule` produces correctly placed chunks with warnings

### Phase 3: UI — Calendar + Task List

- Router, Shell layout, Sidebar
- TaskListView (sort/filter/detail panel/create form with recurring toggle)
- CalendarView (week/day modes, time grid)
- ChunkBlock (status colors, lock icon, resize handles)
- Drag-and-drop: move, resize, create from empty slot
- Chunk completion, reopen
- Status view with warning resolution actions (extend deadline, do now, complete, cancel)
- **Test**: Manual testing of all interactions
- **Deliverable**: Fully interactive desktop app

### Phase 4: Google Calendar Integration

- CalendarSync trait + NoopCalendarSync + GoogleCalendarSync
- OAuth2 flow, token storage, API v3 calls
- SyncService orchestration
- Auth UI in settings
- **New deps**: `reqwest`, `oauth2`
- **Deliverable**: Chunks sync to dedicated Google Calendar

### Phase 3A: Minimal REST API + Agent Loop (accelerated from Phase 5)

- Axum server setup (localhost only, Host-header validation — see §6.2 security)

> **Not implemented** — bearer-token auth. The API is unauthenticated; access is restricted to the loopback interface with Host-header validation (§6.2). Malicious local processes are explicitly out of scope (REQUIREMENTS W6).

- Minimal routes, then expanded to the current 25 (§6.2)
- AppError → Axum IntoResponse mapping
- Start server in Tauri setup hook
- **New deps**: `axum`, `tower`
- **Deliverable**: Agent loop works against live app DB via `scripts/api.sh`

### Phase 5: REST API expansion + chat-facing surface

- Remaining REST routes (chunks, schedules, recurring, config, etc.)

> **Not implemented** — the chat-facing surface (§6.3). Mechanism undecided; agents use `scripts/api.sh` today.

- **Deliverable**: Full REST API; chat-facing surface still open

### Phase 6: Export/Backup + Settings + Polish

- Zip export of SQLite database
- Settings page (timezone, planning horizon, export button)
- Keyboard shortcuts (done); dark/light mode (**not implemented**, see §9.2)
- **New deps**: `zip`
- **Deliverable**: Data safety, polished UX

---

## 13. Verification Strategy

### Per-phase

- **Phase 1**: `cargo test` — validation unit tests, SqliteStore integration tests (in-memory DB)
- **Phase 2**: `cargo test` — scheduler algorithm tests, recurring instance generation, slot finder
- **Phase 3**: `npx tauri dev` — manual UI testing
- **Phase 4**: Mock Google Calendar API; manual test with real account
- **Phase 5**: `curl` against REST endpoints; `scripts/api.sh` integration
- **Phase 6**: Export/import round-trip

### End-to-end scenario

1. Create tasks with various priorities, deadlines, durations
2. Trigger reschedule → verify chunks placed correctly
3. Drag-move a chunk → verify it becomes fixed
4. Complete a chunk → verify time logged, remainder re-queued
5. Create recurring template → verify instances with correct deadlines
6. Edit template → verify future open unpinned instances are regenerated while pinned and closed ones survive
