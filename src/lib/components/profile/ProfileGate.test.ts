// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import type { ProfileInfo } from '../../types';
import ProfileGate from './ProfileGate.svelte';
import { ALICE, BOB, makeStatus } from './profileFixtures';
import { flush, setInputValue } from './profileTestSupport';

afterEach(() => {
  cleanup();
});

describe('ProfileGate — picker', () => {
  it('lists profiles last-used first, then oldest-first', () => {
    const { container } = render(ProfileGate, {
      status: makeStatus({ last_used: BOB.id }),
      onUnlocked: vi.fn(),
    });

    const names = Array.from(container.querySelectorAll('.profile-name')).map(
      (el) => el.textContent,
    );
    expect(names).toEqual(['Bob', 'Alice']);
  });
});

describe('ProfileGate — unlock', () => {
  it('unlocks a profile immediately on click', async () => {
    const unlockProfile = vi.fn().mockResolvedValue({ id: ALICE.id, name: ALICE.name });
    const onUnlocked = vi.fn();
    const { getByText } = render(ProfileGate, {
      status: makeStatus(),
      onUnlocked,
      unlockProfile,
    });

    await fireEvent.click(getByText('Alice'));
    await flush();

    expect(unlockProfile).toHaveBeenCalledWith(ALICE.id);
    expect(onUnlocked).toHaveBeenCalledWith({ id: ALICE.id, name: ALICE.name });
  });

  it.each([
    {
      desc: 'validation error',
      rejection: {
        error: 'validation',
        message: 'A profile is already active — restart the app to switch.',
      },
      expectedText: 'A profile is already active — restart the app to switch.',
    },
    {
      desc: 'internal error',
      rejection: { error: 'internal', message: 'boom' },
      expectedText: 'Could not unlock the profile.',
    },
  ])('unlock failure ($desc) shows the right error', async ({ rejection, expectedText }) => {
    const unlockProfile = vi.fn().mockRejectedValue(rejection);
    const onUnlocked = vi.fn();
    const { getByText } = render(ProfileGate, { status: makeStatus(), onUnlocked, unlockProfile });

    await fireEvent.click(getByText('Alice'));
    await flush();

    expect(getByText(expectedText)).toBeTruthy();
    expect(onUnlocked).not.toHaveBeenCalled();
  });
});

describe('ProfileGate — create profile', () => {
  async function openCreateForm(
    createProfile?: (name: string) => Promise<ProfileInfo>,
    unlockProfile?: (id: string) => Promise<{ id: string; name: string }>,
  ) {
    const onUnlocked = vi.fn();
    const utils = render(ProfileGate, {
      status: makeStatus(),
      onUnlocked,
      ...(createProfile && { createProfile }),
      ...(unlockProfile && { unlockProfile }),
    });
    await fireEvent.click(utils.getByText('Add profile'));
    return { ...utils, onUnlocked };
  }

  it('rejects an empty name inline without calling the backend', async () => {
    const createProfile = vi.fn();
    const { getByText, container } = await openCreateForm(createProfile);

    await fireEvent.submit(container.querySelector('form')!);

    expect(getByText('Profile name is required.')).toBeTruthy();
    expect(createProfile).not.toHaveBeenCalled();
  });

  it('creates the profile (trimming the name), then unlocks it', async () => {
    const created: ProfileInfo = {
      id: 'p-carol',
      name: 'Carol',
      created_at: '2026-07-03T00:00:00Z',
    };
    const createProfile = vi.fn().mockResolvedValue(created);
    const unlockProfile = vi.fn().mockResolvedValue({ id: created.id, name: created.name });

    const { getByLabelText, container, onUnlocked } = await openCreateForm(
      createProfile,
      unlockProfile,
    );

    await setInputValue(getByLabelText('Profile name') as HTMLInputElement, '  Carol  ');
    await fireEvent.submit(container.querySelector('form')!);
    await flush();
    await flush();

    expect(createProfile).toHaveBeenCalledWith('Carol');
    expect(unlockProfile).toHaveBeenCalledWith(created.id);
    expect(onUnlocked).toHaveBeenCalledWith({ id: created.id, name: created.name });
  });

  it('surfaces a backend validation error (e.g. duplicate name) inline', async () => {
    const createProfile = vi.fn().mockRejectedValue({
      error: 'validation',
      message: 'A profile with this name already exists.',
    });
    const unlockProfile = vi.fn();

    const { getByText, getByLabelText, container } = await openCreateForm(
      createProfile,
      unlockProfile,
    );
    await setInputValue(getByLabelText('Profile name') as HTMLInputElement, 'Alice');
    await fireEvent.submit(container.querySelector('form')!);
    await flush();

    expect(getByText('A profile with this name already exists.')).toBeTruthy();
    expect(unlockProfile).not.toHaveBeenCalled();
  });

  it('keeps the created profile listed when the follow-up unlock fails', async () => {
    const created: ProfileInfo = {
      id: 'p-carol',
      name: 'Carol',
      created_at: '2026-07-03T00:00:00Z',
    };
    const createProfile = vi.fn().mockResolvedValue(created);
    const unlockProfile = vi.fn().mockRejectedValue({ error: 'internal', message: 'boom' });

    const { getByText, getByLabelText, container, onUnlocked } = await openCreateForm(
      createProfile,
      unlockProfile,
    );
    await setInputValue(getByLabelText('Profile name') as HTMLInputElement, 'Carol');
    await fireEvent.submit(container.querySelector('form')!);
    await flush();
    await flush();

    expect(onUnlocked).not.toHaveBeenCalled();
    // Back on the picker: the failure is visible and the fresh profile is
    // selectable for a retry.
    expect(getByText('Could not unlock the profile.')).toBeTruthy();
    expect(getByText('Carol')).toBeTruthy();
  });
});
