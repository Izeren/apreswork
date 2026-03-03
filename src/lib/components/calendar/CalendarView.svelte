<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import type {
    AgendaItem,
    AppConfig,
    CreateTaskInput,
    ExternalEvent,
    ScheduleWindow,
    Task,
    UpdateTaskInput,
    UserEventPayload,
  } from '../../types';
  import {
    getWeekStart,
    getWeekEnd,
    toStartOfDayISO,
    toEndOfDayISO,
    formatDayHeader,
    formatWeekHeader,
    startOfTodayMs,
    addDaysMs,
    DAYS_PER_WEEK,
  } from '../../utils';
  import DayView from './DayView.svelte';
  import WeekView from './WeekView.svelte';
  import { defaultCalendarApi } from './calendarViewShared';
  import type { CalendarApi, CalendarViewCommonProps } from './calendarViewShared';
  import GoogleReconnectHint from './GoogleReconnectHint.svelte';
  import CompleteChunkDialog from './CompleteChunkDialog.svelte';
  import EventDialog, { type EventDialogInitial } from './EventDialog.svelte';
  import TaskForm from '../tasks/TaskForm.svelte';
  import ContextMenu from '../shared/ContextMenu.svelte';
  import ConfirmHostDialog from '../shared/ConfirmHostDialog.svelte';
  import RescheduleButton from '../shared/RescheduleButton.svelte';
  import Modal from '../shared/Modal.svelte';
  import { CompletionFlow } from './completionFlow.svelte';
  import { chunkContextMenuItems, TaskActions } from '../../actions/taskActions';
  import { createConfirmHost } from '../../actions/confirmHost.svelte';
  import { runReschedule } from '../../actions/rescheduleTrigger';
  import { runSync } from '../../actions/syncTrigger';
  import { toastState } from '../../stores/toast.svelte';
  import { scheduleState, ScheduleState } from '../../stores/schedules.svelte';
  import { taskState } from '../../stores/tasks.svelte';
  import { calendarFocusState } from '../../stores/calendarFocus.svelte';
  import { router } from '../../router.svelte';
  import { registerShortcuts } from '../../shortcuts.svelte';

  type ViewMode = 'day' | 'week';

  interface Props {
    apiClient?: CalendarApi;
    getNow: () => Date;
    schedulesStore?: ScheduleState;
  }

  const {
    apiClient = defaultCalendarApi,
    getNow,
    schedulesStore: schedulesStoreProp,
  }: Props = $props();
  const effectiveSchedule = $derived(schedulesStoreProp ?? scheduleState);

  let currentTime: Date = $state(untrack(() => getNow()));

  let mode: ViewMode = $state('week');
  // untrack: one-shot mount capture; subsequent navigation mutates currentTimestamp directly
  // number (not Date) avoids svelte/prefer-svelte-reactivity on the $derived currentDate reads.
  let currentTimestamp: number = $state(untrack(() => startOfTodayMs(currentTime)));
  let agendaItems: AgendaItem[] = $state([]);
  let externalEvents: ExternalEvent[] = $state([]);
  let loading: boolean = $state(false);
  let rescheduling: boolean = $state(false);
  let syncing: boolean = $state(false);
  let calendarConnected: boolean = $state(false);
  let googleDisconnected: boolean = $state(false);
  let appConfig: AppConfig | null = $state(null);
  let configLoaded: boolean = $state(false);
  let formOpen: boolean = $state(false);
  let editingTask: Task | null = $state(null);
  let pendingCreateSlot: { start: string; end: string } | null = $state(null);
  // Date-only prefill for shortcut-triggered task creation. Deliberately separate
  // from pendingCreateSlot: that path creates a FIXED chunk on submit, which is
  // wrong semantics for a shortcut-created task where no specific time was chosen.
  let pendingCreateDate: string | null = $state(null);

  /** The primary calendar's id once known; gates event editing/creation. Null ⇒ off. */
  let editableCalendarId: string | null = $state(null);
  let eventDialogOpen: boolean = $state(false);
  let eventDialogMode: 'create' | 'edit' = $state('create');
  let eventDialogInitial: EventDialogInitial = $state({
    title: '',
    description: null,
    start: '',
    end: '',
    all_day: false,
  });
  /** The event being edited (carries the ids for the write); null in create mode. */
  let editingEvent: ExternalEvent | null = $state(null);
  let eventBusy: boolean = $state(false);
  let eventError: string | null = $state(null);

  let slotChooserOpen: boolean = $state(false);
  let chooserSlot: { start: string; end: string } | null = $state(null);

  const completion = new CompletionFlow(
    () => loadAgenda(),
    untrack(() => apiClient),
  );

  const browserTimezone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';

  const allWindows: ScheduleWindow[] = $derived(effectiveSchedule.items.flatMap((s) => s.windows));

  const currentDate = $derived(new Date(currentTimestamp));

  const rangeStart = $derived.by(() => {
    if (mode === 'day') return toStartOfDayISO(currentDate);
    return getWeekStart(currentDate).toISOString();
  });

  const rangeEnd = $derived.by(() => {
    if (mode === 'day') return toEndOfDayISO(currentDate);
    return getWeekEnd(currentDate).toISOString();
  });

  const headerLabel = $derived.by(() =>
    mode === 'day' ? formatDayHeader(currentDate) : formatWeekHeader(currentDate),
  );

  const timezoneMismatch = $derived.by(
    () => appConfig !== null && appConfig.timezone !== browserTimezone,
  );

  const displayDays = $derived.by((): Date[] => {
    if (mode === 'day') return [currentDate];
    const monday = getWeekStart(currentDate);
    return Array.from({ length: DAYS_PER_WEEK }, (_, i) => {
      const ts = addDaysMs(monday.getTime(), i);
      return new Date(ts);
    });
  });

  function goBack(): void {
    currentTimestamp = addDaysMs(currentTimestamp, mode === 'day' ? -1 : -DAYS_PER_WEEK);
  }

  function goForward(): void {
    currentTimestamp = addDaysMs(currentTimestamp, mode === 'day' ? 1 : DAYS_PER_WEEK);
  }

  /** Shift the visible week by ±1 — used by cross-week drag edge flipping. */
  function shiftWeek(direction: -1 | 1): void {
    currentTimestamp = addDaysMs(currentTimestamp, direction * DAYS_PER_WEEK);
  }

  function goToday(): void {
    currentTimestamp = startOfTodayMs(currentTime);
  }

  function setMode(m: ViewMode): void {
    mode = m;
  }

  function loadRange<T>(
    fetch: (start: string, end: string) => Promise<T>,
    onSuccess: (result: T) => void,
    onError: (e: unknown) => void,
  ): void {
    const start = rangeStart;
    const end = rangeEnd;
    fetch(start, end).then(onSuccess).catch(onError);
  }

  function loadAgenda(): void {
    loading = true;
    loadRange(
      (start, end) => apiClient.getAgenda(start, end),
      (items) => {
        agendaItems = items;
        loading = false;
      },
      (e) => {
        agendaItems = [];
        loading = false;
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to load agenda'));
      },
    );
  }

  function loadExternalEvents(): void {
    loadRange(
      (start, end) => apiClient.listExternalEvents(start, end),
      (events) => {
        externalEvents = events;
      },
      (e) => {
        externalEvents = [];
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to load calendar events'));
      },
    );
  }

  /**
   * Resolve the account's primary calendar id — the only calendar whose events
   * are editable. Failure is non-fatal: without an id, externals stay read-only
   * and empty slots create tasks only.
   */
  function loadEditableCalendar(): void {
    apiClient
      .googleListCalendars()
      .then((calendars) => {
        editableCalendarId = calendars.find((c) => c.primary)?.id ?? null;
      })
      .catch(() => {
        editableCalendarId = null;
      });
  }

  function loadConfig(): void {
    apiClient
      .getConfig()
      .then((config) => {
        appConfig = config;
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to load app config'));
      })
      .finally(() => {
        configLoaded = true;
      });
  }

  function handleChunkOp(op: () => Promise<unknown>, errorMsg: string): void {
    op()
      .then(() => {
        loadAgenda();
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, errorMsg));
      });
  }

  function handleChunkMove(chunkId: string, newStart: string, newEnd: string): void {
    handleChunkOp(() => apiClient.moveChunk(chunkId, newStart, newEnd), 'Failed to move chunk');
  }

  function handleChunkResize(chunkId: string, newEnd: string): void {
    handleChunkOp(() => apiClient.resizeChunk(chunkId, newEnd), 'Failed to resize chunk');
  }

  let menuOpen: boolean = $state(false);
  let menuItem: AgendaItem | null = $state(null);
  let menuX: number = $state(0);
  let menuY: number = $state(0);
  const confirmHost = createConfirmHost();

  const actions = new TaskActions(
    {
      refresh: () => loadAgenda(),
      confirm: confirmHost.request,
      openTaskEditor,
      openTemplateEditor: (templateId) => {
        taskState.requestTemplateEdit(templateId);
        router.navigate('tasks');
      },
    },
    untrack(() => apiClient),
  );

  const menuItems = $derived(menuItem ? chunkContextMenuItems(menuItem, actions, currentTime) : []);

  function openChunkMenu(item: AgendaItem, x: number, y: number): void {
    menuItem = item;
    menuX = x;
    menuY = y;
    menuOpen = true;
  }

  function handleChunkLock(item: AgendaItem): void {
    if (item.chunk.is_fixed) {
      actions.unlockChunk(item.chunk.id);
    } else {
      actions.lockChunk(item.chunk.id);
    }
  }

  function closeChunkMenu(): void {
    menuOpen = false;
  }

  function reschedule(): void {
    runReschedule(
      (busy) => (rescheduling = busy),
      () => loadAgenda(),
      apiClient,
    );
  }

  function syncCalendar(): void {
    runSync(
      (busy) => (syncing = busy),
      () => {
        // Sync pulls external events AND reschedules — both visible datasets change.
        loadAgenda();
        loadExternalEvents();
      },
      apiClient,
    );
  }

  function closeForm(): void {
    formOpen = false;
    editingTask = null;
    pendingCreateSlot = null;
    pendingCreateDate = null;
  }

  function openTaskEditor(taskId: string): void {
    apiClient
      .getTask(taskId)
      .then((task) => {
        pendingCreateSlot = null;
        editingTask = task;
        formOpen = true;
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to load task'));
      });
  }

  function openCreateTask(start: string, end: string): void {
    editingTask = null;
    pendingCreateSlot = { start, end };
    formOpen = true;
  }

  function slotDurationMinutes(start: string, end: string): number {
    const startMs = new Date(start).getTime();
    const endMs = new Date(end).getTime();
    return Math.max(5, Math.round((endMs - startMs) / 60_000));
  }

  function openEventDialog(
    event: ExternalEvent | null,
    mode: 'create' | 'edit',
    initial: EventDialogInitial,
  ): void {
    editingEvent = event;
    eventDialogMode = mode;
    eventDialogInitial = initial;
    eventError = null;
    eventDialogOpen = true;
  }

  function openEventEditor(event: ExternalEvent): void {
    openEventDialog(event, 'edit', {
      title: event.title,
      description: event.description,
      start: event.start_time,
      end: event.end_time,
      all_day: event.all_day,
    });
  }

  function openCreateEvent(start: string, end: string): void {
    openEventDialog(null, 'create', { title: '', description: null, start, end, all_day: false });
  }

  function closeEventDialog(): void {
    eventDialogOpen = false;
    editingEvent = null;
    eventError = null;
  }

  async function runEventOp(
    op: (calendarId: string) => Promise<unknown>,
    successMessage: string,
    failMessage: string,
  ): Promise<void> {
    if (editableCalendarId === null) return;
    const calendarId = editableCalendarId;
    eventBusy = true;
    eventError = null;
    try {
      await op(calendarId);
      closeEventDialog();
      // The write reschedules server-side; refetch both visible datasets (invariant 6).
      loadAgenda();
      loadExternalEvents();
      toastState.success(successMessage);
    } catch (e) {
      // syncErrorMessage surfaces sanitized calendar_sync reasons (e.g. HTTP 4xx).
      eventError = apiClient.syncErrorMessage(e, failMessage);
    } finally {
      eventBusy = false;
    }
  }

  async function handleEventSubmit(payload: UserEventPayload): Promise<void> {
    const target = editingEvent;
    await runEventOp(
      (calendarId) =>
        target
          ? apiClient.updateUserEvent(calendarId, target.event_id, payload)
          : apiClient.createUserEvent(calendarId, payload),
      target ? 'Event updated' : 'Event created',
      'Failed to save event',
    );
  }

  async function handleEventDelete(): Promise<void> {
    if (editingEvent === null) return;
    const target = editingEvent;
    await runEventOp(
      (calendarId) => apiClient.deleteUserEvent(calendarId, target.event_id),
      'Event deleted',
      'Failed to delete event',
    );
  }

  function handleSlotCreate(start: string, end: string): void {
    if (calendarConnected && editableCalendarId !== null) {
      chooserSlot = { start, end };
      slotChooserOpen = true;
      return;
    }
    openCreateTask(start, end);
  }

  function closeSlotChooser(): void {
    slotChooserOpen = false;
    chooserSlot = null;
  }

  function chooseAndCreate(handler: (start: string, end: string) => void): void {
    const slot = chooserSlot;
    closeSlotChooser();
    if (slot) handler(slot.start, slot.end);
  }

  async function handleTaskSubmit(input: CreateTaskInput | UpdateTaskInput): Promise<void> {
    if (editingTask) {
      try {
        await apiClient.updateTask(editingTask.id, input as UpdateTaskInput);
        closeForm();
        loadAgenda();
        toastState.success('Task updated');
      } catch (e) {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to update task'));
      }
      return;
    }

    const createInput = input as CreateTaskInput;
    try {
      const task = await apiClient.createTask(createInput);
      if (pendingCreateSlot) {
        const start = pendingCreateSlot.start;
        const end = new Date(
          new Date(start).getTime() + createInput.duration_minutes * 60_000,
        ).toISOString();
        await apiClient.createFixedChunk(task.id, start, end);
      }

      closeForm();
      loadAgenda();
      toastState.success('Task created');
    } catch (e) {
      toastState.error(apiClient.apiErrorMessage(e, 'Failed to create task'));
    }
  }

  const initialCreateDurationMinutes = $derived.by(() =>
    pendingCreateSlot ? slotDurationMinutes(pendingCreateSlot.start, pendingCreateSlot.end) : null,
  );

  const initialCreateStartDate = $derived.by(() => pendingCreateSlot?.start ?? pendingCreateDate);

  function openCreateForVisibleDay(): void {
    editingTask = null;
    pendingCreateSlot = null;
    pendingCreateDate = toStartOfDayISO(currentDate);
    formOpen = true;
  }

  function adoptBrowserTimezone(): void {
    apiClient
      .updateConfig({ timezone: browserTimezone })
      .then((config) => {
        appConfig = config;
        toastState.success(`Schedule timezone set to ${browserTimezone}`);
        loadAgenda();
      })
      .catch((e) => {
        toastState.error(apiClient.apiErrorMessage(e, 'Failed to update schedule timezone'));
      });
  }

  $effect(() => {
    void rangeStart;
    void rangeEnd;
    loadAgenda();
    loadExternalEvents();
  });

  $effect(() => {
    // Load schedules once on mount so that schedule windows are available.
    untrack(() => effectiveSchedule.load()).catch((e) => {
      toastState.error(apiClient.apiErrorMessage(e, 'Failed to load schedules'));
    });
  });

  $effect(() => {
    // Check Google connection once on mount — the Sync button only shows when
    // connected, and only then can we resolve the editable (primary) calendar.
    apiClient
      .googleAuthStatus()
      .then((s) => {
        calendarConnected = s.type === 'connected';
        googleDisconnected = s.type === 'not_connected';
        if (s.type === 'connected') loadEditableCalendar();
      })
      .catch(() => {
        // Visibility-only check — on failure the button simply stays hidden.
      });
  });

  $effect(() => {
    if (configLoaded) return;
    loadConfig();
  });

  const FOCUS_FLASH_MS = 2500;

  let lastHandledFocusNonce = 0;

  $effect(() => {
    const nonce = calendarFocusState.nonce;
    if (nonce === lastHandledFocusNonce) return;
    lastHandledFocusNonce = nonce;
    const start = calendarFocusState.startTime;
    if (!start) return;
    const startMs = new Date(start).getTime();
    if (Number.isNaN(startMs)) return;
    // rangeStart/rangeEnd normalize to the day/week, so the raw chunk start works
    // in both modes.
    currentTimestamp = startMs;
    window.setTimeout(() => {
      // A newer request owns the carrier now — leave it alone.
      if (calendarFocusState.nonce === nonce) calendarFocusState.clear();
    }, FOCUS_FLASH_MS);
  });

  $effect(() => {
    return registerShortcuts([
      { key: 't', description: 'Jump to today', group: 'Calendar', handler: goToday },
      { key: 'ArrowLeft', description: 'Previous day/week', group: 'Calendar', handler: goBack },
      { key: 'ArrowRight', description: 'Next day/week', group: 'Calendar', handler: goForward },
      { key: 'd', description: 'Day view', group: 'Calendar', handler: () => setMode('day') },
      { key: 'w', description: 'Week view', group: 'Calendar', handler: () => setMode('week') },
      {
        key: 'r',
        description: 'Reschedule now',
        group: 'Calendar',
        handler: () => {
          if (!rescheduling) reschedule();
        },
      },
      {
        key: 'n',
        description: 'New task (visible day)',
        group: 'Calendar',
        handler: openCreateForVisibleDay,
      },
    ]);
  });

  $effect(() => {
    const id = setInterval(() => {
      currentTime = getNow();
    }, 60_000);
    return () => clearInterval(id);
  });

  const showReconnectHint = $derived(googleDisconnected && externalEvents.length > 0);

  const commonViewProps: CalendarViewCommonProps = $derived({
    items: agendaItems,
    now: currentTime,
    windows: allWindows,
    externalEvents,
    editableCalendarId,
    disconnected: googleDisconnected,
    oneventopen: openEventEditor,
    onchunkopen: openTaskEditor,
    onchunkcomplete: (item) => completion.open(item),
    onchunkmove: handleChunkMove,
    onchunkresize: handleChunkResize,
    onchunkmenu: openChunkMenu,
    onchunklock: handleChunkLock,
    oncreatechunk: handleSlotCreate,
  });
</script>

<div class="calendar-view">
  <div class="toolbar" role="toolbar" aria-label="Calendar controls">
    <div class="mode-toggle" role="group" aria-label="View mode">
      <button
        class="mode-btn"
        class:active={mode === 'day'}
        onclick={() => setMode('day')}
        aria-pressed={mode === 'day'}
      >
        Day
      </button>
      <button
        class="mode-btn"
        class:active={mode === 'week'}
        onclick={() => setMode('week')}
        aria-pressed={mode === 'week'}
      >
        Week
      </button>
    </div>

    <div class="nav-group">
      <button class="nav-btn" onclick={goBack} aria-label="Previous">&#8249;</button>
      <span class="date-header">{headerLabel}</span>
      <button class="nav-btn" onclick={goForward} aria-label="Next">&#8250;</button>
    </div>

    <button class="today-btn" onclick={goToday}>Today</button>
    <RescheduleButton {rescheduling} onclick={reschedule} />
    {#if calendarConnected}
      <button
        class="sync-btn"
        onclick={syncCalendar}
        disabled={syncing}
        aria-label={syncing ? 'Syncing…' : 'Sync with Google Calendar'}
      >
        <span class="reschedule-icon" class:spinning={syncing}>&#x21bb;</span>
        {syncing ? 'Syncing…' : 'Sync'}
      </button>
    {/if}
  </div>

  {#if timezoneMismatch && appConfig}
    <div class="timezone-warning" role="status" aria-live="polite">
      <span>
        Schedule timezone is <strong>{appConfig.timezone}</strong>, but this device is using
        <strong>{browserTimezone}</strong>. That can make chunks appear outside the visible schedule
        windows.
      </span>
      <button class="timezone-fix-btn" onclick={adoptBrowserTimezone}>
        Use {browserTimezone}
      </button>
    </div>
  {/if}

  <GoogleReconnectHint
    visible={showReconnectHint}
    onreconnect={() => router.navigate('settings')}
  />

  {#if loading}
    <div class="loading-bar" aria-live="polite" aria-label="Loading calendar data">Loading...</div>
  {/if}

  <div class="calendar-body">
    {#if mode === 'week'}
      <WeekView days={displayDays} onweekflip={shiftWeek} {...commonViewProps} />
    {:else}
      <DayView date={displayDays[0]} {...commonViewProps} />
    {/if}
  </div>
</div>

<TaskForm
  open={formOpen}
  task={editingTask}
  initialDurationMinutes={initialCreateDurationMinutes}
  initialStartDate={initialCreateStartDate}
  onsubmit={handleTaskSubmit}
  onclose={closeForm}
  onchunkschange={loadAgenda}
/>
<CompleteChunkDialog
  open={completion.dialogOpen}
  taskTitle={completion.item?.task_title ?? 'this task'}
  selectedTarget={completion.target}
  busy={completion.busy}
  onselecttarget={(target) => completion.selectTarget(target)}
  onconfirm={() => completion.confirm()}
  onclose={() => completion.close()}
/>
<EventDialog
  open={eventDialogOpen}
  mode={eventDialogMode}
  initial={eventDialogInitial}
  busy={eventBusy}
  error={eventError}
  {getNow}
  onsubmit={handleEventSubmit}
  ondelete={eventDialogMode === 'edit' ? handleEventDelete : null}
  oncancel={closeEventDialog}
/>
<Modal open={slotChooserOpen} title="Add to calendar" onclose={closeSlotChooser}>
  <div class="slot-chooser">
    <p class="slot-chooser-prompt">What would you like to create in this slot?</p>
    <div class="slot-chooser-actions">
      <button type="button" class="btn-primary" onclick={() => chooseAndCreate(openCreateTask)}
        >Task</button
      >
      <button type="button" class="btn-chooser" onclick={() => chooseAndCreate(openCreateEvent)}
        >Event</button
      >
    </div>
  </div>
</Modal>
<ContextMenu open={menuOpen} x={menuX} y={menuY} items={menuItems} onclose={closeChunkMenu} />
<ConfirmHostDialog host={confirmHost} />

<style>
  .calendar-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--spacing-4);
    padding: var(--spacing-3) var(--spacing-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg);
    flex-shrink: 0;
  }

  .mode-toggle {
    display: flex;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .mode-btn {
    border: none;
    border-radius: 0;
    padding: var(--spacing-1) var(--spacing-3);
    font-size: var(--font-size-sm);
    background: var(--color-surface);
    color: var(--color-text-secondary);
    transition: background var(--transition-fast);
  }

  .mode-btn:hover {
    background: var(--color-surface-hover);
  }

  .mode-btn.active {
    background: var(--color-primary);
    color: var(--color-text-inverse);
  }

  .nav-group {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    flex: 1;
    justify-content: center;
  }

  .nav-btn {
    border: none;
    background: transparent;
    font-size: var(--font-size-xl);
    color: var(--color-text-secondary);
    padding: var(--spacing-1) var(--spacing-2);
    border-radius: var(--radius-sm);
    line-height: 1;
  }

  .nav-btn:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .date-header {
    font-size: var(--font-size-base);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    min-width: 200px;
    text-align: center;
  }

  .today-btn {
    font-size: var(--font-size-sm);
    padding: var(--spacing-1) var(--spacing-3);
  }

  .sync-btn {
    padding: var(--spacing-1) var(--spacing-3);
  }

  .toolbar :global(.reschedule-btn) {
    padding: var(--spacing-1) var(--spacing-3);
  }

  .loading-bar {
    padding: var(--spacing-1) var(--spacing-4);
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    background: var(--color-bg-secondary);
    flex-shrink: 0;
  }

  .timezone-warning {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-3);
    padding: var(--spacing-2) var(--spacing-4);
    border-bottom: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-warning, #d97706) 10%, transparent);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .timezone-fix-btn {
    font-size: var(--font-size-xs);
    padding: var(--spacing-1) var(--spacing-2);
  }

  .calendar-body {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .slot-chooser {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-4);
  }

  .slot-chooser-prompt {
    margin: 0;
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
  }

  .slot-chooser-actions {
    display: flex;
    gap: var(--spacing-3);
  }

  .slot-chooser-actions > button {
    flex: 1;
  }

  .btn-chooser {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
  }

  .btn-chooser:hover:enabled {
    background: var(--color-surface-hover);
  }
</style>
