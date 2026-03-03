// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { WarningState } from './warnings.svelte';
import type { ScheduleWarning } from '../types';

const makeWarning = (taskId: string, title: string): ScheduleWarning => ({
  task_id: taskId,
  task_title: title,
  kind: { Unschedulable: { reason: 'no windows' } },
});

describe('WarningState', () => {
  let warnings: WarningState;

  beforeEach(() => {
    warnings = new WarningState();
  });

  it('set populates items and count', () => {
    const data: ScheduleWarning[] = [makeWarning('t1', 'Task 1'), makeWarning('t2', 'Task 2')];
    warnings.set(data);
    expect(warnings.items).toHaveLength(2);
    expect(warnings.count).toBe(2);
  });

  it('clear empties items and resets count to 0', () => {
    warnings.set([makeWarning('t1', 'Task 1')]);
    expect(warnings.count).toBe(1);
    warnings.clear();
    expect(warnings.items).toHaveLength(0);
    expect(warnings.count).toBe(0);
  });

  it('set with empty array gives count 0', () => {
    warnings.set([makeWarning('t1', 'Task 1')]);
    warnings.set([]);
    expect(warnings.items).toHaveLength(0);
    expect(warnings.count).toBe(0);
  });

  it('count is derived (reflects items.length)', () => {
    expect(warnings.count).toBe(0);
    warnings.set([makeWarning('t1', 'A'), makeWarning('t2', 'B'), makeWarning('t3', 'C')]);
    expect(warnings.count).toBe(3);
    warnings.set([makeWarning('t1', 'A')]);
    expect(warnings.count).toBe(1);
  });

  it('set preserves warning details', () => {
    const warning: ScheduleWarning = {
      task_id: 'task-42',
      task_title: 'Important Task',
      kind: {
        DeadlineViolation: {
          deadline: '2026-04-01T00:00:00Z',
          earliest_completion: '2026-04-05T00:00:00Z',
        },
      },
    };
    warnings.set([warning]);
    expect(warnings.items[0]).toEqual(warning);
  });
});
