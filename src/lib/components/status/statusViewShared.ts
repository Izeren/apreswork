// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import * as api from '../../api';
import type { Chunk, ScheduleResult, Task, UpdateTaskInput } from '../../types';

export interface StatusViewApi {
  triggerReschedule: () => Promise<ScheduleResult>;
  getTask: (taskId: string) => Promise<Task>;
  updateTask: (taskId: string, input: UpdateTaskInput) => Promise<Task>;
  completeTask: (taskId: string) => Promise<Task>;
  cancelTask: (taskId: string) => Promise<Task>;
  listChunksForTask: (taskId: string) => Promise<Chunk[]>;
  createFixedChunk: (taskId: string, start: string, end: string) => Promise<[Chunk, Task]>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
}

export const defaultStatusViewApi: StatusViewApi = {
  triggerReschedule: api.triggerReschedule,
  getTask: api.getTask,
  updateTask: api.updateTask,
  completeTask: api.completeTask,
  cancelTask: api.cancelTask,
  listChunksForTask: api.listChunksForTask,
  createFixedChunk: api.createFixedChunk,
  apiErrorMessage: api.apiErrorMessage,
};
