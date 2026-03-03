// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * TypeScript interfaces mirroring Rust domain types.
 *
 * Serialization contract (DESIGN.md §9.2a):
 * - DateTime<Utc>       → ISO 8601 string (always UTC)
 * - Option<T>           → T | null
 * - Option<Option<T>>   → T | null | undefined (patch: absent = no change)
 * - chrono::Weekday     → "Mon" | "Tue" | ... | "Sun"
 * - NaiveTime           → "HH:MM:SS" string
 */

// ---------------------------------------------------------------------------
// Enums (string union types)
// ---------------------------------------------------------------------------

export type Priority = 'Low' | 'Medium' | 'High' | 'Critical';

/** Every priority in display order (highest first) — the one list UI enumerations and validators share. */
export const PRIORITIES: readonly Priority[] = ['Critical', 'High', 'Medium', 'Low'];

/** TaskStatus uses serde rename_all = "lowercase" in Rust. */
export type TaskStatus = 'backlog' | 'pending' | 'scheduled' | 'completed' | 'cancelled';

/** Every task status in lifecycle order — the one list UI enumerations and validators share. */
export const TASK_STATUSES: readonly TaskStatus[] = [
  'backlog',
  'pending',
  'scheduled',
  'completed',
  'cancelled',
];

/** ChunkStatus uses serde rename_all = "lowercase" in Rust. */
export type ChunkStatus = 'scheduled' | 'completed';

export type Weekday = 'Mon' | 'Tue' | 'Wed' | 'Thu' | 'Fri' | 'Sat' | 'Sun';

// ---------------------------------------------------------------------------
// Cadence — uniform "interval + in-period day windows" model (mirrors Rust)
// ---------------------------------------------------------------------------

export type Period = 'Weekly' | 'Monthly';

/**
 * A contiguous in-period scheduling window: inclusive day offsets, 0-indexed
 * from the period's first day. Weekly `0..=6` from Monday; monthly `0..=27`
 * from the 1st (capped at the 28th). One window → one instance, schedulable
 * anywhere across `start..=end`. Windows are sorted by start and non-overlapping.
 */
export interface Window {
  start: number;
  end: number;
}

/**
 * Recurrence cadence: a base period, an `interval` multiplier over it, and the
 * in-period day `windows` to schedule (one instance per window).
 */
export interface Cadence {
  period: Period;
  interval: number;
  windows: Window[];
}

// ---------------------------------------------------------------------------
// Domain models
// ---------------------------------------------------------------------------

export interface Task {
  id: string;
  title: string;
  description: string | null;
  duration_minutes: number;
  time_logged_minutes: number;
  priority: Priority;
  status: TaskStatus;
  start_date: string | null;
  deadline: string | null;
  schedule_id: string;
  min_chunk_minutes: number;
  no_split: boolean;
  recurring_template_id: string | null;
  labels: string[];
  created_at: string;
  updated_at: string;
}

export interface Chunk {
  id: string;
  task_id: string;
  start_time: string;
  end_time: string;
  status: ChunkStatus;
  is_fixed: boolean;
  logged_minutes: number | null;
  completed_at: string | null;
  google_event_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface RecurringTemplate {
  id: string;
  title: string;
  description: string | null;
  duration_minutes: number;
  priority: Priority;
  schedule_id: string;
  cadence: Cadence;
  labels: string[];
  is_active: boolean;
  start_date: string;
  created_at: string;
  updated_at: string;
}

export interface Schedule {
  id: string;
  name: string;
  is_default: boolean;
  windows: ScheduleWindow[];
  created_at: string;
  updated_at: string;
}

export interface ScheduleWindow {
  id: string;
  schedule_id: string;
  day_of_week: Weekday;
  start_time: string;
  end_time: string;
}

export interface AppConfig {
  planning_horizon_days: number;
  timezone: string;
  max_continuous_minutes: number;
  min_break_minutes: number;
  last_reschedule: string | null;
  last_mutation: string | null;
  last_sync: string | null;
  last_busy_sync: string | null;
}

export interface ExternalBusyTime {
  id: string;
  start_time: string;
  end_time: string;
  source: string;
}

/** Reserved author of auto-generated progress comments (M12.2) — the one value UI checks share. */
export const SYSTEM_AUTHOR = 'SYSTEM';

/** A comment on a task. Markdown `content`; `author` is `SYSTEM_AUTHOR` for auto-generated notes. */
export interface Comment {
  id: string;
  task_id: string;
  author: string;
  content: string;
  created_at: string;
  updated_at: string;
}

// ---------------------------------------------------------------------------
// Input DTOs
// ---------------------------------------------------------------------------

export interface CreateTaskInput {
  title: string;
  description?: string | null;
  duration_minutes: number;
  priority?: Priority | null;
  start_date?: string | null;
  deadline: string;
  schedule_id?: string | null;
  min_chunk_minutes?: number | null;
  no_split?: boolean | null;
  labels?: string[] | null;
  status?: TaskStatus | null;
}

/**
 * Patch input for updating a task.
 *
 * All fields optional — absent fields are not changed.
 * Fields typed `T | null | undefined` support clearing:
 * - undefined / absent → don't change
 * - null → clear the field
 * - value → set to value
 */
export interface UpdateTaskInput {
  title?: string;
  description?: string | null;
  duration_minutes?: number;
  priority?: Priority;
  start_date?: string | null;
  deadline?: string;
  schedule_id?: string;
  min_chunk_minutes?: number;
  no_split?: boolean;
  labels?: string[];
  status?: TaskStatus;
}

export interface CreateTemplateInput {
  title: string;
  description?: string | null;
  duration_minutes: number;
  priority?: Priority | null;
  schedule_id?: string | null;
  cadence: Cadence;
  labels?: string[] | null;
  /** Recurrence anchor. Absent → backend defaults to now. */
  start_date?: string | null;
}

export interface UpdateTemplateInput {
  title?: string;
  description?: string | null;
  duration_minutes?: number;
  priority?: Priority;
  schedule_id?: string;
  cadence?: Cadence;
  labels?: string[];
  is_active?: boolean;
  /** Recurrence anchor. Absent → unchanged (required field, never cleared). */
  start_date?: string;
}

export interface CreateScheduleInput {
  name: string;
  windows: ScheduleWindowInput[];
}

export interface UpdateScheduleInput {
  name?: string;
  windows?: ScheduleWindowInput[];
}

export interface ScheduleWindowInput {
  day_of_week: Weekday;
  start_time: string;
  end_time: string;
}

export interface UpdateConfigInput {
  planning_horizon_days?: number;
  timezone?: string;
  max_continuous_minutes?: number;
  min_break_minutes?: number;
}

export interface CreateCommentInput {
  task_id: string;
  content: string;
  /** Absent/null → backend defaults to "User" (M12.10). */
  author?: string | null;
}

// ---------------------------------------------------------------------------
// Output DTOs
// ---------------------------------------------------------------------------

export interface AgendaItem {
  chunk: Chunk;
  task_title: string;
  task_priority: Priority;
  task_labels: string[];
  /** Template id when the task is a recurring instance (calendar "Edit template"). */
  task_recurring_template_id: string | null;
  /** Task deadline — chunks ending after it render the overdue treatment. */
  task_deadline: string | null;
}

/** Distinct label with its usage count (facet chips derive these from visible tasks). */
export interface LabelCount {
  label: string;
  task_count: number;
}

export interface TaskFilter {
  search_text?: string | null;
  statuses?: TaskStatus[] | null;
  labels?: string[] | null;
  /** Match-none semantics: tasks carrying any of these labels are dropped. */
  excluded_labels?: string[] | null;
  /** true = only tasks with no labels; false = only labeled tasks. */
  unlabeled?: boolean | null;
  /** Match-any (IN) semantics, like `statuses`; empty means unconstrained. */
  priorities?: Priority[] | null;
  deadline_before?: string | null;
  deadline_after?: string | null;
  schedule_id?: string | null;
  recurring_template_id?: string | null;
}

// ---------------------------------------------------------------------------
// Google Calendar types
// ---------------------------------------------------------------------------

/**
 * OAuth connection status — discriminated union (type field).
 *
 * Serialization contract: internally tagged with `type` field (serde default).
 */
export type AuthStatus =
  | { type: 'not_connected' }
  | { type: 'pending' }
  | { type: 'connected'; email: string | null };

/**
 * A Google Calendar visible to the authenticated account.
 *
 * `primary` marks the account's primary calendar — display first and label "(primary)".
 */
export interface ExternalCalendar {
  id: string;
  title: string;
  primary: boolean;
}

/** Sync bookkeeping recorded by the manual "Sync now" flow (display-only). */
export interface SyncStatus {
  /** Completion time of the last successful sync, or null if never synced. */
  last_sync_at: string | null;
  /** Message of the last failed sync; null after a successful sync. */
  last_sync_error: string | null;
}

/** A locally mirrored external calendar event (provider-owned, read-only). */
export interface ExternalEvent {
  id: string;
  calendar_id: string;
  event_id: string;
  title: string;
  description: string | null;
  start_time: string;
  end_time: string;
  busy: boolean;
  declined: boolean;
  /** When true, this is an all-day event; start_time/end_time span whole local days. */
  all_day: boolean;
  updated_at: string;
}

/**
 * Write payload for creating or editing a user-owned calendar event (G11).
 *
 * The counterpart to the read-side {@link ExternalEvent} mirror: it carries no
 * id (the target event is addressed by the `calendarId`/`eventId` arguments on
 * the update/delete calls) and uses `start`/`end` rather than the mirror's
 * `start_time`/`end_time`. Both are ISO 8601 instants; `end` must be strictly
 * after `start`.
 */
export interface UserEventPayload {
  title: string;
  description: string | null;
  start: string;
  end: string;
  /**
   * When true, `start`/`end` are written to Google as all-day `date` fields
   * (time-of-day ignored); `end` is the exclusive day after the last day.
   */
  all_day: boolean;
}

// ---------------------------------------------------------------------------
// Backup types (M11)
// ---------------------------------------------------------------------------

/** Backup bookkeeping for the Settings card (display-only; backend is lenient). */
export interface BackupStatus {
  /** Whether this profile opted into automatic backup. */
  enabled: boolean;
  /** Whether the backup target has credentials (can enable / back up now). */
  connected: boolean;
  /** Completion time of the last successful export, or null if never exported. */
  last_export_at: string | null;
  /** Last export/restore problem; null when the last run succeeded. */
  last_backup_error: string | null;
  /**
   * Set when this app run restored the database from a backup: the backup's
   * last-change time as RFC 3339, or '' when the backup carried none.
   */
  restored_this_run: string | null;
}

// ---------------------------------------------------------------------------
// Scheduling types (returned by reschedule commands)
// ---------------------------------------------------------------------------

export interface ScheduleResult {
  placed_chunks: Chunk[];
  warnings: ScheduleWarning[];
}

/** Provider write ops performed by the push leg of a manual sync. */
export interface PushCounts {
  created: number;
  updated: number;
  deleted: number;
}

/** Result of a manual "Sync now": the reschedule plus push-op counts. */
export interface SyncOutcome {
  schedule: ScheduleResult;
  pushed: PushCounts;
}

export interface ScheduleWarning {
  task_id: string;
  task_title: string;
  kind: WarningKind;
}

/** Externally tagged enum (default serde representation). */
export type WarningKind =
  | {
      DeadlineViolation: {
        deadline: string;
        earliest_completion: string;
      };
    }
  | {
      Unschedulable: {
        reason: string;
      };
    };

// ---------------------------------------------------------------------------
// Profile types (M13)
// ---------------------------------------------------------------------------

/** The unlocked profile this app instance is running as. */
export interface ActiveProfile {
  id: string;
  name: string;
}

/** A profile as listed by the gate/settings. */
export interface ProfileInfo {
  id: string;
  name: string;
  created_at: string;
}

/** Gate snapshot: whether a profile is active and what the picker lists. */
export interface ProfileStatus {
  active: ActiveProfile | null;
  profiles: ProfileInfo[];
  last_used: string | null;
}
