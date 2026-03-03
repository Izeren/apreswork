// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { ScheduleWarning } from '../../types';

export const DEADLINE_WARNING: ScheduleWarning = {
  task_id: 'task-1',
  task_title: 'Alpha task',
  kind: {
    DeadlineViolation: {
      deadline: '2026-07-01T10:00:00Z',
      earliest_completion: '2026-07-20T18:00:00Z',
    },
  },
};

export const BLOCKING_WARNING: ScheduleWarning = {
  task_id: 'task-2',
  task_title: 'Beta task',
  kind: {
    Unschedulable: { reason: 'No schedule windows are available.' },
  },
};
