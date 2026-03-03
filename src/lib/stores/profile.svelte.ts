// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { ActiveProfile, ProfileInfo, ProfileStatus } from '../types';
import * as api from '../api';
import { toastState } from './toast.svelte';
import { taskState } from './tasks.svelte';
import { warningState } from './warnings.svelte';
import { scheduleState } from './schedules.svelte';
import { templateState } from './templates.svelte';
import { calendarFocusState } from './calendarFocus.svelte';

interface ProfileClient {
  profileStatus: () => Promise<ProfileStatus>;
  switchProfile: (id: string) => Promise<ActiveProfile>;
}

const defaultClient: ProfileClient = {
  profileStatus: api.profileStatus,
  switchProfile: api.switchProfile,
};

/**
 * Cross-view profile state: backs the App gate, the sidebar switcher, and
 * the profiles management page. Owns the in-place profile switch — the app
 * no longer restarts; `switchTo` swaps the backend state, resets every
 * profile-scoped store, and the `{#key active.id}` block in App.svelte
 * remounts the Shell so all views refetch for the new profile.
 */
export class ProfileState {
  status: ProfileStatus | null = $state(null);
  loadError: string | null = $state(null);
  switching: boolean = $state(false);

  active: ActiveProfile | null = $derived.by(() => this.status?.active ?? null);

  /** Every profile except the active one (switch targets, delete candidates). */
  others: ProfileInfo[] = $derived.by(() => {
    if (this.status === null || this.status.active === null) return [];
    const activeId = this.status.active.id;
    return this.status.profiles.filter((p) => p.id !== activeId);
  });

  readonly #client: ProfileClient;

  constructor(client: ProfileClient = defaultClient) {
    this.#client = client;
  }

  async load(): Promise<void> {
    try {
      this.status = await this.#client.profileStatus();
      this.loadError = null;
    } catch (e) {
      this.loadError = api.apiErrorMessage(e, 'Could not load profiles.');
    }
  }

  /** The gate unlocked a profile — reflect it without another round-trip. */
  setActive(profile: ActiveProfile): void {
    if (this.status !== null) {
      this.status = { ...this.status, active: profile, last_used: profile.id };
    }
  }

  /** Switch the active profile in place (no confirmation, no restart). */
  async switchTo(id: string): Promise<void> {
    if (this.switching) return;
    this.switching = true;
    try {
      const active = await this.#client.switchProfile(id);
      if (this.status !== null) {
        this.status = { ...this.status, active, last_used: active.id };
      }
      taskState.reset();
      warningState.clear();
      scheduleState.reset();
      templateState.reset();
      calendarFocusState.clear();
      toastState.success(`Switched to "${active.name}".`);
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Could not switch profiles.'));
      // A failed switch can leave no profile active (the old one is flushed
      // and released before the new one activates) — re-sync so the App
      // gate takes over instead of every view erroring against an empty slot.
      // Safe to await un-caught: load() traps its own errors into loadError
      // and never rejects, so this cannot escape switchTo.
      await this.load();
    } finally {
      this.switching = false;
    }
  }
}

export const profileState = new ProfileState();
