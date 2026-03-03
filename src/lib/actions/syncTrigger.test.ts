// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, type MockedFunction } from 'vitest';
import type { ScheduleWarning, SyncOutcome } from '../types';
import type { SyncApiSubset } from './syncTrigger';
import { runSync } from './syncTrigger';
import { syncSuccessText } from '../utils';
import {
  busyCallSequence,
  flushPromises,
  lastToastError,
  syncSuccessResult,
  syncSuccessResultEmpty,
} from '../testFixtures';
import { toastState } from '../stores/toast.svelte';
import { warningState } from '../stores/warnings.svelte';
import { syncErrorMessage } from '../api';

type FakeSyncApi = {
  syncNow: MockedFunction<SyncApiSubset['syncNow']>;
  syncErrorMessage: MockedFunction<SyncApiSubset['syncErrorMessage']>;
};

describe('runSync', () => {
  let fakeApi: FakeSyncApi;
  let setBusy: MockedFunction<(busy: boolean) => void>;
  let onRefetch: MockedFunction<() => void>;

  beforeEach(() => {
    setBusy = vi.fn();
    onRefetch = vi.fn();
    toastState.reset();
    warningState.clear();
    fakeApi = {
      syncNow: vi.fn<() => Promise<SyncOutcome>>().mockResolvedValue(syncSuccessResultEmpty()),
      syncErrorMessage: vi.fn().mockImplementation(syncErrorMessage),
    };
  });

  async function rejectAndRun(error: unknown, onError?: () => void): Promise<void> {
    fakeApi.syncNow.mockRejectedValue(error);
    runSync(setBusy, onRefetch, fakeApi, onError);
    await flushPromises();
  }

  describe('success path', () => {
    it('calls setBusy(true) synchronously before any async work', () => {
      runSync(setBusy, onRefetch, fakeApi);
      expect(setBusy).toHaveBeenCalledWith(true);
    });

    it('updates warningState from schedule.warnings', async () => {
      const warnings: ScheduleWarning[] = [
        { task_id: 't1', task_title: 'T1', kind: { Unschedulable: { reason: 'no slot' } } },
      ];
      fakeApi.syncNow.mockResolvedValue({
        schedule: { placed_chunks: [], warnings },
        pushed: { created: 0, updated: 0, deleted: 0 },
      });

      runSync(setBusy, onRefetch, fakeApi);
      await flushPromises();

      expect(warningState.items).toEqual(warnings);
    });

    it.each([
      {
        label: 'placed count and pushed count',
        result: syncSuccessResult(),
        expectedText: syncSuccessText(1, 1),
      },
      {
        label: 'pushed as created+updated+deleted sum',
        result: {
          schedule: { placed_chunks: [] as never[], warnings: [] },
          pushed: { created: 2, updated: 3, deleted: 1 },
        },
        expectedText: syncSuccessText(0, 6),
      },
    ])('shows a success toast: $label', async ({ result, expectedText }) => {
      fakeApi.syncNow.mockResolvedValue(result);
      runSync(setBusy, onRefetch, fakeApi);
      await flushPromises();
      expect(toastState.items[0]?.text).toBe(expectedText);
    });

    it('calls onRefetch()', async () => {
      runSync(setBusy, onRefetch, fakeApi);
      await flushPromises();

      expect(onRefetch).toHaveBeenCalledOnce();
    });

    it('calls setBusy(false) in finally', async () => {
      runSync(setBusy, onRefetch, fakeApi);
      await flushPromises();

      expect(busyCallSequence(setBusy)).toEqual([true, false]);
    });
  });

  describe('on failure', () => {
    it.each([
      { label: 'generic fallback', error: new Error('network'), expectedError: 'Sync failed.' },
      {
        label: 'calendar_sync verbatim',
        error: { error: 'calendar_sync', message: 'Google API returned 403' },
        expectedError: 'Google API returned 403',
      },
    ])('shows correct error toast: $label', async ({ error, expectedError }) => {
      await rejectAndRun(error);

      expect(lastToastError(toastState.items)).toBe(expectedError);
    });

    it('calls onError when provided', async () => {
      const onError = vi.fn();

      await rejectAndRun(new Error('network'), onError);

      expect(onError).toHaveBeenCalledOnce();
    });

    it('does not throw when onError is not provided', async () => {
      await rejectAndRun(new Error('network'));

      expect(lastToastError(toastState.items)).not.toBeNull();
    });

    it('does not call onRefetch', async () => {
      await rejectAndRun(new Error('network'));

      expect(onRefetch).not.toHaveBeenCalled();
    });

    it('setBusy(false) fires in the finally block', async () => {
      await rejectAndRun(new Error('network'));

      expect(busyCallSequence(setBusy)).toEqual([true, false]);
    });
  });
});
