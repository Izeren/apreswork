// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { Chunk, Comment, CreateCommentInput } from '../../types';
import * as api from '../../api';

export interface TaskFormApi {
  listComments: (taskId: string) => Promise<Comment[]>;
  createComment: (input: CreateCommentInput) => Promise<Comment>;
  updateComment: (id: string, content: string) => Promise<Comment>;
  deleteComment: (id: string) => Promise<void>;
  listChunksForTask: (taskId: string) => Promise<Chunk[]>;
  unlockChunk: (chunkId: string) => Promise<Chunk>;
  deleteFixedChunk: (chunkId: string) => Promise<Chunk>;
}

export const defaultTaskFormApi: TaskFormApi = {
  listComments: (taskId) => api.listComments(taskId),
  createComment: (input) => api.createComment(input),
  updateComment: (id, content) => api.updateComment(id, content),
  deleteComment: (id) => api.deleteComment(id),
  listChunksForTask: (taskId) => api.listChunksForTask(taskId),
  unlockChunk: (chunkId) => api.unlockChunk(chunkId),
  deleteFixedChunk: (chunkId) => api.deleteFixedChunk(chunkId),
};
