// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { Task, TaskFilter, CreateTaskInput, UpdateTaskInput } from '../types';
import * as api from '../api';
import { toastState } from './toast.svelte';

export interface TasksClient {
  listTasks: (filter?: TaskFilter) => Promise<Task[]>;
  createTask: (input: CreateTaskInput) => Promise<Task>;
  updateTask: (id: string, input: UpdateTaskInput) => Promise<Task>;
  deleteTask: (id: string) => Promise<void>;
}

const defaultClient: TasksClient = {
  listTasks: api.listTasks,
  createTask: api.createTask,
  updateTask: api.updateTask,
  deleteTask: api.deleteTask,
};

export class TaskState {
  items: Task[] = $state([]);
  loading: boolean = $state(false);
  selectedId: string | null = $state(null);
  templateEditRequestId: string | null = $state(null);
  templateEditRequestNonce: number = $state(0);
  filter: TaskFilter = $state({});

  selected: Task | undefined = $derived.by(() => this.items.find((t) => t.id === this.selectedId));

  readonly #client: TasksClient;

  constructor(client: TasksClient = defaultClient) {
    this.#client = client;
  }

  async load(): Promise<void> {
    this.loading = true;
    try {
      this.items = await this.#client.listTasks(this.filter);
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to load tasks'));
    } finally {
      this.loading = false;
    }
  }

  async create(input: CreateTaskInput): Promise<Task | undefined> {
    try {
      const task = await this.#client.createTask(input);
      this.items = [...this.items, task];
      toastState.success('Task created');
      return task;
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to create task'));
      return undefined;
    }
  }

  async update(id: string, input: UpdateTaskInput): Promise<void> {
    const snapshot = this.items;
    this.items = this.items.map((t) => (t.id === id ? { ...t, ...input } : t));
    try {
      const updated = await this.#client.updateTask(id, input);
      this.items = this.items.map((t) => (t.id === id ? updated : t));
      toastState.success('Task updated');
    } catch (e) {
      this.items = snapshot;
      toastState.error(api.apiErrorMessage(e, 'Failed to update task'));
    }
  }

  async remove(id: string): Promise<void> {
    const snapshot = this.items;
    this.items = this.items.filter((t) => t.id !== id);
    try {
      await this.#client.deleteTask(id);
      toastState.success('Task deleted');
      if (this.selectedId === id) {
        this.selectedId = null;
      }
    } catch (e) {
      this.items = snapshot;
      toastState.error(api.apiErrorMessage(e, 'Failed to delete task'));
    }
  }

  select(id: string | null): void {
    this.selectedId = id;
  }

  requestTemplateEdit(templateId: string): void {
    this.templateEditRequestId = templateId;
    this.templateEditRequestNonce += 1;
  }

  clearTemplateEditRequest(): void {
    this.templateEditRequestId = null;
  }

  setFilter(filter: TaskFilter): void {
    this.filter = filter;
  }

  /** Drop all profile-scoped state (profile switch). Nonces stay monotonic. */
  reset(): void {
    this.items = [];
    this.loading = false;
    this.selectedId = null;
    this.templateEditRequestId = null;
    this.filter = {};
  }
}

export const taskState = new TaskState();
