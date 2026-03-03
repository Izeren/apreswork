// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { RescheduleApiSubset } from '../../actions/rescheduleTrigger';
import type { TaskActionsApiSubset } from '../../actions/taskActions';
import * as api from '../../api';

export type TaskListViewApi = RescheduleApiSubset & TaskActionsApiSubset;

/**
 * Default binds to the api module object directly: property lookup is deferred
 * to call time, so vi.mock('../../api') replacements are observed without changes
 * to existing tests. No lambda table needed — direct assignment cannot drift when
 * TaskActionsApiSubset grows, and TypeScript will error at the assignment site.
 */
export const defaultTaskListViewApi: TaskListViewApi = api;
