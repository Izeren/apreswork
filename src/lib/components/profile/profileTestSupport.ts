// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared helpers for profile component tests. Not collected by vitest (no .test suffix).

import { tick } from 'svelte';
import { cleanup, fireEvent } from '@testing-library/svelte';
import { vi } from 'vitest';
import { toastState } from '../../stores/toast.svelte';
import { profileState, ProfileState } from '../../stores/profile.svelte';
import type { ActiveProfile, ProfileStatus } from '../../types';

export async function setInputValue(el: HTMLInputElement, value: string) {
  el.value = value;
  await fireEvent.input(el);
}

export async function flush() {
  await Promise.resolve();
  await tick();
}

export function makeTestProfileState() {
  const profileStatus = vi.fn<() => Promise<ProfileStatus>>();
  const switchProfile = vi.fn<(id: string) => Promise<ActiveProfile>>();
  const store = new ProfileState({ profileStatus, switchProfile });
  return { store, profileStatus, switchProfile };
}

export function resetProfileStores() {
  cleanup();
  vi.clearAllMocks();
  toastState.items = [];
  profileState.status = null;
  profileState.loadError = null;
  profileState.switching = false;
  window.location.hash = '';
}
