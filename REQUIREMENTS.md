# Apreswork - Requirements Document (MoSCoW)

A lightweight desktop task scheduler for after-work activities with Google Calendar integration and a Claude-friendly API.

**Tech stack:** Tauri + Svelte, SQLite, local-first.

---

## Implementation status

The letters are priorities, not delivery status. This table records what the code
delivers so that every gap is visible; whether a gap is closed on the code side or the
requirement side is a separate decision.

| Requirement | Status |
|---|---|
| M1–M3, M5, M7–M9, M11, M12 | Implemented |
| M4.1 cadence | **Diverged** — the cadence is a period, an interval and explicit in-period day windows, one instance per window; a multi-day window is one instance schedulable across those days. There is no "instances per week" count with automatic spreading, and the interval is only bounded below (≥ 1), not capped at 8 or 12 |
| M4.2–M4.4, M4.6, M4.7 | Implemented on the window model: a weekly instance's deadline is the end of its window's last day |
| M4.5 expiry | Weekly as written. **Monthly diverged from intent** — the code expires a monthly instance at its own widened deadline (the 28th ceiling, as the text below still says); the owner's ruling (2026-09-03) is that monthly mirrors weekly and expires when the next occurrence's window opens, a day-N monthly task being a whole-month window that starts on day N. Tracked as a backlog task; a second backlog task covers the longer-term rule of expiring an instance the moment it is placed outside its window while keeping cancelled tasks visible (configurable) |
| M4.2a | **Diverged** — editing a template deletes only future open *unpinned* instances (deadline after now); pinned (moved), completed, cancelled and overdue instances are kept, not erased |
| M6.4 | Stale wording — schedules have a UI (S2.1) and are *not* exposed by the REST API |
| M7, M11.2 | Push sync is on demand only: the Sync button, the REST sync-now endpoint. There is no background sync timer; the background timer only reschedules (debounce flush, past-due chunks, midnight) |
| M10.1 | Implemented — REST API on the loopback interface, `Host`-header check |
| M10.2 | **Partial** — REST covers task CRUD, task completion, comments, chunk move, agenda, labels, and the filter (label, status, priority, deadline range, text). Recurring templates, schedules and an explicit reschedule trigger are Tauri commands only; over REST a reschedule happens as part of sync-now or calendar-pull |
| M10.3 chat-driven task operation | **Not implemented** — mechanism undecided (CLI, MCP server, or in-app chat); no code for any of them. Agents drive the REST API through `scripts/api.sh` for now |
| M13 Profiles | Implemented |
| S1.1, S1.2, S2.1, S2.2 | Implemented (S2.2 as the schedule-window overlay on the calendar) |
| S1.3 contention | **Partial** — the single scheduling order already lets a higher-priority task take an overlapping slot; there is no round-robin between schedules on equal priority |
| S3, S4, S5, S6 | Implemented |
| S7, S8, S9 | Implemented |
| C1 task dependencies | Not implemented |
| C3 calendar-triggered rescheduling | **Partial** — a sync pulls external events and reschedules around them; nothing detects a remote change and triggers a reschedule on its own |
| C4 bidirectional sync | Implemented — the sync cycle's three-way merge accepts or rejects moves made in Google Calendar and reflects deletions |
| C5 label management | Not implemented — labels are free-form strings; the store only lists them with counts |
| C6 desktop notifications | Not implemented |
| W1–W7 | Not implemented, as intended |

---

## Glossary

- **Task**: A unit of work with title, description, duration estimate, priority, and optional scheduling metadata.
- **Chunk**: A scheduled time block for a task. A task may be split into multiple chunks based on its minimum chunk size.
- **Schedule**: A named set of recurring time windows during which tasks assigned to that schedule may be placed (e.g., "reading: weekends 8–12").
- **Label**: A tag for grouping/filtering tasks (e.g., "fitness", "reading", "home").
- **Recurring task**: A task template that auto-generates instances at a defined cadence.
- **Instance**: A concrete task generated from a recurring template for a specific cadence period. Has its own deadline, time log, and a single chunk (instances are always "no split").
- **Fixed chunk**: A chunk that is locked to a specific time slot (whether manually moved there or created as fixed). Shown with a lock icon. Other chunks of the same task remain auto-schedulable.
- **Planning horizon**: The time window into which the scheduler places tasks (default: 1 month from today).

---

## M — Must Have

### M1: Core Task Model

- **M1.1** Tasks have: title (required), description (optional), duration (required), priority, start date (optional), deadline (required; except recurring templates — see M4.2), labels (zero or more). Labels are free-form strings (tags).
- **M1.2** Description supports Markdown with checkbox lists and clickable links.
- **M1.3** Duration is required for all tasks. Can range from 5 minutes to hundreds of hours.
- **M1.4** Priority levels: **Low**, **Medium** (default), **High**, **Critical**. Used for scheduling order.
- **M1.5** Tasks track cumulative time logged vs. total estimated duration. Total duration can be adjusted after creation.
- **M1.6** Minimum chunk size is configurable per task (minimum 5 minutes, default 30 minutes). Tasks can also be marked as "no split" (must be scheduled as a single block).
- **M1.7** Tasks can be placed in **backlog** status. Backlogged tasks are visible in the task list but ignored by the scheduler. Moving a task out of backlog enables auto-scheduling.

### M2: Task Lifecycle

- **M2.1** Task states: **backlog** (not prioritised, excluded from scheduling), **pending** (not yet scheduled), **scheduled** (placed in calendar), **completed** (all chunks completed; derived automatically), **cancelled**. Chunks have their own states: **scheduled** or **completed**.
- **M2.2** Completing a task chunk logs the time. Completion timestamp = when the user clicks "complete."
- **M2.3** When a chunk is completed and remaining duration > 0, the remainder is re-queued for scheduling.
- **M2.4** Cancel is available for any task. Cancelling removes all scheduled chunks (completed chunks are retained).
- **M2.5** Completed and cancelled tasks are retained in history (viewable in task list).
- **M2.6** A completed chunk can be **reopened** (clicking it in the calendar). This subtracts the chunk's logged duration from the task's cumulative time and returns the chunk to scheduled state.
- **M2.7** Tasks can be **permanently deleted** from both the UI and API.

### M3: Scheduling Engine

- **M3.1** The scheduler places task chunks into available slots within the configured schedule windows, inside the planning horizon (default 1 month).
- **M3.2** Scheduling priority order: (1) higher priority first, (2) earlier deadline first, (3) shorter remaining duration first, (4) lexicographic by title.
- **M3.3** Tasks with a start date are not scheduled before that date.
- **M3.4** Tasks with a deadline are scheduled to complete before the deadline (if possible).
- **M3.5** Manual reschedule trigger (button in UI) re-runs the scheduling algorithm for all pending/incomplete tasks. The scheduler recomputes placement for all non-fixed, non-completed chunks and regenerates recurring instances within the planning horizon. Chunks that end up in the same position are preserved (to maintain Google Calendar event IDs); only actually changed chunks are created, updated, or removed. Backlogged tasks are not affected.
- **M3.5a** On app startup, if the last reschedule was more than 24 hours ago, an automatic reschedule is triggered.
- **M3.6** Tasks that cannot meet their deadline or cannot be scheduled at all (e.g., "no split" task too large for any available window) are surfaced in a dedicated **Status view** (accessible via sidebar tab) so the user can resolve them. The sidebar shows an indicator (amber for deadline violations, red for unschedulable) when warnings exist.
- **M3.6a** Each warning in the Status view offers quick resolution actions:
  - **Extend deadline** — preset options ("next week", "next month") and a custom date picker.
  - **Do now** — creates a fixed chunk starting at the current time for the task.
  - **Cancel task** — cancels the task (equivalent to "won't do").
- **M3.6b** Clicking a warning item opens the task edit form (same TaskForm used elsewhere) as a modal, allowing direct task editing from the Status view.
- **M3.7** Tasks outside the planning horizon remain in **pending** state and are visible in the task list.
- **M3.8** The scheduler minimizes chunk count (greedy: use largest possible chunks). Chunks are capped at `max_continuous_minutes` (global setting, default 2 hours). After a continuous block reaches this limit, a break of at least `min_break_minutes` (global setting, default 5 minutes) is inserted before the next chunk. "No split" tasks are placed as a single block regardless of length, but a break is enforced after them.

### M4: Recurring Tasks

- **M4.1** Recurring tasks define a cadence: weekly (select days of the week + instances per week + an interval in weeks, e.g. every 2 weeks; 1–8) or monthly (specific day of month + an interval in months, e.g. every 3 months; 1–12). The interval counts on-cadence periods forward from the template's `start_date` (the anchor): only every Nth week/month relative to the anchor produces instances. Instances per week must not exceed the number of selected days. When instances < selected days, instances are evenly spread across the selected days (e.g., Mon/Tue/Wed with 2 instances → Mon, Wed).
- **M4.2** Recurring templates do not have a deadline. Instances are auto-generated within the planning horizon, each inheriting duration, priority, labels, and schedule assignment from the template. Instance deadlines are derived as follows:
  - **Weekly, 1 instance, 1 day selected**: deadline = end of that day.
  - **Weekly, 1 instance, multiple days selected**: deadline = end of the last candidate day in that week (gives scheduling flexibility across all candidate days).
  - **Weekly, N instances, N+ days selected**: each instance's deadline = end of its scheduled day.
  - **Monthly, 1 instance (single window)**: deadline = end of the last guaranteed day in the month (the 28th, the last day every calendar month has). The scheduled day opens the window; the instance remains schedulable — and visible — through the 28th regardless of which day was configured.
  - **Monthly, N instances (multiple windows)**: each instance's deadline = end of the day before the next window opens, except the last instance whose deadline = end of the 28th (guaranteed last day). This caps each instance to its own slice of the month.
- **M4.2a** Editing a recurring template erases all pending and scheduled instances (including fixed ones) and regenerates/reschedules them from the updated template. Completed and cancelled instances are not affected.
- **M4.2b** Deleting a recurring template deletes all its pending and scheduled instances. Completed and cancelled instances are de-linked from the template and retained in history.
- **M4.3** Recurring instances are always "no split" — they are scheduled as a single chunk and cannot be broken into multiple chunks.
- **M4.4** Each instance tracks its own independent time log.
- **M4.5** Each instance carries an `expire_at` that bounds how long an uncompleted instance persists before auto-cancellation. The value is period-dependent: **Weekly**: end of the first day of the next occurrence's window (local 23:59:59) — a missed instance is cancelled once the next one comes due (e.g. on a Wed/Fri cadence, a missed Wednesday is cancelled when Friday passes; a missed Sat–Sun weekend survives only into the Saturday of the next weekend). **Monthly**: the instance's own widened deadline (the period-aware ceiling: the day before the next window opens for non-last windows; the 28th — last guaranteed day — for the last window in the period). No carry-over grace into the next period.
- **M4.6** Cancelling a single instance does not affect the recurring series — the next eligible instance is generated normally.
- **M4.7** Moving a recurring instance to a different time makes it fixed (locked) but does not create a duplicate instance for the original slot.

### M5: Fixed (Locked) Chunks

- **M5.1** If a user manually moves a chunk (via drag-and-drop), that chunk becomes **fixed**. Other chunks of the same task remain auto-schedulable.
- **M5.2** Fixed chunks display a lock icon in the calendar view.
- **M5.3** The scheduler does not move fixed chunks during rescheduling.
- **M5.4** Chunks can also be created as fixed from the start (pre-scheduled to a specific time).
- **M5.5** Fixed chunks are exempt from schedule window constraints — they can be placed at any time, including outside the task's schedule windows.
- **M5.6** Fixed chunks can **overlap** with other chunks (both auto-scheduled and fixed). Overlapping auto-scheduled chunks are displaced on the next reschedule. Overlapping fixed chunks are shown as conflicts in the calendar (side-by-side, similar to Google Calendar).
- **M5.7** A fixed chunk can be **unlocked** (lock removed), returning it to auto-scheduling. On the next reschedule, the scheduler may move it.

### M6: Schedule Windows

- **M6.1** A schedule is a named set of time windows (e.g., "weekdays 7:00–9:00 + 18:00–23:00, weekends 8:00–22:00").
- **M6.2** A **default schedule** always exists and cannot be deleted. Its initial windows are: weekdays 7:00–9:00 + 18:00–23:00, weekends 8:00–22:00. Each task is assigned to exactly one schedule; if none is specified, the default is used.
- **M6.3** The scheduler only places task chunks within their assigned schedule's time windows.
- **M6.4** Schedules are stored in SQLite (same as all other data). UI configuration is a Should Have; until then, schedules are managed via the API.
- **M6.5** All times stored internally in UTC. A global app timezone setting (in a settings page) converts for display.

### M7: Google Calendar Integration

- **M7.1** Task chunks are synced to a **dedicated** Google Calendar (separate from primary).
- **M7.2** Scheduled and completed task chunks appear as calendar events with a hardcoded color (configurable later).
- **M7.3** Sync does not block scheduling. The scheduler works offline; conflict avoidance with Google Calendar is a Should Have (see S6).
- **M7.4** Sync is app-to-calendar (the app is the source of truth). Edits should be made in the app, not in Google Calendar.

### M8: UI — Calendar View

- **M8.1** In-app calendar view with **daily** and **weekly** modes.
- **M8.2** Drag-and-drop support for moving task chunks to different time slots.
- **M8.3** Moving a chunk converts it to fixed (lock icon appears).
- **M8.4** Visual distinction between scheduled (upcoming) and completed chunks. Overlapping chunks are displayed side-by-side (narrower) with a conflict indicator, similar to Google Calendar.
- **M8.5** Click-and-drag on an empty calendar slot to select a time range, which opens the "create task" form with the selected start time and duration pre-filled. The created chunk is fixed to that slot. If the user toggles the task to recurring, the fixed time is cleared but the duration is kept.
- **M8.6** Drag to resize chunk duration in the calendar (in 5- or 15-minute increments). Resizing rules:
  - **Increasing**: the chunk absorbs time from the task's remaining budget first. Only if the new chunk duration exceeds the remaining budget does the task's total estimated duration grow (by the overflow amount). The chunk becomes fixed.
  - **Decreasing a completed chunk**: decreases the task's total estimated duration by the same amount.
  - **Decreasing a non-completed chunk**: reduces the time allocated to that chunk only; remaining time is re-queued for scheduling. The chunk becomes fixed.
  - Minimum chunk size does **not** apply to manual resizing — it only constrains the scheduler.

### M9: UI — Task List View

- **M9.1** List of all tasks (backlog, pending, scheduled, completed, cancelled).
- **M9.2** Sortable by status, priority, deadline.
- **M9.3** Filterable by label.
- **M9.4** Task detail panel showing: title, description, time logged vs. total, labels, schedule assignment, deadline, status.
- **M9.5** Recurring task instances appear as regular tasks in the list. Clicking an instance shows a button to edit the parent recurring template. If a recurring template has no instances within the planning horizon, its next upcoming instance is still shown in the task list (even if outside the horizon) so the template remains accessible.
- **M9.6** The "create task" form offers a toggle to define the task as recurring, which reveals cadence options (weekly/monthly dropdown, day selection, instance count).

### M10: Claude-Friendly API (Local)

- **M10.1** Local HTTP REST API (localhost, no auth) exposing task management operations.
- **M10.2** Supported operations:
  - CRUD tasks (create, read, update, delete)
  - Create/edit recurring tasks
  - Mark chunk complete (optional duration override; defaults to auto-log of scheduled duration)
  - Move a chunk to a different time slot
  - Trigger reschedule
  - Get agenda (scheduled tasks in a date range, with optional label filter)
  - Create/edit/list schedules
  - Search/filter tasks by label, status, priority, date range
- **M10.3** Operating tasks from a chat: an easy, automated way to create and manage
  tasks by talking to an assistant, without opening the app. The mechanism is
  **undecided** — a CLI the assistant drives, an MCP server wrapping the REST API, or
  a chat integration inside the app (see W7). Until then the REST API with
  `scripts/api.sh` is the interim path.

### M11: Storage & Backup

- **M11.1** Local SQLite database for all task, schedule, label, and recurrence data.
- **M11.2** Offline-first: app is fully functional without internet. Google Calendar sync happens when connectivity is available.
- **M11.3** Export/backup: ability to export the full database as a zip file (for manual backup to Drive or similar).

### M12: Task Comments

- **M12.1** Tasks can have comments. Each comment has: content (Markdown), author, `created_at`, `updated_at` timestamps.
- **M12.2** Author is a string field. Reserved name `SYSTEM` is used for automatically generated comments. All other values represent human or agent authors. Phase A uses `User` (hardcoded) and `SYSTEM` only; agent-specific author names are deferred until authentication is implemented.
- **M12.3** Comments are editable by their author only. System comments are immutable.
- **M12.4** Comments are displayed in **reverse chronological order** (newest first) at the bottom of the task form.
- **M12.5** System comments are generated automatically when task progress changes:
  - Chunk completed: logs duration added and running total (e.g., "Chunk completed: +45m logged (1h 15m / 2h total)").
  - Chunk reopened: logs duration subtracted and running total.
- **M12.6** System comments are visually distinct: smaller text, muted/semi-transparent color, no author header. They appear as compact inline annotations rather than full comments.
- **M12.7** User comments have an author header and a context menu ("dots") for editing. Editing uses the same text input field with prepopulated content.
- **M12.8** Comments are cascade-deleted when a task is permanently deleted.
- **M12.9** Recurring task instances have their own independent comments (no inheritance from template).
- **M12.10** *Design note (future)*: agent authentication will allow registering distinct agent identities as comment authors. Until then, agent access via CLI uses `User` as the author.

### M13: Profiles

- **M13.1** Multiple named profiles inside one installed app, each a fully isolated data set: its own database and Google token under `profiles/<id>/` in the app data directory, listed in a `profiles.json` registry.
- **M13.2** A profile picker gates the app: nothing, the local API server included, opens until a profile is unlocked. Profiles can be created, renamed, deleted and switched in-app.
- **M13.3** The local API reports the active profile and can list and switch profiles. A switch is global (it moves the running app), so an API client passes the profile it expects and is refused on a mismatch.
- **M13.4** The first launch of a profiles-aware build adopts the pre-profiles database and Google token into a "Default" profile, keeping the originals as `*.pre-profiles-backup`.

---

## S — Should Have

### S1: Multiple Schedules with Contention Resolution

- **S1.1** Multiple named schedules can be defined and active simultaneously.
- **S1.2** Tasks are assigned to a specific schedule; the scheduler respects schedule boundaries.
- **S1.3** When schedules overlap in time windows: highest priority task wins the slot. Equal priority: round-robin between schedules.

### S2: Schedule Management UI

- **S2.1** In-app UI for creating, editing, and deleting schedules (instead of config file only). Deleting a non-default schedule reassigns all its tasks to the default schedule.
- **S2.2** Visual preview of schedule time windows on the calendar.

### S3: Automatic Rescheduling

- **S3.1** If a scheduled task's end time is more than 1 hour in the past and it has not been marked complete, automatically re-queue it.
- **S3.2** Nightly auto-reschedule (midnight) as a simpler fallback.

### S4: Import/Restore

- **S4.1** Import a previously exported zip backup to restore state.

### S5: Recurring Template List View

- **S5.1** A separate tab/section in the task list for viewing and managing recurring templates directly (instead of navigating through instances).

### S6: Google Calendar Conflict Avoidance

- **S6.1** Conflict detection reads **all** calendars the user has access to and avoids scheduling task chunks in slots marked as busy.
- **S6.2** When offline or calendar data is unavailable, the scheduler proceeds without conflict checking.

### S7: User Events on Google Calendar

- **S7.1** Standalone events (not tied to a task) can be created, edited and deleted from the in-app calendar on any of the user's Google calendars. The provider write happens first; the event is then mirrored locally and the scheduler reschedules around it.

### S8: External Calendars

- **S8.1** The user picks, in Settings, which of their Google calendars are pulled. Their events are mirrored locally, drawn on the in-app calendar, and treated as busy time by the scheduler (S6).

### S9: Automatic Backup to Google Drive

- **S9.1** Per-profile opt-in backup of the database to Google Drive: an interval export (default 5 minutes) that runs only when the database changed, plus an export on exit.
- **S9.2** Restore check before the store opens: a newer backup on Drive wins and is pulled on start. A stale-writer guard blocks exports from an app run that is behind the Drive copy until it restarts.
- **S9.3** Backup status (enabled, connected, last export, last error, restored-this-run) is shown in Settings and exposed by the local API together with a manual "back up now".

---

## C — Could Have

### C1: Task Dependencies

- **C1.1** A task can be marked as "blocked by" another task.
- **C1.2** Blocked tasks are not scheduled until their blocker is completed.
- **C1.3** Visual indicator in task list for blocked tasks.

### ~~C2: Comments on Tasks~~ → Promoted to M12

See **M12** below.

### C3: Calendar-Triggered Rescheduling

- **C3.1** Detect new/changed events in Google Calendar and automatically trigger rescheduling when conflicts arise.

### C4: Bidirectional Calendar Sync

- **C4.1** Detect deletions or moves of task events made directly in Google Calendar and reflect them in the app.

### C5: Label Management

- **C5.1** Labels as managed entities with CRUD operations, pre-select suggestions, and rename/merge support.

### C6: Desktop Notifications

- **C6.1** OS-level notifications before a scheduled task starts (configurable lead time).

---

## W — Won't Have (for now)

- **W1** Subtasks / child task hierarchy
- **W2** Image or file attachments on tasks
- **W3** Multi-user / sharing / collaboration
- **W4** Mobile app (desktop only for now)
- **W5** Cloud sync between devices
- **W6** Authentication on the local API — no token/credential scheme. Decision
  (2026-07): the API still validates the `Host` header against loopback forms as a
  browser DNS-rebinding defense (DESIGN §6.2); that is a trust-boundary check, not
  authentication, and keeps the no-auth UX. A bearer token remains a possible later
  hardening step if agent multi-tenancy (M12.10) lands.
- **W7** In-app AI chat — **under review**: M10.3 lists an in-app chat integration as
  one candidate mechanism, so this stays a Won't only if the CLI or MCP route is chosen.

---

## Design Decisions

1. **Recurring task cadence UI**: Dropdown with weekly/monthly options. For weekly: user selects which days of the week and how many instances per week (instances ≤ selected days; evenly spread when instances < days).
2. **Planning horizon configurability**: Global setting only (default 1 month). Per-task configurability deferred.
3. **Label colors**: No label colors for now — keep it simple.
4. **Chunk completion UX**: Auto-log the scheduled duration on "done" click. No manual time confirmation.
5. **Google Calendar event details**: Task title only in the calendar event.
6. **Offline-first priority**: Scheduling works fully offline. Google Calendar conflict avoidance is a Should Have that enhances scheduling when available.
7. **Deadline required**: All tasks must have a deadline. Scheduling tiebreaker chain: priority → deadline → shorter duration → title (lexicographic).
8. **Reschedule modes**: Two modes — **full** (recompute all auto-chunks) and **incremental** (reschedule specific tasks with priority-aware cascading). Full reschedule for manual trigger, startup, structural changes. Incremental for single-task mutations (complete, resize, move). A diff preserves unchanged chunks (and their Google Calendar event IDs).
9. **Chunk resize behaviour**: Resize changes the chunk size and marks it fixed. The incremental reschedule (triggered automatically) handles budget reallocation — growing task duration if needed, placing/removing auto-chunks for the remaining time.
10. **Google Calendar event color**: Hardcoded for now, configurable later.
11. **Deletion and Google Calendar**: Deleting a task does not require removing past events from Google Calendar. Cleanup is optional/best-effort.
12. **Duration invariant**: `task.duration_minutes - task.time_logged_minutes == sum(scheduled chunk durations)`. This invariant is **eventually consistent** — operations like resize may temporarily break it. The scheduler orchestrator repairs it before any scheduling decisions. Safe because no user-facing operation depends on the invariant between repairs.
13. **Short tasks**: Tasks with duration ≤ min_chunk_minutes are auto-set to "no split" on creation.
14. **Task duration floor**: Task duration cannot be reduced below time_logged_minutes. To reduce further, resize individual completed chunks first.
