// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared, pure profile fixtures for the profile test files (ProfileSwitcher,
// ProfilesView, ProfileGate component tests + the ProfileState store test). Not
// collected by vitest (no .test suffix); no DOM/testing-library imports so the
// node-adjacent store test can pull it in cheaply.

import type { ProfileInfo, ProfileStatus } from '../../types';

export const ALICE: ProfileInfo = {
  id: 'p-alice',
  name: 'Alice',
  created_at: '2026-07-01T00:00:00Z',
};
export const BOB: ProfileInfo = { id: 'p-bob', name: 'Bob', created_at: '2026-07-02T00:00:00Z' };

function activeProfile(profile: ProfileInfo): ProfileStatus {
  return {
    active: { id: profile.id, name: profile.name },
    profiles: [ALICE, BOB],
    last_used: profile.id,
  };
}

export function aliceActive(): ProfileStatus {
  return activeProfile(ALICE);
}

/** Status with Bob active. */
export function bobActive(): ProfileStatus {
  return activeProfile(BOB);
}

/** Gate-picker status (no active profile); override fields as needed. */
export function makeStatus(overrides: Partial<ProfileStatus> = {}): ProfileStatus {
  return {
    active: null,
    profiles: [ALICE, BOB],
    last_used: null,
    ...overrides,
  };
}
