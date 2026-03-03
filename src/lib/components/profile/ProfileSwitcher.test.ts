// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import ProfileSwitcher from './ProfileSwitcher.svelte';
import { BOB, aliceActive } from './profileFixtures';
import { flush, resetProfileStores, makeTestProfileState } from './profileTestSupport';

afterEach(resetProfileStores);

describe('ProfileSwitcher', () => {
  type StoreT = ReturnType<typeof makeTestProfileState>['store'];

  it.each([
    {
      label: 'shows active profile name (Alice) on toggle',
      setup: (store: StoreT) => {
        store.status = aliceActive();
      },
      namePattern: /Alice/,
    },
    {
      label: 'falls back to generic label when no profile known',
      setup: () => {},
      namePattern: /Profile/,
    },
  ])('toggle button: $label', ({ setup, namePattern }) => {
    const { store } = makeTestProfileState();
    setup(store);
    const { getByRole } = render(ProfileSwitcher, { store });
    const button = getByRole('button', { name: namePattern });
    expect(button).toBeTruthy();
    expect(button.getAttribute('aria-haspopup')).toBe('menu');
    expect(button.getAttribute('aria-expanded')).toBe('false');
  });

  it('opens a menu listing the other profiles and the manage link', async () => {
    const { store } = makeTestProfileState();
    store.status = aliceActive();
    const { getByRole, getByText } = render(ProfileSwitcher, { store });

    await fireEvent.click(getByRole('button', { name: /Alice/ }));

    expect(getByRole('button', { name: /Alice/ }).getAttribute('aria-expanded')).toBe('true');
    expect(getByText('Switch to Bob')).toBeTruthy();
    expect(getByText('Manage profiles…')).toBeTruthy();
  });

  it('switches directly from the menu — no confirmation dialog', async () => {
    const { store, switchProfile } = makeTestProfileState();
    store.status = aliceActive();
    switchProfile.mockResolvedValue({ id: BOB.id, name: BOB.name });
    const { getByRole, getByText, queryByRole } = render(ProfileSwitcher, { store });

    await fireEvent.click(getByRole('button', { name: /Alice/ }));
    await fireEvent.click(getByText('Switch to Bob'));
    await flush();

    expect(switchProfile).toHaveBeenCalledWith(BOB.id);
    expect(store.active).toEqual({ id: BOB.id, name: BOB.name });
    expect(queryByRole('menu')).toBeNull();
    expect(queryByRole('alertdialog')).toBeNull();
  });

  it('navigates to the profiles page from the manage item', async () => {
    const { store, switchProfile } = makeTestProfileState();
    store.status = aliceActive();
    const { getByRole, getByText } = render(ProfileSwitcher, { store });

    await fireEvent.click(getByRole('button', { name: /Alice/ }));
    await fireEvent.click(getByText('Manage profiles…'));

    expect(window.location.hash).toBe('#/profiles');
    expect(switchProfile).not.toHaveBeenCalled();
  });

  it('clicking the toggle again closes the menu', async () => {
    const { store } = makeTestProfileState();
    store.status = aliceActive();
    const { getByRole, queryByText } = render(ProfileSwitcher, { store });

    await fireEvent.click(getByRole('button', { name: /Alice/ }));
    expect(queryByText('Switch to Bob')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: /Alice/ }));
    expect(queryByText('Switch to Bob')).toBeNull();
  });

  // Real-life toggle click = pointerdown (the menu closes it as "outside")
  // followed by click — without the grace window the click would reopen it.
  it('does not reopen when the toggle is clicked within the close grace window', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    try {
      vi.setSystemTime(new Date('2026-07-13T12:00:00Z'));
      const { store } = makeTestProfileState();
      store.status = aliceActive();
      const { getByRole, queryByText } = render(ProfileSwitcher, { store });
      const toggle = getByRole('button', { name: /Alice/ });

      await fireEvent.click(toggle);
      expect(queryByText('Switch to Bob')).toBeTruthy();

      await fireEvent.pointerDown(toggle);
      expect(queryByText('Switch to Bob')).toBeNull();
      await fireEvent.click(toggle);
      expect(queryByText('Switch to Bob')).toBeNull();

      // Past the grace window the toggle opens the menu again.
      vi.setSystemTime(new Date('2026-07-13T12:00:01Z'));
      await fireEvent.click(toggle);
      expect(queryByText('Switch to Bob')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it('is disabled and shows progress while a switch is in flight', async () => {
    const { store } = makeTestProfileState();
    store.status = aliceActive();
    store.switching = true;
    const { getByRole } = render(ProfileSwitcher, { store });
    await tick();

    const button = getByRole('button', { name: /Switching…/ }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
