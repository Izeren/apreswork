# Apreswork — Scheduler Algorithm Specification

Pseudo-algorithm specification and complexity analysis for the scheduling engine.
Cross-references DESIGN.md §4.2, §5.2, §5.3 and REQUIREMENTS.md M3, M4.

---

## 1. Notation and Variables

| Symbol | Meaning |
|--------|---------|
| T | Number of schedulable tasks |
| S | Number of raw available slots (from slot expansion) |
| S' | Number of free slots after subtraction and alignment |
| F | Number of fixed/completed chunks (all tasks combined) |
| C | Total number of placed chunks (output) |
| H | Planning horizon in days (default 30) |
| W | Total number of schedule windows across all schedules |
| R | Number of active recurring templates |
| D | Total desired occurrences generated per template (within the horizon) |
| I | Total recurring instances (across all templates) |
| N_sched | Number of distinct schedules |

**Typical magnitudes** (single-user desktop app):
T ≈ 10–200, S ≈ 60–300, F ≈ 0–50, C ≈ 20–500, H = 30, W ≈ 4–20, R ≈ 0–30.

---

## 2. Reschedule Pipeline Overview

The free function `scheduling::reschedule` orchestrates the full pipeline.

```
function reschedule(store, scheduler, now):
    # ── Preparation ──────────────────────────────────────
    1.  config = store.get_config()                         # O(1) DB read
        horizon_end = now + config.planning_horizon_days
        tz = config.timezone

    2.  for template in store.list_templates():             # O(R) DB read
            reconcile(store, template,                     # §7.4 below — in-place, churn-minimal
                       now, horizon_end, tz)

    3.  auto_cancel_overdue(store, now)                     # §7.7 below

    3a. release_stale_fixed_locks(store, now)               # §2.3 below

    # ── Shared prep (config re-read, slot pool) ─────────
    #    ReschedulePrep::load — shared by both pipelines.
    #    Runs AFTER the mutating steps (2, 3, 3a) so its
    #    snapshot reflects their changes.
    1'. config = store.get_config()                         # O(1) — re-read after mutating steps
        horizon_end = now + config.planning_horizon_days
    6.  fixed    = store.get_all_fixed_and_completed()      # O(F) — is_fixed=true OR status=Completed
    7.  raw_slots = expand_schedule_windows(                # §3 below
                        store.list_schedules(), tz,
                        now, horizon_end)
    7a. events = store.get_external_events_in_range(        # [S6] local mirror, 0 API calls
                    now, horizon_end)
        busy = [e for e in events if e.busy]                # transparent+declined events skipped
        raw_slots = subtract_intervals(raw_slots, busy + fixed)  # §4
    7b. raw_slots = align_slots_to_grid(raw_slots)          # §4.1 — snap to minute grid

    # ── Gather data ──────────────────────────────────────
    4.  tasks = store.get_schedulable_tasks()               # O(T) — status IN (Pending,Scheduled)
                                                            #         AND time_logged < duration
    5.  old_auto = store.get_auto_chunks()                  # O(C_old) — non-fixed, non-completed

    # ── Core scheduling ──────────────────────────────────
    8.  result = scheduler.schedule(ScheduleInput {          # §5 below
            tasks, fixed, raw_slots, horizon_end, now,
            config.max_continuous_minutes,
            config.min_break_minutes
        })
    8a. retain_horizon_warnings(result.warnings, tasks,     # drop warnings outside horizon
            horizon_end)

    # ── Apply diff (atomic transaction) ──────────────────
    #    Steps 9–11 run in a single SQLite transaction so
    #    concurrent readers see either the old or new state.
    9.  ops = diff_chunks(old_auto, result.placed_chunks)   # §8 below
        apply(ops, store)

    10. for task in tasks:
            has_chunks = any chunk exists for task (placed or fixed)
            if task.status == Pending  AND has_chunks  → set Scheduled
            if task.status == Scheduled AND !has_chunks → set Pending

    11. config.last_reschedule = now
        config.last_mutation = now
        store.update_config(config)

    12. return result    # includes warnings
```

### 2.2 Incremental Reschedule (Cascading)

Priority-aware rescheduling for specific tasks. Higher-priority tasks can
displace lower-priority auto-chunks, cascading the reschedule to affected tasks.

Unlike the full pipeline, incremental skips recurring-instance generation (step 2),
overdue cancellation (step 3), and the `last_reschedule` timestamp update.

```
function reschedule_incremental(store, scheduler, initial_task_ids, now):
    if initial_task_ids is empty: return empty result

    # ── Stale-lock release ───────────────────────────────
    release_stale_fixed_locks(store, now)                   # §2.3 below

    # ── Fix invariant for initially affected tasks ────────
    for task_id in initial_task_ids:                         # O(T_init)
        task = store.get_task(task_id)
        fixed_durations = sum(c.duration for c in get_fixed_chunks(task_id))
        total_committed = task.time_logged_minutes + fixed_durations
        if total_committed > task.duration_minutes:
            task.duration_minutes = total_committed
            store.update_task(task)

    # ── Shared prep (same as full pipeline) ──────────────
    config, horizon_end, fixed, free_slots = ReschedulePrep.load(store, now)

    # ── Gather + sort all tasks ──────────────────────────
    all_tasks = store.get_schedulable_tasks()                # O(T)
    sort(all_tasks, by=scheduling_order)                     # O(T log T)

    all_auto_chunks = store.get_auto_chunks()                # O(C)
    existing_auto = group_by(all_auto_chunks, task_id)

    # ── Cascading placement ──────────────────────────────
    affected = set(initial_task_ids)
    old_auto_by_task = {}
    new_auto_by_task = {}
    processed = set()

    for task in all_tasks:                                   # O(T) iterations
        if task.id in affected:
            # Snapshot old chunks for diff
            old_auto_by_task[task.id] = existing_auto[task.id]

            # Place this task into currently free slots
            result = scheduler.schedule(ScheduleInput {      # O(S') per task
                tasks: [task], fixed, free_slots, ...
            })
            new_auto_by_task[task.id] = result.placed_chunks
            all_warnings += result.warnings

            # Check: did new chunks displace any unprocessed task's auto-chunks?
            for placed in result.placed_chunks:
                for other_id, other_chunks in existing_auto:  # O(C) total
                    if other_id in processed or other_id == task.id: continue
                    if any(overlaps(placed, oc) for oc in other_chunks):
                        affected.add(other_id)                # cascade!

            # Consume placed slots
            free_slots = subtract_intervals(free_slots, result.placed_chunks)
        else:
            # Unaffected — keep existing auto-chunks, consume their slots
            free_slots = subtract_intervals(
                free_slots, existing_auto[task.id]
            )

        processed.add(task.id)

    # ── Apply diff for all affected tasks (transaction) ──
    for task_id in affected:
        ops = diff_chunks(
            old_auto_by_task.get(task_id, []),
            new_auto_by_task.get(task_id, [])
        )
        apply(ops, store)

    # ── Update statuses for affected tasks ───────────────
    for task_id in affected:
        update_task_status(task_id, store)

    config.last_mutation = now
    store.update_config(config)

    retain_horizon_warnings(all_warnings, all_tasks, horizon_end)
    return ScheduleResult { placed: all placed chunks, warnings: all_warnings }
```

**Convergence**: Tasks processed in strict priority order. A processed task
can only displace unprocessed (lower-priority) tasks. Each task processed at
most once → no cycles. Worst case: all tasks affected → equivalent to full reschedule.

**Complexity**: O(T log T + T_affected × S') where T_affected varies by scenario.
For edits/completions T_affected ≈ 1–3. For deletion of a high-priority task with
many chunks, cascading displacement can propagate through most subsequent tasks
(up to T_affected = T), making incremental equivalent to full reschedule.

### 2.3 Stale Fixed-Lock Release

Fixed chunks whose `end_time` is more than `STALE_FIXED_LOCK_HOURS` (4) hours
before `now` are stale: the scheduled slot was missed, so the lock is dead
weight. `release_stale_fixed_locks` unlocks them (`is_fixed = false`) before
each reschedule pass so the scheduler can reclaim those slots and re-place the
owning tasks. Completed chunks are never touched. After unlocking, each
affected task's `is_pinned` flag is re-synced via `sync_task_pinned`
(`is_pinned` ⇔ the task still has at least one fixed chunk).

Runs inside a single transaction; operates on both the full and incremental
pipelines (`scheduling.rs`).

```
function release_stale_fixed_locks(store, now):
    cutoff = now - STALE_FIXED_LOCK_HOURS hours          # 4h
    with_tx:
        for chunk in store.get_fixed_scheduled_chunks():  # is_fixed=1 AND status=Scheduled
            if chunk.end_time < cutoff:
                chunk.is_fixed = false
                store.update_chunk(chunk)
                sync_task_pinned(store, chunk.task_id)    # recompute is_pinned
```

**Complexity**: O(F_sched) time where F_sched is the count of fixed+scheduled chunks.

### 2.4 Horizon Warning Filter

`retain_horizon_warnings` filters warnings after the scheduler returns:

- **DeadlineViolation**: kept only when the violated deadline is on or before
  `horizon_end`. A deadline past the horizon is not yet actionable.
- **Unschedulable**: kept only when the task has a deadline on or before
  `horizon_end`. Without one, unplaced work is normal backlog that may become
  schedulable in a later horizon.

### 2.5 Trigger Timing

The `RescheduleTrigger` coordinator maps each completed domain mutation to a
reschedule mode and timing via `policy_for` — the single code definition of the
trigger table (`services/trigger.rs`). Command surfaces construct the `Mutation`
variant and call `trigger_mutation`; they never pick a mode or timing themselves.

The production debounce duration is `Duration::ZERO` (`RescheduleTrigger::new`),
so debounced triggers execute immediately — the debounce infrastructure exists
for future configurability. With a non-zero duration, debounced entries are
coalesced and flushed when their deadline passes.

| Mutation | Mode | Timing |
|----------|------|--------|
| TaskCreated | Incremental | Debounced |
| TaskUpdated (not to backlog) | Incremental | Debounced |
| ChunkMoved | Incremental | Debounced |
| ChunkResized | Incremental | Debounced |
| ChunkLocked | Incremental | Debounced |
| ChunkUnlocked | Incremental | Debounced |
| FixedChunkDeleted | Incremental | Debounced |
| ChunkCompleted | Incremental | Immediate |
| ChunkReopened | Incremental | Immediate |
| TaskUpdated (to backlog) | Full | Debounced |
| TaskDeleted | Full | Debounced |
| TaskCancelled | Full | Debounced |
| TaskCompleted | Full | Immediate |
| FixedChunkCreated | Full | Immediate |
| TemplateCreated | Full | Immediate |
| TemplateUpdated | Full | Immediate |
| TemplateDeleted | Full | Immediate |
| ScheduleCreated | Full | Immediate |
| ScheduleUpdated | Full | Immediate |
| ScheduleDeleted | Full | Immediate |
| ConfigUpdated | Full | Immediate |

Mode coalescing: `Full` + anything → `Full`; `Incremental` + `Incremental` →
merged, deduplicated task IDs.

A background timer thread (`start_background_timer`) polls every 250ms for
pending flushes, and every `BACKGROUND_CHECK_INTERVAL` (300s) checks for
past-due scheduled chunks (1h grace) and midnight crossings — both trigger a
full immediate reschedule.

---

## 3. Slot Finder — `expand_schedule_windows`

Converts abstract schedule windows (day-of-week + local time range) into concrete
UTC time intervals for the planning horizon.

```
function expand_schedule_windows(schedules, tz, start, end) -> Vec<Slot>:
    slots = []
    for day in each_calendar_day(start, end, tz):           # H iterations (+ 1-day buffer)
        weekday = day.weekday_in(tz)
        for schedule in schedules:                          # N_sched iterations
            for window in schedule.windows:                 # W_per_sched iterations
                if window.day_of_week != weekday:
                    continue
                # Convert local times to UTC for this specific date (DST-aware)
                slot_start = local_to_utc(day, window.start_time, tz)
                slot_end   = local_to_utc(day, window.end_time, tz)
                if slot_end <= slot_start: continue         # degenerate
                if slot_end <= start or slot_start >= end:
                    continue    # outside horizon
                slot_start = max(slot_start, start)         # clip to horizon
                slot_end   = min(slot_end, end)
                slots.append(Slot { slot_start, slot_end, schedule.id })

    sort(slots, by=start_time)                              # O(S log S)
    return slots
```

**Complexity**: O(H × W + S log S) time, O(S) space.
Where S = number of emitted slots ≤ H × W.

**DST handling**: `local_to_utc` resolves ambiguity and gaps:
- **Spring-forward gap** (time does not exist): advances 1 hour to the post-gap
  time, shortening the window rather than dropping it.
- **Fall-back ambiguity** (time exists twice): picks the earliest UTC
  interpretation (pre-transition / summer-time offset).

---

## 4. Fixed-Chunk Subtraction — `subtract_intervals`

Removes occupied time (fixed chunks, completed chunks, busy external events)
from available slots. Used both for fixed-chunk avoidance and Google Calendar
busy-time subtraction ([S6]).

Slots of *different* schedules may overlap in wall-clock time (each schedule is an
independent source of availability), so the sweep runs once per `schedule_id`
group; every occupied interval is subtracted from every group. Within one
schedule, validation guarantees windows never overlap (touching is legal), which
is the precondition the single-cursor sweep relies on.

```
function subtract_intervals(slots, occupied) -> Vec<Slot>:
    result = []
    for group in slots.group_by(schedule_id):               # BTreeMap: deterministic
        result += subtract_single_schedule(group, occupied)
    sort(result, by=(start, schedule_id))
    return result

function subtract_single_schedule(slots, occupied) -> Vec<Slot>:
    # Precondition: slots share one schedule_id and never overlap (may touch).
    events = []
    for s in slots:
        events.append((s.start, 'slot_start', s))
        events.append((s.end,   'slot_end',   s))
    for o in occupied:
        events.append((o.start, 'occ_start', o))
        events.append((o.end,   'occ_end',   o))

    sort(events, by=(time, type_priority))                  # O((S+F) log(S+F))
    # type_priority: occ_start < slot_end < slot_start < occ_end (tie-breaking).
    # slot_end BEFORE slot_start: adjacent (touching) slots must close the
    # earlier slot — emitting its final fragment — before the next one opens.
    # occ_start first / occ_end last keep boundary-touching busy intervals
    # non-subtracting.

    result = []
    occ_depth = 0       # number of active occupied intervals
    slot_open = false
    free_start = None

    for (time, type, obj) in events:
        match type:
            'slot_start':
                slot_open = true
                if occ_depth == 0:
                    free_start = time
            'slot_end':
                if occ_depth == 0 and free_start is not None and free_start < time:
                    result.append(Slot { free_start, time, schedule_id })
                slot_open = false
                free_start = None
            'occ_start':
                if occ_depth == 0 and slot_open and free_start and free_start < time:
                    result.append(Slot { free_start, time, schedule_id })
                    free_start = None
                occ_depth += 1
            'occ_end':
                occ_depth -= 1
                if occ_depth == 0 and slot_open:
                    free_start = time

    return result
```

**Complexity**: O((S + G·F) log(S + F)) time, O(S + F) space, where
G is the number of schedules — slots split across groups (summing to S), while
each occupied interval is replicated into every group's event list. G is a
small constant in practice, so this remains O((S + F) log(S + F)) effectively.

### 4.1 Minute-Grid Alignment — `align_slots_to_grid`

Runs once, immediately after all occupied-interval subtraction, at the single
point where both pipelines build their slot pool (`ReschedulePrep::load`).
Snaps every free slot inward to the minute grid (`SLOT_GRID_MINUTES` in
`slot_finder.rs`, value 1 — the ONE definition of the chunk-time
granularity policy): starts round up, ends round down, slots shorter than the
grid are dropped.

```
function align_slots_to_grid(slots) -> Vec<Slot>:
    result = []
    for slot in slots:                                      # O(S')
        start = ceil_to_grid(slot.start, SLOT_GRID_MINUTES)
        end   = floor_to_grid(slot.end, SLOT_GRID_MINUTES)
        if start < end:
            result.append(Slot { start, end, slot.schedule_id })
    return result
```

**Why**: the engine performs only whole-minute arithmetic, so minute-aligned
slot boundaries guarantee every generated chunk is minute-aligned. Ragged
inputs enter from exactly two places: the `now` horizon clip (§3 clips the
first slot to the exact call time) and busy-interval edges (external events
carry second precision). Rounding inward never schedules before `now` and
never overlaps occupied time; sub-grid slots are unusable and dropped. The
engine's internal re-subtraction of the same fixed set (§5 step 3) removes
intervals already absent from the pool, so it cannot reintroduce ragged edges.

**Complexity**: O(S') time, O(S') space.

---

## 5. Core Scheduling Algorithm — `DefaultScheduler::schedule`

Priority-based greedy placement. Related to the **Weighted Job Scheduling** /
**Interval Scheduling Maximization** class of problems, but simplified because
tasks are chunked (not atomic) and we optimize for deadline compliance rather
than maximum utilization.

```
function schedule(input: ScheduleInput) -> ScheduleResult:
    tasks      = input.tasks
    fixed      = input.existing_fixed_chunks
    raw_slots  = input.available_slots
    now        = input.now
    max_cont   = input.max_continuous_minutes
    min_break  = input.min_break_minutes

    # ── Step 1: Compute remaining duration per task ──────
    fixed_by_task = group_by(fixed, key=task_id)            # O(F)
    for task in tasks:                                      # O(T)
        task.remaining = task.duration_minutes
                       - task.time_logged_minutes
                       - sum(c.duration_minutes for c in fixed_by_task[task.id])
        if task.remaining <= 0:
            task.remaining = 0

    # ── Step 2: Sort tasks by scheduling priority ────────
    # This ordering has exactly ONE code definition —
    # `scheduling_order` in traits/scheduling.rs — shared by the
    # full engine sort and the incremental cascade sort (§2.2); the
    # cascade's convergence argument depends on the two agreeing.
    sort(tasks, by=(                                        # O(T log T)
        -priority,          # higher priority first
        deadline ASC,       # earlier deadline first (None sorts last)
        remaining ASC,      # shorter remaining first
        title ASC           # lexicographic tiebreak
    ))

    # ── Step 3: Subtract fixed/completed chunks from slots ──
    free_slots = subtract_intervals(raw_slots, fixed)       # §4: O((S+F) log(S+F))

    # ── Step 4: Initialize placement state ───────────────
    placed    = []          # output chunks
    warnings  = []
    # Timeline: BTreeMap keyed by chunk end time; value is cumulative
    # continuous minutes ending at that point.
    timeline  = BTreeMap<DateTime, minutes>

    # ── Step 5: Place each task ──────────────────────────
    for task in tasks:
        if task.remaining <= 0:
            warn_if_fixed_past_deadline(task, fixed_by_task, warnings)
            continue

        # Skip tasks whose start_date is beyond the horizon (unless
        # a within-horizon deadline makes the deferral a conflict).
        if deferred_beyond_horizon(task, horizon_end):
            continue

        placed_before = len(placed)

        if task.no_split:
            place_no_split(task, free_slots, placed, warnings,
                           timeline, max_cont, min_break, now)
        else:
            place_splittable(task, task.remaining, free_slots,
                             placed, warnings,
                             timeline, max_cont, min_break, now)

        # Deadline check (O(1) — only scan newly placed chunks)
        if len(placed) > placed_before:
            latest_end = max(c.end for c in placed[placed_before:])
            if task.deadline is not None and latest_end > task.deadline:
                warnings.append(DeadlineViolation {
                    task_id: task.id,
                    deadline: task.deadline,
                    earliest_completion: latest_end
                })

    return ScheduleResult { placed, warnings }
```

### 5.1 Placing a no-split task

```
function place_no_split(task, free_slots, placed, warnings,
                        timeline, max_cont, min_break, now):
    duration = task.remaining
    for slot_idx in 0..len(free_slots):                    # O(S') worst case
        slot = free_slots[slot_idx]
        if not is_eligible(slot, task):
            continue
        # Determine effective start (respecting break)
        eff_start = apply_break(slot.start, timeline, max_cont, min_break)
        available = slot.end - eff_start
        if available < duration:
            continue    # doesn't fit in this slot

        # Place the chunk
        chunk = Chunk {
            task_id: task.id,
            start: eff_start,
            end:   eff_start + duration,
            is_fixed: false,
            status: Scheduled
        }
        placed.append(chunk)
        consume_slot(free_slots, slot_idx, eff_start, eff_start + duration)
        update_timeline(timeline, eff_start, eff_start + duration,
                        duration, max_cont)                 # O(log C)
        return

    # No slot large enough
    warnings.append(Unschedulable {
        task_id: task.id,
        reason: "no single slot large enough for no-split task"
    })
```

### 5.2 Placing a splittable task

```
function place_splittable(task, remaining, free_slots, placed, warnings,
                          timeline, max_cont, min_break, now):
    slot_idx = 0
    while remaining > 0 and slot_idx < len(free_slots):
        slot = free_slots[slot_idx]
        if not is_eligible(slot, task):
            slot_idx += 1
            continue

        # Determine effective start (respecting break)
        eff_start = apply_break(slot.start, timeline, max_cont, min_break)
        if eff_start >= slot.end:
            slot_idx += 1
            continue

        available_in_slot = slot.end - eff_start

        chunk_dur = min(remaining, available_in_slot)

        # Cap at remaining continuous-work budget: max_cont minus the
        # cumulative minutes carried forward from the adjacent predecessor.
        cont_budget = max_cont - cumulative_at(timeline, eff_start)
        chunk_dur = min(chunk_dur, cont_budget)

        # Enforce minimum chunk size (except for the final sub-floor remainder)
        if chunk_dur < task.min_chunk_minutes:
            if remaining <= task.min_chunk_minutes:
                # Final-chunk exception: allow the sub-floor remainder only when
                # (a) the slot is at least min_chunk_minutes wide — ensuring it
                # would hold a full chunk, not a micro-sliver — and (b) the
                # continuous-work budget is not exhausted (chunk_dur > 0).
                if available_in_slot < task.min_chunk_minutes or chunk_dur <= 0:
                    slot_idx += 1
                    continue
            else:
                # Slot too small for a meaningful chunk, skip it
                slot_idx += 1
                continue

        # Place the chunk
        chunk = Chunk {
            task_id: task.id,
            start: eff_start,
            end:   eff_start + chunk_dur,
            is_fixed: false,
            status: Scheduled
        }
        placed.append(chunk)
        remaining -= chunk_dur
        consume_slot(free_slots, slot_idx, eff_start, eff_start + chunk_dur)
        update_timeline(timeline, eff_start, eff_start + chunk_dur,
                        chunk_dur, max_cont)
        # NOTE: consume_slot may split slot at slot_idx into fragments,
        # so do NOT increment slot_idx — re-examine the same index
        # (which holds a fragment or the next slot after removal).

    if remaining > 0:
        warnings.append(Unschedulable {
            task_id: task.id,
            reason: "insufficient slot capacity for remaining duration"
        })
```

### 5.3 Break enforcement and eligibility

```
function is_eligible(slot, task) -> bool:
    return slot.schedule_id == task.schedule_id
       and (task.start_date is None or slot.start >= task.start_date)

function deferred_beyond_horizon(task, horizon_end) -> bool:
    # A task with start_date past the horizon is skipped without a warning —
    # it belongs to a later window. Exception: if its deadline falls within
    # the horizon, the conflict must surface.
    if task.start_date is None: return false
    return task.start_date > horizon_end
       and (task.deadline is None or task.deadline > horizon_end)

function apply_break(slot_start, timeline, max_cont, min_break) -> DateTime:
    # Find the most recent entry whose end time is ≤ slot_start
    prev = timeline.range(..=slot_start).last()             # O(log C)

    if prev is None:
        return slot_start   # no predecessor

    if prev.end == slot_start and prev.cumulative >= max_cont:
        return slot_start + min_break   # adjacent and budget exhausted — force break

    return slot_start                   # gap exists or budget not yet exhausted

function update_timeline(timeline, chunk_start, chunk_end, chunk_dur, max_cont):
    cumulative = chunk_dur
    prev = timeline.range(..=chunk_start).last()            # O(log C)
    if prev is not None and prev.end == chunk_start:
        cumulative = prev.cumulative + chunk_dur

    # Cap at max_cont to keep the timeline bounded (a no_split task may
    # exceed max_cont; capping here ensures subsequent apply_break calls
    # see a clean budget reset).
    cumulative = min(cumulative, max_cont)

    timeline.insert(chunk_end, cumulative)                  # O(log C)
```

### 5.4 Slot consumption

```
function consume_slot(free_slots, slot_idx, chunk_start, chunk_end):
    # The calling loop already knows which slot the chunk falls in —
    # no binary search needed.
    slot = free_slots.remove(slot_idx)                      # O(S') for array shift

    # Split into up to 2 fragments, inserted at slot_idx to maintain order.
    insert_pos = slot_idx
    if slot.start < chunk_start:
        free_slots.insert(insert_pos, Slot { slot.start, chunk_start, slot.schedule_id })
        insert_pos += 1
    if chunk_end < slot.end:
        free_slots.insert(insert_pos, Slot { chunk_end, slot.end, slot.schedule_id })
```

### 5.5 Core algorithm complexity

| Operation | Per-invocation | Invocations | Total |
|-----------|---------------|-------------|-------|
| Task sorting | O(T log T) | 1 | O(T log T) |
| Fixed grouping | O(F) | 1 | O(F) |
| Slot subtraction | O((S+F) log(S+F)) | 1 | O((S+F) log(S+F)) |
| Place no-split (scan + filter) | O(S') | T_nosplit | O(T_nosplit × S') |
| Place splittable (scan + filter + consume) | O(S') per chunk | C_splittable | O(C × S') |
| Break check (timeline lookup) | O(log C) | C | O(C log C) |
| Slot consumption (array splice) | O(S') | C | O(C × S') |
| Deadline check | O(1) | T | O(T) |

**Overall**: O(T log T + (S+F) log(S+F) + C × S')

**Space**: O(S' + T + C) — free slot list + task list + placed chunks + timeline

**Practical note**: With T ≈ 100, S' ≈ 200, C ≈ 300, this is ~60K operations — trivially fast.

**Optimization**: If S' becomes large, replace the free-slot array with a balanced BST
(e.g., `BTreeMap<DateTime, Slot>`) to reduce `consume_slot` from O(S') to O(log S'),
bringing the total to O(C log S' + T log T). Not needed at current scale.

---

## 6. Eligible Slot Filtering — Optimization Note

> **Not implemented** — the engine uses the naive O(S') per-task linear scan with `is_eligible` filtering; no pre-grouping exists.

The naive O(S') per-task filter can be avoided by pre-grouping slots:

```
slots_by_schedule = group_by(free_slots, key=schedule_id)   # O(S')
```

Then for each task, iterate only `slots_by_schedule[task.schedule_id]`.
This reduces the effective S' per task to S'_k (slots for that schedule),
and the total work across all tasks to O(S') (each slot scanned once per
task assigned to its schedule).

---

## 7. Recurring Instance Generation

Recurring instances are produced by a `Cadence`-owned **occurrence iterator**
and maintained by a single-pass, two-pointer **reconciliation** that reuses
existing instances in place (patch by `id`) rather than delete-and-recreate.
In-place reuse is deliberate: every recreated instance is a deleted+recreated
Google Calendar event (lost mapping, sync drift), so the algorithm minimizes
churn by construction.

### 7.1 Cadence Model — `domain/cadence.rs`

A `Cadence` has three fields:

| Field | Type | Meaning |
|-------|------|---------|
| `period` | `Period` (Weekly / Monthly) | Base recurrence period |
| `interval` | `u8` (≥ 1) | Every N periods |
| `windows` | `Vec<Window>` | In-period day spans (sorted, non-overlapping) |

A `Window { start: u8, end: u8 }` is a contiguous range of in-period day
offsets, inclusive and 0-indexed from the period's first day (Monday for
weekly, the 1st for monthly). One window produces one instance per active
period, schedulable across `start..=end`.

Invariants (enforced by `Cadence::new`, also on deserialization via `try_from`):
- `interval ≥ 1`, `windows` non-empty
- Each window: `start ≤ end`, `end ≤ max_offset` (6 weekly, 27 monthly)
- Windows sorted by start, non-overlapping (touching is illegal)

Weekly and monthly differ only in how a period is located and advanced
(`Period::floor`, `Period::next`); window resolution and the anchor filter
are uniform across both.

### 7.2 `Cadence::occurrences` — the iterator

`occurrences(start_date, tz)` yields `Occurrence { start, deadline }` ascending
from `start_date` forward, lazily and without bound.

```
function Cadence.occurrences(start_date, tz) -> lazy [Occurrence]:
    period_start = period.floor(start_date)    # Monday of anchor's week, or 1st of month
    idx = 0

    loop forever:
        if idx >= len(windows):
            period_start = period.next(period_start, interval)
            idx = 0

        window = windows[idx]
        idx += 1

        first = period_start + window.start days
        effective_end = match period:
            Monthly:
                # Extend the schedulable span to fill the gap to the next
                # window (or the period max = 27 / the 28th), so a missed
                # occurrence stays visible within the month.
                if idx < len(windows):
                    ceiling = windows[idx].start - 1
                else:
                    ceiling = 27   # period.max_offset() for Monthly
                ceiling
            Weekly:
                window.end

        last = period_start + effective_end days
        deadline = end_of_day(last, tz)            # 23:59:59 local → UTC
        if deadline < start_date:
            continue                               # never before the anchor

        start = max(start_of_day(first, tz), start_date)   # clamp up to anchor
        yield Occurrence { start, deadline }
```

**Monthly extension**: each monthly window's `deadline` is widened to the last
guaranteed day of the period (offset 27 / the 28th), or to the day before the
next window's start when multiple windows share the period. This prevents overlap
and keeps a missed instance schedulable within its period rather than silently
disappearing.

**Complexity**: O(1) per yielded occurrence (constant-time period/window arithmetic).

### 7.3 Expiry and Reuse Policies

Two period-aware methods on `Cadence`:

**`deadline_for_reuse(stored, occ_deadline)`** — determines what deadline to write
when an instance is reused by the reconcile's ‹D2› path:
- **Monthly**: always `occ_deadline` (the widened span is authoritative).
- **Weekly**: `stored` if present (preserving a user override), else `occ_deadline`.

**`expiry_for_occurrence(occ, next_start, tz)`** — determines `expire_at`:
- **Weekly**: `end_of_day(next_start, tz)` — end of the next occurrence's first
  day, capping the overdue overlap at one day (M4.5). `None` if no next occurrence.
- **Monthly**: `occ.deadline` — the instance expires at the end of its own widened
  span, cancelling it promptly at month-end rather than letting it linger.

### 7.4 `reconcile` — single-pass two-pointer

`reconcile` converges in-window instances with the cadence. It walks desired
occurrences and existing instances together, each cursor advancing monotonically.
Open instances are **reused by `id`** (no GCal churn) with their existing timing
preserved; null timing is backfilled from the cadence. Create/delete happen only
on a count mismatch.

Cadence and anchor changes are handled by `update_template` **before** reconcile
runs: all future open unpinned instances (`deadline > now`) are deleted atomically
so reconcile generates from scratch rather than repositioning into wrong slots.
Pinned, closed, and overdue instances survive this pre-deletion.

```
function reconcile(store, template, now, horizon, tz):
    if not template.is_active:                            # D0: deactivated — clean open unpinned
        delete all open unpinned instances; return

    desired = template.cadence.occurrences(template.start_date, tz)
                .skip_while(deadline <= now)
                .take_while(deadline <= horizon)          # ascending, finite
                + ONE lookahead occurrence past horizon   # → expire_at of the last slot (D4)
    inst    = store.instances(template.id)
                .filter(deadline > now).sort(deadline, created_at)   # all statuses, ascending

    o = 0; i = 0
    while i < len(inst):
        T = inst[i]
        if T.status in {Completed, Cancelled} or T.is_pinned:  # D1: sticky — owns its slot(s)
            anchor = T.start_date ?? T.deadline           # immune to deadline widening and user overrides
            while o < len(desired) and desired[o].start <= anchor: o += 1
            if not T.status in {Completed, Cancelled}:    # pinned: refresh identity only
                patch T -> { title, priority, labels, schedule_id } from template
            i += 1
        else if o < len(desired):                         # reuse open instance by id
            d = desired[o]
            # Preserve existing timing; backfill only if null (legacy rows).
            # deadline is refreshed through the cadence's period-aware policy:
            # monthly always uses the widened occurrence deadline (period-aware ceiling);
            # weekly preserves any user override (T.deadline if set, else d.deadline).
            patch T -> { start_date: T.start_date ?? d.start,
                         deadline:   cadence.deadline_for_reuse(T.deadline, d.deadline),
                         expire_at:  d.expire_at,
                         title, duration, priority, labels, schedule_id,
                         min_chunk_minutes: duration, no_split: true }  # no churn ‹D2›
            o += 1; i += 1
        else:                                             # D3: surplus open instance, no occurrence
            delete T and its chunks
            i += 1
    while o < len(desired):                               # merge tail: occurrences with no instance
        create instance_from_template(template, desired[o].start, desired[o].deadline, desired[o].expire_at)
        o += 1
```

No reschedule is triggered here (D2); conflicts from a moved deadline are settled
by the scheduler pass that runs after generation.

**Properties.** Unchanged cadence → reuse with no timing write (idempotent, zero
writes when nothing changed). Cadence/anchor change → `update_template` deletes
future open unpinned; next reconcile generates from scratch (GCal churn on those
instances; pinned/closed/overdue survive). Completed/Cancelled instances are
consumed (`≤`) and never recreated or reversed. **Complexity**: O(D + I) time,
O(D) space (D = desired in window, I = existing instances).

### 7.5 Orchestration

Generation precedes scheduling so a moved deadline is resolved before any
calendar sync:

```
on reschedule_requested(now):
    horizon = now + planning_horizon_days
    for template in active_templates: reconcile(store, template, now, horizon, tz)
    auto_cancel_overdue(store, now)        # past-side sweep
    release_stale_fixed_locks(store, now)  # unlock missed fixed chunks
    run_scheduler()                        # D2: resolve conflicts AFTER generation
    # A plain reschedule ends here. Only `sync_now` (Sync button, REST
    # sync-now) continues with the push phase — a single diff pass.
```

All active templates are reconciled on every reschedule (simple; the per-template
cost is small and can be made incremental later if measured slow).

### 7.6 `get_orphaned_template_instances`

For each active template with no pending/scheduled instance, the virtual "next"
instance is the head of the iterator: `template.cadence.occurrences(start_date,
tz)` advanced to the first occurrence with `deadline >= now` (deadline only; the
virtual task is never persisted, M9.5).

### 7.7 `auto_cancel_overdue`

`expire_at` is denormalized onto each instance, so the past-side sweep needs no
template lookup. User-pinned instances (`is_pinned = true`) are exempt and never
auto-cancelled.

```
function auto_cancel_overdue(store, now):
    for instance in store.list_tasks(statuses: [Pending, Scheduled]):   # O(I)
        if instance.recurring_template_id is None: continue
        if instance.is_pinned: continue                    # pinned instances are exempt
        if instance.expire_at is None: continue            # never expires
        if now <= instance.expire_at: continue
        delete scheduled chunks for instance
        instance.status = Cancelled                        # preserve as history
        store.update_task(instance)
```

**Complexity**: O(I) time, O(1) extra space.

---

## 8. Reschedule Diff Algorithm

Matches old auto-chunks to new placed chunks to minimize churn (preserving
`google_event_id` where possible).

```
function diff_chunks(old_chunks, new_chunks) -> Vec<DiffOp>:
    old_by_task = group_by(old_chunks, key=task_id)         # O(C_old)
    new_by_task = group_by(new_chunks, key=task_id)         # O(C_new)

    ops = []
    all_task_ids = union(old_by_task.keys(), new_by_task.keys())

    for task_id in all_task_ids:
        old = sort(old_by_task[task_id], by=start_time)     # O(c log c) per task
        new = sort(new_by_task[task_id], by=start_time)

        # Greedy pairing by closest start time
        paired_old = set()
        paired_new = set()

        for ni in 0..len(new):
            best_match = None
            best_dist  = infinity
            # Scan all unpaired old chunks for closest match.
            # Both lists are sorted, so once distance starts increasing
            # past the current best, no subsequent entry can be closer.
            for oj in 0..len(old):
                if oj in paired_old:
                    continue
                dist = abs(new[ni].start - old[oj].start)
                if dist < best_dist:
                    best_dist = dist
                    best_match = oj
                elif dist > best_dist:
                    break   # sorted → distance only increases from here
            if best_match is not None:
                paired_old.add(best_match)
                paired_new.add(ni)
                if old[best_match].start == new[ni].start
                   and old[best_match].end == new[ni].end:
                    ops.append(KEEP {
                        chunk_id: old[best_match].id,
                    })
                else:
                    # UPDATE reuses the old chunk's DB row and google_event_id;
                    # the scheduler's newly generated ID for this chunk is discarded.
                    ops.append(UPDATE {
                        chunk_id: old[best_match].id,
                        new_start: new[ni].start,
                        new_end: new[ni].end,
                        google_event_id: old[best_match].google_event_id
                    })

        # Unpaired old → DELETE
        for oi in 0..len(old):
            if oi not in paired_old:
                ops.append(DELETE { chunk_id: old[oi].id })

        # Unpaired new → CREATE
        for ni in 0..len(new):
            if ni not in paired_new:
                ops.append(CREATE { chunk: new[ni] })

    return ops
```

**Complexity**: O(C log C) dominated by the per-task sorts.
The greedy pairing within each task scans old chunks for each new chunk,
breaking early once distances increase. Worst case O(c_old × c_new) per
task, but O(c_old + c_new) typical (when chunks shift uniformly and the
early break fires quickly). Across all tasks: O(C_old + C_new) typical,
O(C²) worst case.

**Space**: O(C_old + C_new) for grouped lists and ops.

---

## 9. Google Calendar API Call Analysis

### 9.1 Two-layer diff minimizes API calls

The design uses a two-layer strategy to minimize GCal mutations:

**Layer 1 — Reschedule diff** (§8 above, runs locally):
Matches old auto-chunks to new placed chunks by task_id + closest start time.
A chunk that shifted 30 minutes becomes an UPDATE (preserving `google_event_id`)
instead of DELETE + CREATE. This runs entirely in-memory — zero API cost.

**Layer 2 — Sync diff** (sync::Service, runs against GCal):
Compares local chunks (with `google_event_id`) against remote events.
Only actual differences trigger API calls.

### 9.2 API calls per scenario (C=60 auto-chunks)

| Scenario | Layer 1 result | GCal API calls |
|----------|----------------|----------------|
| **Idempotent reschedule** (nothing changed) | 60 KEEP | 1 list + 0 mutations = **1** |
| **1 new task added** | 55 KEEP, 2 UPDATE, 3 CREATE | 1 list + 5 mutations = **6** |
| **Priority reorder** | 30 KEEP, 20 UPDATE, 5 DELETE, 5 CREATE | 1 list + 30 = **31** |
| **Schedule windows changed** (worst case) | 60 DELETE, 60 CREATE | 1 list + 120 = **121** |

### 9.3 Batch optimization

Mutations are pushed through Google's batch endpoint (`POST /batch/calendar/v3`,
`multipart/mixed`): every create/update/delete becomes one inner request inside a
single HTTP round-trip. Google allows up to 1000 calls per batch; we cap at
`BATCH_MAX_OPS = 250` so a retried batch stays well within the ~600 requests/min/user
rate window and per-request latency/memory stay bounded.

```
function batch_sync_ops(ops):
    BATCH_MAX_OPS = 250
    for batch in ops.chunks(BATCH_MAX_OPS):        # ceil(len(ops) / 250) round-trips
        pending = batch
        for attempt in 1..=MAX_ATTEMPTS:
            resp = POST /batch/calendar/v3 (pending)     # one multipart request
            pending = inner parts throttled(403 usageLimits / 429) or transient(5xx)
            if pending is empty: break
            sleep(exp_backoff_with_jitter(attempt))      # honor Retry-After if sent
        permanent inner failure (e.g. 400) ⇒ fail the whole sync   # Phase B reconciles
```

| Scenario | Without batching | With batching |
|----------|-----------------|---------------|
| 6 mutations | 6 HTTP requests | 1 list + **1 batch** = 2 |
| 31 mutations | 31 HTTP requests | 1 list + **1 batch** = 2 |
| 121 mutations | 121 HTTP requests | 1 list + **1 batch** = 2 |
| 600 mutations | 600 HTTP requests | 1 list + **3 batches** = 4 |

Throttled (403 `usageLimits`/`rateLimitExceeded`, 429) and transient (5xx) inner
parts are retried with truncated exponential backoff + jitter (64s ceiling); a
`Retry-After` header is honored when present, though Calendar rarely sends one. Each
batch is all-or-nothing to the caller: a permanent inner failure fails the whole sync
and Phase B (orphan reconciliation) cleans up any partially-applied effects next run.

### 9.4 Steady-state API usage

In typical daily use (a few task completions, occasional reschedule):
- Sync runs on demand only (Sync button, REST sync-now); there is no sync timer,
  so a reschedule costs no API calls until the user syncs
- Most syncs: 1 list_events + 1 batch (≤ 250 changes) = **2 HTTP requests**
- GCal API quota: 1,000,000 queries/day — we'll use < 100/day

### 9.5 [S6] Busy-time read cost

If conflict avoidance (S6) is enabled:
- Reschedule reads the local `external_events` mirror (step 7a, busy=1 only) — **0 API calls** per reschedule
- The mirror is refreshed by `pull_external_events`: one `events.list` call per selected calendar per pull
- Pulls are on demand: the Pull button in Settings, REST calendar-pull, or the first
  phase of sync-now. **Not implemented** — a periodic (hourly) background pull; the
  daily cost is therefore one `events.list` per selected calendar per manual pull

---

## 10. Overall Complexity Summary

| Pipeline Step | Time Complexity | Space |
|---------------|----------------|-------|
| 1. Get config | O(1) | O(1) |
| 2. Generate recurring instances | O(R × D) | O(D) |
| 3. Auto-cancel overdue | O(I) | O(1) |
| 3a. Release stale fixed locks | O(F_sched) | O(F_sched) |
| 6. Get fixed chunks | O(F) | O(F) |
| 7. Expand schedule windows | O(H × W + S log S) | O(S) |
| 7a. Subtract busy times | O((S+B) log(S+B)) | O(S+B) |
| 7b. Align to grid | O(S') | O(S') |
| 4. Get schedulable tasks | O(T) | O(T) |
| 5. Get old auto-chunks | O(C_old) | O(C_old) |
| **8. Core scheduler** | **O(T log T + C × S')** | **O(S' + C)** |
| 8a. Filter warnings | O(T + W_count) | O(T) |
| 9. Diff old vs new | O(C log C) | O(C) |
| 10. Update task statuses | O(T) | O(1) |
| 11. Update config | O(1) | O(1) |

**Full reschedule**: O(T log T + C × S' + R × D)

**Dominant term**: C × S' (chunk placement loop scanning free slots).
At typical scale (C ≈ 300, S' ≈ 300): ~90K comparisons. Trivially fast.

**Incremental reschedule**: O(T log T + T_aff × S') where T_aff = affected tasks.
For edits/completions T_aff ≈ 1–3; for high-priority deletions T_aff can approach T
(see §2.2 convergence note). Typical case: O(T log T). Sub-millisecond.

| Mode | Typical wall time | GCal API calls at the next sync-now |
|------|------------------|----------------|
| Full reschedule | 5–50ms | 1 list + 1 batch (≤250 ops) = **2** |
| Incremental (1 task, no cascade) | < 1ms | 1 list + 1 batch (1–3 ops) = **2** |
| Incremental (cascade to 3 tasks) | 1–2ms | 1 list + 1 batch (5–10 ops) = **2** |

**Space**: O(T + S' + C + F + I) — all linear in input/output size.

---

## 11. Algorithmic Properties

### 11.1 Relationship to classical problems

The scheduling problem is a variant of **offline bin-packing with item splitting**:
- **Bins** = available time slots (variable size)
- **Items** = tasks with required durations (splittable or not)
- **Constraints** = priority ordering, deadline compliance, schedule affinity, break enforcement

Classical bin-packing is NP-hard, but our variant is tractable because:
1. Items are processed in a fixed priority order (not optimized)
2. Splittable items can fill any combination of bins (relaxes the hard constraint)
3. We optimize for **deadline compliance** and **minimum chunk count**, not minimum bins used

### 11.2 Optimality guarantees

**What the greedy algorithm guarantees**:
- Higher-priority tasks always get first pick of slots (priority ordering is strict)
- Within a task, chunks are placed as early as possible (chronological first-fit, capped at max_continuous)
- Deadline violations are detected and reported

**What it does NOT guarantee**:
- **Global optimality**: A lower-priority task might miss its deadline even though
  a feasible schedule exists (by rearranging higher-priority tasks). This is by design —
  priority always wins over deadline optimization.
- **Minimum total slots used**: The greedy scan may fragment slots in ways that a
  more sophisticated algorithm could avoid. In practice this is negligible.
- **Minimum chunk count**: Chronological first-fit may create a small chunk in an
  early slot when a larger slot exists later. A largest-fit-first strategy would
  reduce chunk count, but at the cost of pushing work later. Earlier placement is
  preferred for a task scheduler (do it sooner, not fewer pieces).
- **Fairness**: Tasks of equal priority are ordered by deadline, not by remaining
  slack. A task with plenty of slack but an earlier deadline gets scheduled first.

### 11.3 Break enforcement correctness

The break enforcement tracks **cumulative continuous time** as a property of
timeline positions (`BTreeMap<DateTime, i64>`), not individual tasks. This
correctly handles the case where task A's last chunk and task B's first chunk
are adjacent — the cumulative time spans both.

**Invariant**: At any point in the placed timeline, no contiguous sequence of
chunks (possibly from different tasks) exceeds `max_continuous_minutes` without
a gap of at least `min_break_minutes`.

**Exception**: A `no_split` task longer than `max_continuous_minutes` is placed as a
single block (violating the max), but a break is enforced after it.
`update_timeline` caps cumulative at `max_cont` so the next `apply_break` call
sees a clean budget-exhausted state.

### 11.4 Idempotency

Rescheduling is **not idempotent** in general — running it twice may produce different
results if the timestamp advances between runs (new "now" changes horizon, auto-cancels
different instances, etc.). However, running it twice at the same logical timestamp
on the same data produces identical output (the algorithm is deterministic).
