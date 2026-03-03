// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { createConfirmHost } from '../../actions/confirmHost.svelte';
import type { ConfirmSpec } from '../../actions/taskActions';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importDialog() {
  const mod = await import('./ConfirmHostDialog.svelte');
  return mod.default;
}

function makeSpec(overrides: Partial<ConfirmSpec> = {}): ConfirmSpec {
  return {
    title: 'Delete task',
    message: 'This cannot be undone.',
    confirmLabel: 'Delete',
    destructive: true,
    ...overrides,
  };
}

async function renderDialog() {
  const Dialog = await importDialog();
  const host = createConfirmHost();
  return { host, ...render(Dialog, { host }) };
}

async function clickButtonAndGetSettle(spec: ConfirmSpec, buttonName: string) {
  const { host, getByRole } = await renderDialog();
  const settle = vi.spyOn(host, 'settle');
  void host.request(spec);
  await tick();
  await fireEvent.click(getByRole('button', { name: buttonName }));
  return settle;
}

describe('ConfirmHostDialog', () => {
  it('is closed when spec is null (dialog not in DOM)', async () => {
    const { queryByRole } = await renderDialog();
    expect(queryByRole('alertdialog')).toBeNull();
  });

  it('opens and shows the dialog when spec is set', async () => {
    const { host, getByRole } = await renderDialog();
    void host.request(makeSpec({ title: 'Remove' }));
    await tick();
    expect(getByRole('alertdialog')).toBeTruthy();
  });

  it('shows the spec title and message inside the dialog', async () => {
    const { host, getByRole, getByText } = await renderDialog();
    void host.request(makeSpec({ title: 'Confirm removal', message: 'All data will be lost.' }));
    await tick();
    expect(getByRole('heading', { name: 'Confirm removal' })).toBeTruthy();
    expect(getByText('All data will be lost.')).toBeTruthy();
  });

  it.each([{ confirmLabel: 'Yes, remove it' }, { confirmLabel: 'OK' }])(
    'shows $confirmLabel button for spec.confirmLabel',
    async ({ confirmLabel }) => {
      const { host, getByRole } = await renderDialog();
      void host.request(makeSpec({ confirmLabel }));
      await tick();
      expect(getByRole('button', { name: confirmLabel })).toBeTruthy();
    },
  );

  it.each([
    { buttonName: 'Delete', expected: true },
    { buttonName: 'Cancel', expected: false },
  ])('clicking $buttonName calls host.settle($expected)', async ({ buttonName, expected }) => {
    const settle = await clickButtonAndGetSettle(makeSpec({ confirmLabel: 'Delete' }), buttonName);
    expect(settle).toHaveBeenCalledWith(expected);
  });
});
