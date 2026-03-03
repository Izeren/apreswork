// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Shared task/chunk verb layer — the frontend counterpart of the backend's
 * "one definition per policy" invariant. Each verb (API call + confirmation
 * rule + refetch policy + toast copy) is defined exactly once here; calendar
 * and task-list surfaces render these verbs instead of re-implementing them.
 */

import * as api from '../api';
import type { Snippet } from 'svelte';
import type { AgendaItem, Chunk, Task, UpdateTaskInput } from '../types';
import { toastState } from '../stores/toast.svelte';
import { formatDateTime } from '../utils';
import {
  todayDeadline,
  tomorrowDeadline,
  nextWeekDeadline,
  nextMonthDeadline,
} from '../components/shared/deadlinePresets';

export interface ConfirmSpec {
  title: string;
  message: string;
  confirmLabel: string;
  destructive: boolean;
}

export interface TaskActionsHost {
  /** Refetch the visible range/list — reschedules cascade to other chunks. */
  refresh: () => void;
  /** Present a confirmation dialog; resolve `true` to proceed. */
  confirm: (spec: ConfirmSpec) => Promise<boolean>;
  openTaskEditor: (taskId: string) => void;
  openTemplateEditor: (templateId: string) => void;
}

/** One entry of a context menu; built by the menu builders below. */
export interface ContextMenuItem {
  label: string;
  /** Runs on click. Omit when `submenu` supplies the interaction instead. */
  action?: () => void | Promise<void>;
  destructive?: boolean;
  /** Panel shown beside the item on hover/click instead of running an action. */
  submenu?: Snippet;
}

/** Injectable subset of api functions that TaskActions dispatches. */
export interface TaskActionsApiSubset {
  completeTask?: (taskId: string) => Promise<Task>;
  cancelTask?: (taskId: string) => Promise<Task>;
  getTask?: (taskId: string) => Promise<Task>;
  listChunksForTask?: (taskId: string) => Promise<Chunk[]>;
  createFixedChunk?: (taskId: string, start: string, end: string) => Promise<[Chunk, Task]>;
  updateTask?: (taskId: string, input: UpdateTaskInput) => Promise<Task>;
  deleteTask?: (taskId: string) => Promise<void>;
  apiErrorMessage?: (e: unknown, fallback: string) => string;
  completeChunk?: (chunkId: string) => Promise<[Chunk, Task]>;
  reopenChunk?: (chunkId: string) => Promise<[Chunk, Task]>;
  lockChunk?: (chunkId: string) => Promise<Chunk>;
  unlockChunk?: (chunkId: string) => Promise<Chunk>;
  deleteFixedChunk?: (chunkId: string) => Promise<Chunk>;
}

function minutesBetween(start: string, end: string): number {
  return (Date.parse(end) - Date.parse(start)) / 60_000;
}

export class TaskActions {
  readonly #host: TaskActionsHost;
  // All api calls go through #c so tests can inject fakes without vi.mock.
  readonly #c: Required<TaskActionsApiSubset>;

  constructor(host: TaskActionsHost, apiSubset?: TaskActionsApiSubset) {
    this.#host = host;
    this.#c = {
      completeTask: apiSubset?.completeTask ?? ((id) => api.completeTask(id)),
      cancelTask: apiSubset?.cancelTask ?? ((id) => api.cancelTask(id)),
      getTask: apiSubset?.getTask ?? ((id) => api.getTask(id)),
      listChunksForTask: apiSubset?.listChunksForTask ?? ((id) => api.listChunksForTask(id)),
      createFixedChunk:
        apiSubset?.createFixedChunk ?? ((id, s, e) => api.createFixedChunk(id, s, e)),
      updateTask: apiSubset?.updateTask ?? ((id, input) => api.updateTask(id, input)),
      deleteTask: apiSubset?.deleteTask ?? ((id) => api.deleteTask(id)),
      apiErrorMessage: apiSubset?.apiErrorMessage ?? ((e, f) => api.apiErrorMessage(e, f)),
      completeChunk: apiSubset?.completeChunk ?? ((id) => api.completeChunk(id)),
      reopenChunk: apiSubset?.reopenChunk ?? ((id) => api.reopenChunk(id)),
      lockChunk: apiSubset?.lockChunk ?? ((id) => api.lockChunk(id)),
      unlockChunk: apiSubset?.unlockChunk ?? ((id) => api.unlockChunk(id)),
      deleteFixedChunk: apiSubset?.deleteFixedChunk ?? ((id) => api.deleteFixedChunk(id)),
    };
  }

  async #run(call: () => Promise<unknown>, success: string, failure: string): Promise<void> {
    try {
      await call();
      toastState.success(success);
      this.#host.refresh();
    } catch (e) {
      toastState.error(this.#c.apiErrorMessage(e, failure));
    }
  }

  async #runWithConfirmation(
    spec: ConfirmSpec,
    call: () => Promise<unknown>,
    success: string,
    failure: string,
  ): Promise<void> {
    const ok = await this.#host.confirm(spec);
    if (!ok) return;
    await this.#run(call, success, failure);
  }

  async #runChunkVerb(
    chunkId: string,
    verb: (id: string) => Promise<unknown>,
    successMsg: string,
    failureMsg: string,
  ): Promise<void> {
    await this.#run(() => verb(chunkId), successMsg, failureMsg);
  }

  async completeChunk(chunkId: string): Promise<void> {
    await this.#runChunkVerb(
      chunkId,
      this.#c.completeChunk,
      'Chunk completed',
      'Failed to complete chunk',
    );
  }

  async reopenChunk(chunkId: string): Promise<void> {
    await this.#runChunkVerb(
      chunkId,
      this.#c.reopenChunk,
      'Chunk reopened',
      'Failed to reopen chunk',
    );
  }

  async completeTask(taskId: string, taskTitle: string): Promise<void> {
    await this.#runWithConfirmation(
      {
        title: 'Complete task',
        message: `Complete "${taskTitle}"? All remaining time will be logged as done.`,
        confirmLabel: 'Complete task',
        destructive: false,
      },
      () => this.#c.completeTask(taskId),
      'Task completed',
      'Failed to complete task',
    );
  }

  async lockChunk(chunkId: string): Promise<void> {
    await this.#runChunkVerb(chunkId, this.#c.lockChunk, 'Chunk locked', 'Failed to lock chunk');
  }

  async unlockChunk(chunkId: string): Promise<void> {
    await this.#runChunkVerb(
      chunkId,
      this.#c.unlockChunk,
      'Chunk unlocked',
      'Failed to unlock chunk',
    );
  }

  /**
   * Delete a fixed (manually placed) chunk. No confirm gate — like unlock,
   * only the manual placement is lost; the scheduler re-places the task's
   * remaining time on the next incremental reschedule.
   */
  async deleteFixedChunk(chunkId: string): Promise<void> {
    await this.#runChunkVerb(
      chunkId,
      this.#c.deleteFixedChunk,
      'Fixed chunk deleted',
      'Failed to delete chunk',
    );
  }

  /**
   * Create a fixed chunk starting now for the task's remaining minutes.
   * Mirrors the backend allocation rule for `create_fixed_chunk`: fixed
   * chunks of any status count against the budget; auto chunks are
   * recomputed on reschedule and do not.
   */
  async doNow(taskId: string, now: Date): Promise<void> {
    try {
      const task = await this.#c.getTask(taskId);
      const chunks = await this.#c.listChunksForTask(taskId);
      const fixedMinutes = chunks
        .filter((c) => c.is_fixed)
        .reduce((sum, c) => sum + minutesBetween(c.start_time, c.end_time), 0);
      const remaining = task.duration_minutes - task.time_logged_minutes - fixedMinutes;
      if (remaining <= 0) {
        toastState.error('Nothing left to schedule — the task is fully logged or locked in');
        return;
      }
      const start = now;
      const end = new Date(now.getTime() + remaining * 60_000);
      await this.#run(
        () => this.#c.createFixedChunk(taskId, start.toISOString(), end.toISOString()),
        'Scheduled to start now',
        'Failed to schedule task',
      );
    } catch (e) {
      toastState.error(this.#c.apiErrorMessage(e, 'Failed to schedule task'));
    }
  }

  /** Preset computation belongs to the calling surface. */
  async extendDeadline(taskId: string, newDeadline: string): Promise<void> {
    await this.#run(
      () => this.#c.updateTask(taskId, { deadline: newDeadline }),
      'Deadline updated',
      'Failed to update deadline',
    );
  }

  async toBacklog(taskId: string): Promise<void> {
    await this.#run(
      () => this.#c.updateTask(taskId, { status: 'backlog' }),
      'Task moved to backlog',
      'Failed to move task to backlog',
    );
  }

  async activate(taskId: string): Promise<void> {
    await this.#run(
      () => this.#c.updateTask(taskId, { status: 'pending' }),
      'Task activated',
      'Failed to activate task',
    );
  }

  async cancelTask(taskId: string, taskTitle: string): Promise<void> {
    await this.#runWithConfirmation(
      {
        title: 'Cancel task',
        message: `Cancel "${taskTitle}"? Its unfinished chunks will be removed from the calendar.`,
        confirmLabel: 'Cancel task',
        destructive: true,
      },
      () => this.#c.cancelTask(taskId),
      'Task cancelled',
      'Failed to cancel task',
    );
  }

  async deleteTask(taskId: string, taskTitle: string, isRecurringInstance: boolean): Promise<void> {
    const message = isRecurringInstance
      ? `"${taskTitle}" is a recurring instance: deleting cancels this occurrence, ` +
        `and the template keeps generating future ones. Continue?`
      : `Delete "${taskTitle}"? The task and its history are removed permanently.`;
    await this.#runWithConfirmation(
      { title: 'Delete task', message, confirmLabel: 'Delete', destructive: true },
      () => this.#c.deleteTask(taskId),
      'Task deleted',
      'Failed to delete task',
    );
  }

  editTask(taskId: string): void {
    this.#host.openTaskEditor(taskId);
  }

  editTemplate(templateId: string): void {
    this.#host.openTemplateEditor(templateId);
  }
}

/**
 * The canonical set of deadline-extend preset items shared by the chunk
 * context menu and the status warning resolution menu. Both surfaces call
 * this instead of assembling the four items independently.
 */
export function deadlineExtendItems(
  taskId: string,
  actions: TaskActions,
  now: Date,
): ContextMenuItem[] {
  const presets: [string, string][] = [
    ['today', todayDeadline(now)],
    ['tomorrow', tomorrowDeadline(now)],
    ['next week', nextWeekDeadline(now)],
    ['next month', nextMonthDeadline(now)],
  ];
  return presets.map(([name, deadline]) => ({
    label: `Extend to ${name} (${formatDateTime(deadline)})`,
    action: () => actions.extendDeadline(taskId, deadline),
  }));
}

/**
 * The one definition of which verbs a chunk offers in each state
 * (plans/calendar-ux-polish.md §B). Surfaces render this list; they never
 * assemble their own.
 */
export function chunkContextMenuItems(
  item: AgendaItem,
  actions: TaskActions,
  now: Date,
): ContextMenuItem[] {
  const chunk = item.chunk;
  const taskId = chunk.task_id;
  const templateId = item.task_recurring_template_id;
  const items: ContextMenuItem[] = [];

  if (chunk.status === 'completed') {
    items.push({ label: 'Reopen chunk', action: () => actions.reopenChunk(chunk.id) });
  } else {
    items.push({ label: 'Complete chunk', action: () => actions.completeChunk(chunk.id) });
    items.push({
      label: 'Complete task',
      action: () => actions.completeTask(taskId, item.task_title),
    });
    items.push({ label: 'Do now', action: () => actions.doNow(taskId, now) });
    items.push(
      chunk.is_fixed
        ? { label: 'Unlock chunk', action: () => actions.unlockChunk(chunk.id) }
        : { label: 'Lock chunk', action: () => actions.lockChunk(chunk.id) },
    );
    if (chunk.is_fixed) {
      items.push({
        label: 'Delete fixed chunk',
        destructive: true,
        action: () => actions.deleteFixedChunk(chunk.id),
      });
    }
    if (item.task_deadline !== null && Date.parse(item.task_deadline) < now.getTime()) {
      items.push(...deadlineExtendItems(taskId, actions, now));
    }
  }

  items.push({ label: 'Edit task', action: () => actions.editTask(taskId) });
  if (templateId !== null) {
    items.push({ label: 'Edit template', action: () => actions.editTemplate(templateId) });
  }
  if (chunk.status !== 'completed') {
    items.push({
      label: 'Cancel task',
      destructive: true,
      action: () => actions.cancelTask(taskId, item.task_title),
    });
  }
  return items;
}

/**
 * The one definition of which verbs a task row offers in each status
 * (plans/calendar-ux-polish.md §C), constrained to the task state machine
 * (ARCHITECTURE.md §4): Complete task only from Scheduled, Activate only
 * from Backlog, no transitions out of Completed/Cancelled besides Delete.
 */
export function taskContextMenuItems(
  task: Task,
  actions: TaskActions,
  now: Date,
): ContextMenuItem[] {
  const templateId = task.recurring_template_id;
  const items: ContextMenuItem[] = [];

  if (task.status === 'scheduled') {
    items.push({ label: 'Complete task', action: () => actions.completeTask(task.id, task.title) });
  }
  if (task.status === 'pending' || task.status === 'scheduled') {
    items.push({ label: 'Do now', action: () => actions.doNow(task.id, now) });
    items.push({ label: 'Move to backlog', action: () => actions.toBacklog(task.id) });
  }
  if (task.status === 'backlog') {
    items.push({ label: 'Activate', action: () => actions.activate(task.id) });
  }

  items.push({ label: 'Edit task', action: () => actions.editTask(task.id) });
  if (templateId !== null) {
    items.push({ label: 'Edit template', action: () => actions.editTemplate(templateId) });
  }

  if (task.status === 'backlog' || task.status === 'pending' || task.status === 'scheduled') {
    items.push({
      label: 'Cancel task',
      destructive: true,
      action: () => actions.cancelTask(task.id, task.title),
    });
  }
  items.push({
    label: 'Delete task',
    destructive: true,
    action: () => actions.deleteTask(task.id, task.title, templateId !== null),
  });
  return items;
}
