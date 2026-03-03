// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import type {
  ActiveProfile,
  AgendaItem,
  AppConfig,
  AuthStatus,
  BackupStatus,
  Chunk,
  Comment,
  CreateCommentInput,
  CreateScheduleInput,
  CreateTaskInput,
  CreateTemplateInput,
  ExternalCalendar,
  ExternalEvent,
  ProfileInfo,
  ProfileStatus,
  RecurringTemplate,
  Schedule,
  ScheduleResult,
  SyncOutcome,
  SyncStatus,
  Task,
  TaskFilter,
  UpdateConfigInput,
  UpdateScheduleInput,
  UpdateTaskInput,
  UpdateTemplateInput,
  UserEventPayload,
} from './types';

export function createTask(input: CreateTaskInput): Promise<Task> {
  return invoke('create_task', { input });
}

export function getTask(id: string): Promise<Task> {
  return invoke('get_task', { id });
}

export function updateTask(id: string, input: UpdateTaskInput): Promise<Task> {
  return invoke('update_task', { id, input });
}

export function deleteTask(id: string): Promise<void> {
  return invoke('delete_task', { id });
}

export function cancelTask(id: string): Promise<Task> {
  return invoke('cancel_task', { id });
}

export function completeTask(id: string): Promise<Task> {
  return invoke('complete_task', { id });
}

export function listTasks(filter?: TaskFilter): Promise<Task[]> {
  return invoke('list_tasks', { filter: filter ?? null });
}

export function getOrphanedTemplateInstances(): Promise<Task[]> {
  return invoke('get_orphaned_template_instances');
}

export function listChunksForTask(taskId: string): Promise<Chunk[]> {
  return invoke('list_chunks_for_task', { taskId });
}

export function listChunksInRange(start: string, end: string): Promise<Chunk[]> {
  return invoke('list_chunks_in_range', { start, end });
}

export function listExternalEvents(start: string, end: string): Promise<ExternalEvent[]> {
  return invoke('list_external_events', { start, end });
}

export function getAgenda(
  start: string,
  end: string,
  label?: string | null,
): Promise<AgendaItem[]> {
  return invoke('get_agenda', { start, end, label: label ?? null });
}

export function createFixedChunk(
  taskId: string,
  startTime: string,
  endTime: string,
): Promise<[Chunk, Task]> {
  return invoke('create_fixed_chunk', { taskId, startTime, endTime });
}

export function completeChunk(
  chunkId: string,
  durationOverride?: number | null,
): Promise<[Chunk, Task]> {
  return invoke('complete_chunk', {
    chunkId,
    durationOverride: durationOverride ?? null,
  });
}

export function reopenChunk(chunkId: string): Promise<[Chunk, Task]> {
  return invoke('reopen_chunk', { chunkId });
}

export function moveChunk(chunkId: string, newStart: string, newEnd: string): Promise<Chunk> {
  return invoke('move_chunk', { chunkId, newStart, newEnd });
}

export function resizeChunk(chunkId: string, newEnd: string): Promise<[Chunk, Task]> {
  return invoke('resize_chunk', { chunkId, newEnd });
}

export function lockChunk(chunkId: string): Promise<Chunk> {
  return invoke('lock_chunk', { chunkId });
}

export function unlockChunk(chunkId: string): Promise<Chunk> {
  return invoke('unlock_chunk', { chunkId });
}

export function deleteFixedChunk(chunkId: string): Promise<Chunk> {
  return invoke('delete_fixed_chunk', { chunkId });
}

export function listComments(taskId: string): Promise<Comment[]> {
  return invoke('list_comments', { taskId });
}

export function createComment(input: CreateCommentInput): Promise<Comment> {
  return invoke('create_comment', { input });
}

/** Replace a user comment's content (author-only; system comments are immutable). */
export function updateComment(id: string, content: string): Promise<Comment> {
  return invoke('update_comment', { id, input: { content } });
}

/** Delete a user comment (author-only; system comments are immutable). */
export function deleteComment(id: string): Promise<void> {
  return invoke('delete_comment', { id });
}

export function createSchedule(input: CreateScheduleInput): Promise<Schedule> {
  return invoke('create_schedule', { input });
}

export function getSchedule(id: string): Promise<Schedule> {
  return invoke('get_schedule', { id });
}

export function updateSchedule(id: string, input: UpdateScheduleInput): Promise<Schedule> {
  return invoke('update_schedule', { id, input });
}

export function deleteSchedule(id: string): Promise<void> {
  return invoke('delete_schedule', { id });
}

export function listSchedules(): Promise<Schedule[]> {
  return invoke('list_schedules');
}

export function createTemplate(input: CreateTemplateInput): Promise<RecurringTemplate> {
  return invoke('create_template', { input });
}

export function getTemplate(id: string): Promise<RecurringTemplate> {
  return invoke('get_template', { id });
}

export function updateTemplate(id: string, input: UpdateTemplateInput): Promise<RecurringTemplate> {
  return invoke('update_template', { id, input });
}

export function deleteTemplate(id: string): Promise<void> {
  return invoke('delete_template', { id });
}

export function listTemplates(): Promise<RecurringTemplate[]> {
  return invoke('list_templates');
}

export function getConfig(): Promise<AppConfig> {
  return invoke('get_config');
}

export function updateConfig(input: UpdateConfigInput): Promise<AppConfig> {
  return invoke('update_config', { input });
}

export function triggerReschedule(): Promise<ScheduleResult> {
  return invoke('trigger_reschedule');
}

export function triggerRescheduleIncremental(taskIds: string[]): Promise<ScheduleResult> {
  return invoke('trigger_reschedule_incremental', { taskIds });
}

/** Open a URL in the system browser (never navigates the webview). */
export function openExternalUrl(url: string): Promise<void> {
  return openUrl(url);
}

/** Begin Google OAuth flow. Returns the consent URL; the loopback exchange completes in the background. */
export function beginGoogleAuth(): Promise<string> {
  return invoke('begin_google_auth');
}

/** Poll the current Google auth connection status (read-only, infallible). */
export function googleAuthStatus(): Promise<AuthStatus> {
  return invoke('google_auth_status');
}

/** Revoke the stored token and clear mirrored events. Does not trigger a reschedule. */
export function googleDisconnect(): Promise<void> {
  return invoke('google_disconnect');
}

export function googleListCalendars(): Promise<ExternalCalendar[]> {
  return invoke('google_list_calendars');
}

export function getPullCalendars(): Promise<string[]> {
  return invoke('get_pull_calendars');
}

export function setPullCalendars(calendarIds: string[]): Promise<void> {
  return invoke('set_pull_calendars', { calendarIds });
}

/** Mirror the selected calendars and run a full reschedule. Safe no-op when disconnected. */
export function pullExternalEvents(): Promise<ScheduleResult> {
  return invoke('pull_external_events');
}

/** Manual "Sync now": pull, fully reschedule, then push placements. Records sync bookkeeping. */
export function syncNow(): Promise<SyncOutcome> {
  return invoke('sync_now');
}

/** Read the last-sync bookkeeping (display-only; backend is lenient on malformed values). */
export function getSyncStatus(): Promise<SyncStatus> {
  return invoke('get_sync_status');
}

/**
 * Create a user-owned event on `calendarId`, write it through to Google, mirror
 * it, and reschedule. Resolves to the mirrored event; callers must still refetch
 * the visible range (the reschedule may cascade other chunks).
 */
export function createUserEvent(
  calendarId: string,
  payload: UserEventPayload,
): Promise<ExternalEvent> {
  return invoke('create_user_event', { calendarId, payload });
}

/**
 * Update a user-owned event, write through to Google, re-mirror, and reschedule.
 * Resolves to the re-mirrored event; callers must still refetch the visible range.
 */
export function updateUserEvent(
  calendarId: string,
  eventId: string,
  payload: UserEventPayload,
): Promise<ExternalEvent> {
  return invoke('update_user_event', { calendarId, eventId, payload });
}

/**
 * Delete a user-owned event, remove its mirror row, and reschedule. Callers must
 * still refetch the visible range afterwards.
 */
export function deleteUserEvent(calendarId: string, eventId: string): Promise<void> {
  return invoke('delete_user_event', { calendarId, eventId });
}

export function getBackupStatus(): Promise<BackupStatus> {
  return invoke('get_backup_status');
}

export function setBackupEnabled(enabled: boolean): Promise<BackupStatus> {
  return invoke('set_backup_enabled', { enabled });
}

/** Manual "Back up now" (keeps the stale-writer guard); returns fresh status. */
export function backupNow(): Promise<BackupStatus> {
  return invoke('backup_now');
}

export function exportBackupToFile(path: string): Promise<void> {
  return invoke('export_backup_to_file', { path });
}

/** Verify + stage an import, then restart into it — on success this never resolves. */
export function importBackupFromFile(path: string): Promise<void> {
  return invoke('import_backup_from_file', { path });
}

export function profileStatus(): Promise<ProfileStatus> {
  return invoke('profile_status');
}

export function unlockProfile(id: string): Promise<ActiveProfile> {
  return invoke('unlock_profile', { id });
}

export function createProfile(name: string): Promise<ProfileInfo> {
  return invoke('create_profile', { name });
}

/** Rename any profile (active or not). */
export function renameProfile(id: string, name: string): Promise<ProfileInfo> {
  return invoke('rename_profile', { id, name });
}

/** Delete a non-active profile and its data. */
export function deleteProfile(id: string): Promise<void> {
  return invoke('delete_profile', { id });
}

/** Switch the active profile in place; resolves with the new active profile. */
export function switchProfile(id: string): Promise<ActiveProfile> {
  return invoke('switch_profile', { id });
}

/**
 * Extract a user-facing message from a failed API call.
 *
 * Backend errors cross IPC as `{ error, message }`. Only error kinds in
 * `allowedKinds` are surfaced verbatim; every other kind falls back to the
 * caller's generic string so internal details never reach toasts.
 */
function errorMessage(e: unknown, fallback: string, allowedKinds: string[]): string {
  if (typeof e === 'object' && e !== null && 'error' in e && 'message' in e) {
    const { error, message } = e as { error: unknown; message: unknown };
    if (
      typeof error === 'string' &&
      allowedKinds.includes(error) &&
      typeof message === 'string' &&
      message.length > 0
    ) {
      return message;
    }
  }
  return fallback;
}

function errorMessageFn(allowedKinds: string[]): (e: unknown, fallback: string) => string {
  return (e, fallback) => errorMessage(e, fallback, allowedKinds);
}

/** Surfaces `validation` messages only; all other kinds fall back to `fallback`. */
export const apiErrorMessage = errorMessageFn(['validation']);

/**
 * Like `apiErrorMessage`, but also surfaces `calendar_sync` messages.
 *
 * Calendar sync messages are sanitized at construction (status codes / "network
 * error" only — never tokens or URLs) and are safe to display verbatim.
 */
export const syncErrorMessage = errorMessageFn(['validation', 'calendar_sync']);

/**
 * Like `apiErrorMessage`, but also surfaces `backup` messages.
 *
 * Backup messages are constructed sanitized (archive/database problems,
 * never tokens) and user-actionable, so they are safe to display verbatim.
 */
export const backupErrorMessage = errorMessageFn(['validation', 'backup']);
