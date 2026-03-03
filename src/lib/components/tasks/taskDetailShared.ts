// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Injectable API subset used by TaskDetail to enable prop-based dependency injection in tests.

import * as api from '../../api';

/** Injectable subset of api functions called directly by TaskDetail (not via stores). */
export interface TaskDetailApi {
  completeTask: typeof api.completeTask;
  cancelTask: typeof api.cancelTask;
  listChunksForTask: typeof api.listChunksForTask;
  listComments: typeof api.listComments;
  createComment: typeof api.createComment;
  updateComment: typeof api.updateComment;
  deleteComment: typeof api.deleteComment;
}

/**
 * Default implementation delegates to api at call time so sibling test suites'
 * vi.mock('../../api') replacements are observed without changes to those tests.
 * Direct function references (api.X) would capture the mock at module load time and
 * fail when the mock omits a function — lambdas defer the lookup to the call site.
 */
export const defaultTaskDetailApi: TaskDetailApi = {
  completeTask: (id) => api.completeTask(id),
  cancelTask: (id) => api.cancelTask(id),
  listChunksForTask: (id) => api.listChunksForTask(id),
  listComments: (id) => api.listComments(id),
  createComment: (input) => api.createComment(input),
  updateComment: (id, content) => api.updateComment(id, content),
  deleteComment: (id) => api.deleteComment(id),
};
