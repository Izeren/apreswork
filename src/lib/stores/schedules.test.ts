// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { Schedule, CreateScheduleInput, UpdateScheduleInput } from '../types';

function buildClient() {
  return {
    listSchedules: vi.fn(),
    createSchedule: vi.fn(),
    updateSchedule: vi.fn(),
    deleteSchedule: vi.fn(),
  };
}

const mockSchedule: Schedule = {
  id: 'sched-1',
  name: 'Default',
  is_default: true,
  windows: [],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

const mockSchedule2: Schedule = {
  id: 'sched-2',
  name: 'Weekend',
  is_default: false,
  windows: [],
  created_at: '2026-01-02T00:00:00Z',
  updated_at: '2026-01-02T00:00:00Z',
};

describe('ScheduleState', () => {
  // Dynamically import so the modules resolve after mocks are set up
  let ScheduleState: typeof import('./schedules.svelte').ScheduleState;
  let toastState: import('./toast.svelte').ToastState;
  let schedules: InstanceType<typeof ScheduleState>;
  let client!: ReturnType<typeof buildClient>;

  function assertSingleToast(
    items: typeof toastState.items,
    expected: { level: string; text: string },
  ): void {
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject(expected);
  }

  beforeEach(async () => {
    client = buildClient();
    const schedMod = await import('./schedules.svelte');
    const toastMod = await import('./toast.svelte');
    ScheduleState = schedMod.ScheduleState;
    toastState = toastMod.toastState;
    toastState.items = [];
    schedules = new ScheduleState(client);
  });

  describe('load', () => {
    it.each([
      {
        name: 'success',
        setup: () => client.listSchedules.mockResolvedValue([mockSchedule, mockSchedule2]),
        expectedItems: [mockSchedule, mockSchedule2],
        expectedToast: null as { level: string; text: string } | null,
      },
      {
        name: 'error',
        setup: () => client.listSchedules.mockRejectedValue(new Error('network')),
        expectedItems: [] as (typeof mockSchedule)[],
        expectedToast: { level: 'error', text: 'Failed to load schedules' },
      },
    ])(
      '$name: updates items and loading state',
      async ({ setup, expectedItems, expectedToast }) => {
        setup();
        expect(schedules.loading).toBe(false);
        const promise = schedules.load();
        expect(schedules.loading).toBe(true);
        await promise;
        expect(schedules.loading).toBe(false);
        expect(schedules.items).toEqual(expectedItems);
        if (expectedToast) {
          assertSingleToast(toastState.items, expectedToast);
        } else {
          expect(toastState.items).toHaveLength(0);
        }
      },
    );
  });

  describe('create', () => {
    it.each([
      {
        name: 'success',
        setup: () => client.createSchedule.mockResolvedValue(mockSchedule),
        input: { name: 'Default', windows: [] } as CreateScheduleInput,
        expectedResult: mockSchedule as Schedule | undefined,
        expectedItems: [mockSchedule],
        expectedToast: { level: 'success', text: 'Schedule created' },
      },
      {
        name: 'error',
        setup: () => client.createSchedule.mockRejectedValue(new Error('fail')),
        input: { name: 'Bad', windows: [] } as CreateScheduleInput,
        expectedResult: undefined as Schedule | undefined,
        expectedItems: [] as Schedule[],
        expectedToast: { level: 'error', text: 'Failed to create schedule' },
      },
    ])(
      '$name: returns result and updates items/toast',
      async ({ setup, input, expectedResult, expectedItems, expectedToast }) => {
        setup();
        const result = await schedules.create(input);
        expect(result).toEqual(expectedResult);
        expect(schedules.items).toEqual(expectedItems);
        assertSingleToast(toastState.items, expectedToast);
      },
    );
  });

  describe('update', () => {
    it.each([
      {
        name: 'success: replaces correct item in list',
        initialItems: [mockSchedule, mockSchedule2] as Schedule[],
        setup: () => {
          client.updateSchedule.mockResolvedValue({ ...mockSchedule, name: 'Updated Default' });
        },
        act: async () =>
          schedules.update('sched-1', { name: 'Updated Default' } as UpdateScheduleInput),
        expectedToast: { level: 'success', text: 'Schedule updated' },
        check: () => {
          expect(schedules.items[0].name).toBe('Updated Default');
          expect(schedules.items[1]).toEqual(mockSchedule2);
        },
      },
      {
        name: 'error: shows toast and leaves items unchanged',
        initialItems: [] as Schedule[],
        setup: () => {
          client.updateSchedule.mockRejectedValue(new Error('fail'));
        },
        act: async () => schedules.update('sched-1', { name: 'X' } as UpdateScheduleInput),
        expectedToast: { level: 'error', text: 'Failed to update schedule' },
        check: () => {
          expect(schedules.items).toEqual([]);
        },
      },
    ])('$name', async ({ initialItems, setup, act, expectedToast, check }) => {
      schedules.items = initialItems;
      setup();
      await act();
      assertSingleToast(toastState.items, expectedToast);
      check();
    });
  });

  describe('reset', () => {
    it('drops all profile-scoped state so the next load refetches', async () => {
      client.listSchedules.mockResolvedValue([mockSchedule]);
      await schedules.load();
      expect(schedules.loaded).toBe(true);

      schedules.reset();

      expect(schedules.items).toEqual([]);
      expect(schedules.loading).toBe(false);
      expect(schedules.loaded).toBe(false);

      // A post-reset load refetches instead of serving the stale cache.
      client.listSchedules.mockResolvedValue([mockSchedule2]);
      await schedules.load();
      expect(schedules.items).toEqual([mockSchedule2]);
    });
  });

  describe('remove', () => {
    it.each([
      {
        name: 'success: filters out deleted item',
        initialItems: [mockSchedule, mockSchedule2] as Schedule[],
        setup: () => {
          client.deleteSchedule.mockResolvedValue(undefined);
        },
        act: async () => schedules.remove('sched-1'),
        expectedToast: { level: 'success', text: 'Schedule deleted' },
        expectedItems: [mockSchedule2] as Schedule[],
      },
      {
        name: 'error: shows toast and leaves items unchanged',
        initialItems: [mockSchedule] as Schedule[],
        setup: () => {
          client.deleteSchedule.mockRejectedValue(new Error('fail'));
        },
        act: async () => schedules.remove('sched-1'),
        expectedToast: { level: 'error', text: 'Failed to delete schedule' },
        expectedItems: [mockSchedule] as Schedule[],
      },
    ])('$name', async ({ initialItems, setup, act, expectedToast, expectedItems }) => {
      schedules.items = initialItems;
      setup();
      await act();
      expect(schedules.items).toEqual(expectedItems);
      assertSingleToast(toastState.items, expectedToast);
    });
  });
});
