// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import { toastState } from '../../stores/toast.svelte';
import type { ProfileInfo } from '../../types';
import { ALICE, BOB, aliceActive, bobActive } from './profileFixtures';
import {
  flush,
  makeTestProfileState,
  resetProfileStores,
  setInputValue,
} from './profileTestSupport';
import ProfilesView from './ProfilesView.svelte';

afterEach(resetProfileStores);

describe('ProfilesView — rendering', () => {
  it('shows the active profile and lists the others with a Switch button', async () => {
    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockResolvedValue(aliceActive());
    const { getByText, getAllByText } = render(ProfilesView, { store });
    await flush();

    expect(getByText('Alice')).toBeTruthy();
    expect(getAllByText('Bob').length).toBeGreaterThan(0);
    expect(getByText('Switch')).toBeTruthy();
  });

  it('shows a retry button when loading fails, and retries on click', async () => {
    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockRejectedValueOnce({ error: 'internal', message: 'boom' });
    profileStatus.mockResolvedValueOnce(aliceActive());

    const { getByText } = render(ProfilesView, { store });
    await flush();

    expect(getByText('Could not load profiles.')).toBeTruthy();
    await fireEvent.click(getByText('Retry'));
    await flush();

    expect(getByText('Alice')).toBeTruthy();
  });

  it('keeps the danger zone apart: Delete… never sits in the profile rows', async () => {
    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockResolvedValue(aliceActive());
    const { getByText, getAllByText } = render(ProfilesView, { store });
    await flush();

    const dangerCard = getByText('Danger zone').closest('.settings-card')!;
    const deleteButtons = getAllByText('Delete…');
    expect(deleteButtons).toHaveLength(1);
    expect(dangerCard.contains(deleteButtons[0]!)).toBe(true);
    expect(deleteButtons[0]!.classList.contains('btn-danger')).toBe(true);
    expect(getByText('Switch').closest('.settings-card')).not.toBe(dangerCard);
  });

  it('the danger zone explains itself when only the active profile exists', async () => {
    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockResolvedValue({
      active: { id: ALICE.id, name: ALICE.name },
      profiles: [ALICE],
      last_used: ALICE.id,
    });
    const { getByText, queryByText } = render(ProfilesView, { store });
    await flush();

    expect(getByText(/Nothing to delete/)).toBeTruthy();
    expect(queryByText('Delete…')).toBeNull();
  });
});

describe('ProfilesView — switch', () => {
  it('switches immediately on click without any confirmation', async () => {
    const { store, profileStatus, switchProfile } = makeTestProfileState();
    profileStatus.mockResolvedValue(aliceActive());
    switchProfile.mockResolvedValue({ id: BOB.id, name: BOB.name });

    const { getByText, queryByRole } = render(ProfilesView, { store });
    await flush();

    await fireEvent.click(getByText('Switch'));
    await flush();

    expect(queryByRole('alertdialog')).toBeNull();
    expect(switchProfile).toHaveBeenCalledWith(BOB.id);
    expect(store.active).toEqual({ id: BOB.id, name: BOB.name });
    expect(toastState.items.some((t) => t.text === 'Switched to "Bob".')).toBe(true);
  });

  it('a switch failure shows an error toast', async () => {
    const { store, profileStatus, switchProfile } = makeTestProfileState();
    profileStatus.mockResolvedValue(aliceActive());
    switchProfile.mockRejectedValue({ error: 'internal', message: 'boom' });

    const { getByText } = render(ProfilesView, { store });
    await flush();

    await fireEvent.click(getByText('Switch'));
    await flush();

    expect(toastState.items.some((t) => t.text === 'Could not switch profiles.')).toBe(true);
  });
});

describe('ProfilesView — create profile', () => {
  it('creates a profile, toasts, and refreshes the list', async () => {
    const carol: ProfileInfo = {
      id: 'p-carol',
      name: 'Carol',
      created_at: '2026-07-03T00:00:00Z',
    };
    const { store, profileStatus } = makeTestProfileState();
    const createProfile = vi.fn<(name: string) => Promise<ProfileInfo>>();
    profileStatus.mockResolvedValueOnce(aliceActive());
    createProfile.mockResolvedValue(carol);
    profileStatus.mockResolvedValueOnce({
      ...aliceActive(),
      profiles: [ALICE, BOB, carol],
    });

    const { getByText, getAllByText, getByLabelText, container } = render(ProfilesView, {
      store,
      createProfile,
    });
    await flush();

    await fireEvent.click(getByText('Add profile'));
    await setInputValue(getByLabelText('Profile name') as HTMLInputElement, 'Carol');
    await fireEvent.submit(container.querySelector('form')!);
    await flush();
    await flush();

    expect(createProfile).toHaveBeenCalledWith('Carol');
    expect(toastState.items.some((t) => t.text === 'Profile "Carol" created.')).toBe(true);
    expect(profileStatus).toHaveBeenCalledTimes(2);
    // Non-active profiles render twice: switch row + danger-zone delete row.
    expect(getAllByText('Carol')).toHaveLength(2);
  });

  it('rejects an empty name client-side without calling the backend', async () => {
    const { store, profileStatus } = makeTestProfileState();
    const createProfile = vi.fn<(name: string) => Promise<ProfileInfo>>();
    profileStatus.mockResolvedValue(aliceActive());
    const { getByText, container } = render(ProfilesView, { store, createProfile });
    await flush();

    await fireEvent.click(getByText('Add profile'));
    await fireEvent.submit(container.querySelector('form')!);

    expect(getByText('Profile name is required.')).toBeTruthy();
    expect(createProfile).not.toHaveBeenCalled();
  });
});

type RenderProps = {
  store: ReturnType<typeof makeTestProfileState>['store'];
  createProfile?: (name: string) => Promise<ProfileInfo>;
  renameProfile?: (id: string, name: string) => Promise<ProfileInfo>;
  deleteProfile?: (id: string) => Promise<void>;
};

describe('ProfilesView — rename', () => {
  async function openRenameForm(renderProps: RenderProps, index = 0) {
    const utils = render(ProfilesView, renderProps);
    await flush();
    await fireEvent.click(utils.getAllByText('Rename…')[index]!);
    const input = utils.getByLabelText('New profile name') as HTMLInputElement;
    return { ...utils, input };
  }

  function makeRenameSetup() {
    const { store, profileStatus } = makeTestProfileState();
    const renameProfile = vi.fn<(id: string, name: string) => Promise<ProfileInfo>>();
    profileStatus.mockResolvedValue(aliceActive());
    return { store, profileStatus, renameProfile };
  }

  type StatusMock = ReturnType<typeof makeTestProfileState>['profileStatus'];

  it.each([
    {
      label: 'active profile',
      profileIndex: 0,
      expectedInitialName: ALICE.name,
      newName: 'Dana',
      expectedId: ALICE.id,
      renamedProfile: { ...ALICE, name: 'Dana' } as ProfileInfo,
      setupStatus: (ps: StatusMock) => {
        ps.mockResolvedValueOnce(aliceActive());
        ps.mockResolvedValueOnce({
          active: { id: ALICE.id, name: 'Dana' },
          profiles: [{ ...ALICE, name: 'Dana' }, BOB],
          last_used: ALICE.id,
        });
      },
      postCheck: (getByText: (t: string) => HTMLElement, ps: StatusMock) => {
        expect(toastState.items.some((t) => t.text === 'Profile renamed to "Dana".')).toBe(true);
        expect(ps).toHaveBeenCalledTimes(2);
        expect(getByText('Dana')).toBeTruthy();
      },
    },
    {
      label: 'non-active profile',
      profileIndex: 1,
      expectedInitialName: BOB.name,
      newName: 'Robert',
      expectedId: BOB.id,
      renamedProfile: { ...BOB, name: 'Robert' } as ProfileInfo,
      setupStatus: (ps: StatusMock) => {
        ps.mockResolvedValue(aliceActive());
      },
      postCheck: (_getByText: (t: string) => HTMLElement, ps: StatusMock) => {
        expect(toastState.items.some((t) => t.text === 'Profile renamed to "Robert".')).toBe(true);
        expect(ps).toHaveBeenCalledTimes(2);
      },
    },
  ])(
    'renames $label successfully',
    async ({
      profileIndex,
      expectedInitialName,
      newName,
      expectedId,
      renamedProfile,
      setupStatus,
      postCheck,
    }) => {
      const { store, profileStatus } = makeTestProfileState();
      const renameProfile = vi.fn<(id: string, name: string) => Promise<ProfileInfo>>();
      setupStatus(profileStatus);
      renameProfile.mockResolvedValue(renamedProfile);

      const { input, getByText } = await openRenameForm({ store, renameProfile }, profileIndex);
      expect(input.value).toBe(expectedInitialName);

      await setInputValue(input, newName);
      await fireEvent.submit(input.closest('form')!);
      await flush();
      await flush();

      expect(renameProfile).toHaveBeenCalledWith(expectedId, newName);
      postCheck(getByText, profileStatus);
    },
  );

  it('rejects an empty name client-side without calling the backend', async () => {
    const { store, renameProfile } = makeRenameSetup();
    const { input, getByText } = await openRenameForm({ store, renameProfile });
    await setInputValue(input, '   ');
    await fireEvent.submit(input.closest('form')!);

    expect(getByText('Profile name is required.')).toBeTruthy();
    expect(renameProfile).not.toHaveBeenCalled();
  });

  it('surfaces a duplicate-name error from the backend inline', async () => {
    const { store, renameProfile } = makeRenameSetup();
    renameProfile.mockRejectedValue({
      error: 'validation',
      message: "A profile named 'Bob' already exists.",
    });

    const { input, getByText } = await openRenameForm({ store, renameProfile });
    await setInputValue(input, 'Bob');
    await fireEvent.submit(input.closest('form')!);
    await flush();

    expect(getByText("A profile named 'Bob' already exists.")).toBeTruthy();
  });

  it('cancel closes the form without calling the backend', async () => {
    const { store, renameProfile } = makeRenameSetup();
    const { getAllByText, getByLabelText, queryByLabelText, getByText } = render(ProfilesView, {
      store,
      renameProfile,
    });
    await flush();

    await fireEvent.click(getAllByText('Rename…')[0]!);
    expect(getByLabelText('New profile name')).toBeTruthy();

    await fireEvent.click(getByText('Cancel'));
    expect(queryByLabelText('New profile name')).toBeNull();
    expect(renameProfile).not.toHaveBeenCalled();
  });
});

describe('ProfilesView — delete', () => {
  it('deletes a profile after the confirm, toasts, and refreshes', async () => {
    const { store, profileStatus } = makeTestProfileState();
    const deleteProfile = vi.fn<(id: string) => Promise<void>>();
    profileStatus.mockResolvedValueOnce(bobActive());
    deleteProfile.mockResolvedValue(undefined);
    profileStatus.mockResolvedValueOnce({
      ...bobActive(),
      profiles: [BOB],
    });

    const { getByText, queryByText, container } = render(ProfilesView, { store, deleteProfile });
    await flush();

    await fireEvent.click(getByText('Delete…'));
    expect(getByText('Delete "Alice"?')).toBeTruthy();

    await fireEvent.submit(container.querySelector('form')!);
    await flush();
    await flush();

    expect(deleteProfile).toHaveBeenCalledWith(ALICE.id);
    expect(toastState.items.some((t) => t.text === 'Profile "Alice" deleted.')).toBe(true);
    expect(profileStatus).toHaveBeenCalledTimes(2);
    expect(queryByText('Alice')).toBeNull();
  });

  it('the confirm button is a danger button', async () => {
    const { store, profileStatus } = makeTestProfileState();
    profileStatus.mockResolvedValue(aliceActive());
    const { getByText } = render(ProfilesView, { store });
    await flush();

    await fireEvent.click(getByText('Delete…'));
    expect(getByText('Delete profile').classList.contains('btn-danger')).toBe(true);
  });

  it('surfaces a backend validation error inline', async () => {
    const { store, profileStatus } = makeTestProfileState();
    const deleteProfile = vi.fn<(id: string) => Promise<void>>();
    profileStatus.mockResolvedValue(aliceActive());
    deleteProfile.mockRejectedValue({
      error: 'validation',
      message: 'This profile is currently active — switch to another profile first.',
    });

    const { getByText, getAllByText, container } = render(ProfilesView, { store, deleteProfile });
    await flush();

    await fireEvent.click(getByText('Delete…'));
    await fireEvent.submit(container.querySelector('form')!);
    await flush();

    expect(
      getByText('This profile is currently active — switch to another profile first.'),
    ).toBeTruthy();
    expect(getAllByText('Bob').length).toBeGreaterThan(0);
  });

  it('cancel closes the confirm without calling the backend', async () => {
    const { store, profileStatus } = makeTestProfileState();
    const deleteProfile = vi.fn<(id: string) => Promise<void>>();
    profileStatus.mockResolvedValue(aliceActive());
    const { getByText, queryByText } = render(ProfilesView, { store, deleteProfile });
    await flush();

    await fireEvent.click(getByText('Delete…'));
    expect(getByText('Delete "Bob"?')).toBeTruthy();

    await fireEvent.click(getByText('Cancel'));
    expect(queryByText('Delete "Bob"?')).toBeNull();
    expect(deleteProfile).not.toHaveBeenCalled();
  });

  it('opening Delete closes an open rename form (and vice versa)', async () => {
    const { store, profileStatus } = makeTestProfileState();
    const renameProfile = vi.fn<(id: string, name: string) => Promise<ProfileInfo>>();
    const deleteProfile = vi.fn<(id: string) => Promise<void>>();
    profileStatus.mockResolvedValue(aliceActive());
    const { getByText, getAllByText, queryByLabelText, queryByText } = render(ProfilesView, {
      store,
      renameProfile,
      deleteProfile,
    });
    await flush();

    await fireEvent.click(getAllByText('Rename…')[1]!);
    expect(queryByLabelText('New profile name')).toBeTruthy();

    await fireEvent.click(getByText('Delete…'));
    expect(queryByLabelText('New profile name')).toBeNull();
    expect(getByText('Delete "Bob"?')).toBeTruthy();

    await fireEvent.click(getAllByText('Rename…')[1]!);
    expect(queryByText('Delete "Bob"?')).toBeNull();
    expect(queryByLabelText('New profile name')).toBeTruthy();
  });
});
