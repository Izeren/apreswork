// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { Schedule, CreateScheduleInput, UpdateScheduleInput } from '../types';
import * as api from '../api';
import { toastState } from './toast.svelte';

interface SchedulesClient {
  listSchedules: () => Promise<Schedule[]>;
  createSchedule: (input: CreateScheduleInput) => Promise<Schedule>;
  updateSchedule: (id: string, input: UpdateScheduleInput) => Promise<Schedule>;
  deleteSchedule: (id: string) => Promise<void>;
}

const defaultClient: SchedulesClient = {
  listSchedules: api.listSchedules,
  createSchedule: api.createSchedule,
  updateSchedule: api.updateSchedule,
  deleteSchedule: api.deleteSchedule,
};

export class ScheduleState {
  items: Schedule[] = $state([]);
  loading: boolean = $state(false);
  loaded: boolean = $state(false);

  readonly #client: SchedulesClient;

  constructor(client: SchedulesClient = defaultClient) {
    this.#client = client;
  }

  async load(force = false): Promise<void> {
    if (this.loading) return;
    if (this.loaded && !force) return;

    this.loading = true;
    try {
      this.items = await this.#client.listSchedules();
      this.loaded = true;
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to load schedules'));
    } finally {
      this.loading = false;
    }
  }

  async create(input: CreateScheduleInput): Promise<Schedule | undefined> {
    try {
      const schedule = await this.#client.createSchedule(input);
      this.items = [...this.items, schedule];
      toastState.success('Schedule created');
      return schedule;
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to create schedule'));
      return undefined;
    }
  }

  async update(id: string, input: UpdateScheduleInput): Promise<void> {
    try {
      const updated = await this.#client.updateSchedule(id, input);
      this.items = this.items.map((s) => (s.id === id ? updated : s));
      toastState.success('Schedule updated');
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to update schedule'));
    }
  }

  async remove(id: string): Promise<void> {
    try {
      await this.#client.deleteSchedule(id);
      this.items = this.items.filter((s) => s.id !== id);
      toastState.success('Schedule deleted');
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to delete schedule'));
    }
  }

  /** Drop all profile-scoped state (profile switch); `loaded` clears so the
   *  next `load()` refetches instead of serving the old profile's cache. */
  reset(): void {
    this.items = [];
    this.loading = false;
    this.loaded = false;
  }
}

export const scheduleState = new ScheduleState();
