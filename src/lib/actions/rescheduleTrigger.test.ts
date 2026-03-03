// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, type MockedFunction } from 'vitest';
import type { ScheduleResult } from '../types';
import type { RescheduleApiSubset } from './rescheduleTrigger';
import { runReschedule } from './rescheduleTrigger';
import { busyCallSequence, flushPromises, lastToastError, makeWarning } from '../testFixtures';
import { toastState } from '../stores/toast.svelte';
import { warningState } from '../stores/warnings.svelte';
import { apiErrorMessage } from '../api';

type FakeRescheduleApi = {
  triggerReschedule: MockedFunction<RescheduleApiSubset['triggerReschedule']>;
  apiErrorMessage: RescheduleApiSubset['apiErrorMessage'];
};

describe('runReschedule', () => {
  let fakeApi: FakeRescheduleApi;
  let setBusy: MockedFunction<(busy: boolean) => void>;
  let onSuccess: MockedFunction<() => void>;

  beforeEach(() => {
    setBusy = vi.fn();
    onSuccess = vi.fn();
    toastState.reset();
    warningState.clear();
    fakeApi = {
      triggerReschedule: vi.fn<() => Promise<ScheduleResult>>().mockResolvedValue({
        placed_chunks: [],
        warnings: [],
      }),
      apiErrorMessage: (e, f) => apiErrorMessage(e, f),
    };
  });

  async function rejectAndRun(error: unknown): Promise<void> {
    fakeApi.triggerReschedule.mockRejectedValue(error);
    runReschedule(setBusy, onSuccess, fakeApi);
    await flushPromises();
  }

  describe('success path', () => {
    it('calls setBusy(true) synchronously before any async work', () => {
      runReschedule(setBusy, onSuccess, fakeApi);
      expect(setBusy).toHaveBeenCalledWith(true);
    });

    it('updates warningState with the returned warnings', async () => {
      const warnings = [makeWarning()];
      fakeApi.triggerReschedule.mockResolvedValue({ placed_chunks: [], warnings });

      runReschedule(setBusy, onSuccess, fakeApi);
      await flushPromises();

      expect(warningState.items).toEqual(warnings);
    });

    it('shows a "Reschedule complete" success toast', async () => {
      runReschedule(setBusy, onSuccess, fakeApi);
      await flushPromises();

      expect(toastState.items[0]?.text).toBe('Reschedule complete');
    });

    it('calls onSuccess()', async () => {
      runReschedule(setBusy, onSuccess, fakeApi);
      await flushPromises();

      expect(onSuccess).toHaveBeenCalledOnce();
    });

    it('calls setBusy(false) in finally after onSuccess', async () => {
      runReschedule(setBusy, onSuccess, fakeApi);
      await flushPromises();

      expect(busyCallSequence(setBusy)).toEqual([true, false]);
    });
  });

  describe('failure path', () => {
    it.each([
      {
        desc: 'generic error',
        error: new Error('network'),
        expectedMessage: 'Reschedule failed',
      },
      {
        desc: 'validation error',
        error: {
          error: 'validation',
          message: 'cannot reschedule: task has no schedule window',
        },
        expectedMessage: 'cannot reschedule: task has no schedule window',
      },
    ])('error path: $desc', async ({ error, expectedMessage }) => {
      await rejectAndRun(error);

      expect(lastToastError(toastState.items)).toBe(expectedMessage);
      expect(onSuccess).not.toHaveBeenCalled();
      expect(busyCallSequence(setBusy)).toEqual([true, false]);
    });
  });
});
