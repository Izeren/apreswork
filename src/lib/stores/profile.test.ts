// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { ActiveProfile } from '../types';
import { ALICE, BOB, aliceActive } from '../components/profile/profileFixtures';

function buildClient() {
  return {
    profileStatus: vi.fn(),
    switchProfile: vi.fn(),
  };
}

describe('ProfileState', () => {
  let ProfileState: typeof import('./profile.svelte').ProfileState;
  let toastState: import('./toast.svelte').ToastState;
  let taskState: import('./tasks.svelte').TaskState;
  let warningState: import('./warnings.svelte').WarningState;
  let scheduleState: import('./schedules.svelte').ScheduleState;
  let templateState: import('./templates.svelte').TemplateState;
  let calendarFocusState: import('./calendarFocus.svelte').CalendarFocusState;
  let profile: InstanceType<typeof ProfileState>;
  let client: ReturnType<typeof buildClient>;

  beforeEach(async () => {
    client = buildClient();
    ProfileState = (await import('./profile.svelte')).ProfileState;
    toastState = (await import('./toast.svelte')).toastState;
    taskState = (await import('./tasks.svelte')).taskState;
    warningState = (await import('./warnings.svelte')).warningState;
    scheduleState = (await import('./schedules.svelte')).scheduleState;
    templateState = (await import('./templates.svelte')).templateState;
    calendarFocusState = (await import('./calendarFocus.svelte')).calendarFocusState;
    toastState.items = [];
    taskState.reset();
    warningState.clear();
    scheduleState.reset();
    templateState.reset();
    calendarFocusState.clear();
    profile = new ProfileState(client);
  });

  async function loadProfileWith<T>(status: T): Promise<void> {
    client.profileStatus.mockResolvedValue(status);
    await profile.load();
  }

  describe('load', () => {
    it('populates status and the derived active/others', async () => {
      await loadProfileWith(aliceActive());
      expect(profile.status).toEqual(aliceActive());
      expect(profile.active).toEqual({ id: ALICE.id, name: ALICE.name });
      expect(profile.others).toEqual([BOB]);
      expect(profile.loadError).toBeNull();
    });

    it('sets loadError on failure and clears it on the next success', async () => {
      client.profileStatus.mockRejectedValueOnce({ error: 'internal', message: 'boom' });
      await profile.load();
      expect(profile.loadError).toBe('Could not load profiles.');
      expect(profile.status).toBeNull();

      client.profileStatus.mockResolvedValueOnce(aliceActive());
      await profile.load();
      expect(profile.loadError).toBeNull();
      expect(profile.active?.id).toBe(ALICE.id);
    });

    it('derives an empty others list while no profile is active', async () => {
      await loadProfileWith({ active: null, profiles: [ALICE, BOB], last_used: null });
      expect(profile.active).toBeNull();
      expect(profile.others).toEqual([]);
    });
  });

  describe('setActive', () => {
    it('reflects the gate unlock into status', async () => {
      await loadProfileWith({ active: null, profiles: [ALICE, BOB], last_used: BOB.id });

      profile.setActive({ id: BOB.id, name: BOB.name });
      expect(profile.active).toEqual({ id: BOB.id, name: BOB.name });
      expect(profile.status?.last_used).toBe(BOB.id);
    });

    it('is a no-op before the first load', () => {
      profile.setActive({ id: BOB.id, name: BOB.name });
      expect(profile.status).toBeNull();
      expect(profile.active).toBeNull();
    });
  });

  describe('switchTo', () => {
    const SWITCHED: ActiveProfile = { id: BOB.id, name: BOB.name };

    beforeEach(async () => {
      await loadProfileWith(aliceActive());
    });

    it.each<{
      label: string;
      setup: () => void;
      extraAssert: () => void;
    }>([
      {
        label: 'updates active profile and toasts success',
        setup: () => {},
        extraAssert: () => {
          expect(toastState.items.some((t) => t.text === 'Switched to "Bob".')).toBe(true);
        },
      },
      {
        label: 'resets every profile-scoped store',
        setup: () => {
          taskState.selectedId = 't-1';
          taskState.filter = { statuses: ['scheduled'] };
          warningState.set([
            { task_id: 't-1', task_title: 'T', kind: { Unschedulable: { reason: 'x' } } },
          ]);
          scheduleState.loaded = true;
          templateState.loaded = true;
          calendarFocusState.request('c-1', '2026-07-13T10:00:00Z');
        },
        extraAssert: () => {
          expect(taskState.selectedId).toBeNull();
          expect(taskState.filter).toEqual({});
          expect(warningState.items).toEqual([]);
          expect(scheduleState.loaded).toBe(false);
          expect(templateState.loaded).toBe(false);
          expect(calendarFocusState.chunkId).toBeNull();
        },
      },
    ])('switchTo success: $label', async ({ setup, extraAssert }) => {
      setup();
      client.switchProfile.mockResolvedValue(SWITCHED);
      await profile.switchTo(BOB.id);
      expect(client.switchProfile).toHaveBeenCalledWith(BOB.id);
      expect(profile.active).toEqual(SWITCHED);
      expect(profile.status?.last_used).toBe(BOB.id);
      expect(profile.switching).toBe(false);
      extraAssert();
    });

    it('a failure toasts an error, keeps the stores, and re-syncs status', async () => {
      client.switchProfile.mockRejectedValue({ error: 'internal', message: 'boom' });
      // The re-sync sees the backend's post-failure truth: no active profile.
      client.profileStatus.mockResolvedValue({
        active: null,
        profiles: [ALICE, BOB],
        last_used: ALICE.id,
      });
      taskState.selectedId = 't-1';

      await profile.switchTo(BOB.id);

      expect(toastState.items.some((t) => t.text === 'Could not switch profiles.')).toBe(true);
      expect(taskState.selectedId).toBe('t-1');
      expect(profile.active).toBeNull();
      expect(profile.switching).toBe(false);
    });

    it('ignores re-entrant calls while a switch is in flight', async () => {
      let resolveSwitch: (value: ActiveProfile) => void = () => {};
      client.switchProfile.mockImplementation(
        () =>
          new Promise((resolve) => {
            resolveSwitch = resolve;
          }),
      );

      const first = profile.switchTo(BOB.id);
      expect(profile.switching).toBe(true);
      await profile.switchTo(ALICE.id);
      expect(client.switchProfile).toHaveBeenCalledTimes(1);

      resolveSwitch(SWITCHED);
      await first;
      expect(profile.switching).toBe(false);
    });
  });
});
