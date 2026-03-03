// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockInstance } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { toastState } from '../../stores/toast.svelte';
import { QUICK_DATE_ANCHOR_STORAGE_KEY } from '../../quickDateAnchorPref';
import type { AppConfig, UpdateConfigInput } from '../../types';
import { apiErrorMessage } from '../../api';
import { installLocalStorageStub } from '../../storageStubHooks';
import type { SchedulingSectionApi } from './schedulingSectionShared';

installLocalStorageStub();

const CONFIG: AppConfig = {
  planning_horizon_days: 30,
  timezone: 'Europe/Berlin',
  max_continuous_minutes: 120,
  min_break_minutes: 5,
  last_reschedule: null,
  last_mutation: null,
  last_sync: null,
  last_busy_sync: null,
};

let fakeApi: {
  getConfig: MockInstance<() => Promise<AppConfig>>;
  updateConfig: MockInstance<(c: UpdateConfigInput) => Promise<AppConfig>>;
  apiErrorMessage: (e: unknown, fallback: string) => string;
} & SchedulingSectionApi;

beforeEach(() => {
  fakeApi = {
    getConfig: vi.fn<() => Promise<AppConfig>>().mockResolvedValue(CONFIG),
    updateConfig: vi.fn<(c: UpdateConfigInput) => Promise<AppConfig>>().mockResolvedValue(CONFIG),
    apiErrorMessage: (e, fallback) => apiErrorMessage(e, fallback),
  };
});

afterEach(() => {
  toastState.items = [];
});

async function importSection() {
  const mod = await import('./SchedulingSection.svelte');
  return mod.default;
}

async function flush() {
  await Promise.resolve();
  await tick();
}

async function renderLoaded() {
  const SchedulingSection = await importSection();
  const utils = render(SchedulingSection, { props: { apiClient: fakeApi } });
  await flush();
  return utils;
}

function input(utils: { getByLabelText: (t: string) => HTMLElement }, label: string) {
  const el = utils.getByLabelText(label);
  if (!(el instanceof HTMLInputElement)) throw new Error(`Expected input for "${label}"`);
  return el;
}

describe('SchedulingSection — loading', () => {
  it('renders the loaded config values; Save is disabled while pristine', async () => {
    const utils = await renderLoaded();

    expect(input(utils, 'Planning horizon (days)').value).toBe('30');
    expect(input(utils, 'Max continuous work (minutes)').value).toBe('120');
    expect(input(utils, 'Break between work blocks (minutes)').value).toBe('5');
    expect((utils.getByLabelText('Timezone') as HTMLSelectElement).value).toBe('Europe/Berlin');
    expect((utils.getByText('Save scheduling settings') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows a load error with Retry; retry reloads the form', async () => {
    fakeApi.getConfig.mockRejectedValueOnce({ error: 'database', message: 'boom' });
    const utils = await renderLoaded();

    expect(utils.getByText('Could not load scheduling settings.')).toBeTruthy();

    await fireEvent.click(utils.getByText('Retry'));
    await flush();

    expect(fakeApi.getConfig).toHaveBeenCalledTimes(2);
    expect(input(utils, 'Planning horizon (days)').value).toBe('30');
  });

  it('keeps a stored timezone that the runtime list does not know', async () => {
    fakeApi.getConfig.mockResolvedValue({ ...CONFIG, timezone: 'Legacy/Zone' });
    const utils = await renderLoaded();

    expect((utils.getByLabelText('Timezone') as HTMLSelectElement).value).toBe('Legacy/Zone');
  });
});

describe('SchedulingSection — saving', () => {
  type RenderUtils = Awaited<ReturnType<typeof renderLoaded>>;

  it.each([
    {
      label: 'horizon field',
      setup: () => {
        fakeApi.updateConfig.mockResolvedValue({ ...CONFIG, planning_horizon_days: 60 });
      },
      edit: async (utils: RenderUtils) => {
        await fireEvent.input(input(utils, 'Planning horizon (days)'), { target: { value: '60' } });
      },
      check: (utils: RenderUtils) => {
        expect(fakeApi.updateConfig).toHaveBeenCalledWith({
          planning_horizon_days: 60,
          max_continuous_minutes: 120,
          min_break_minutes: 5,
          timezone: 'Europe/Berlin',
        });
        expect(toastState.items.map((t) => t.text)).toContain('Scheduling settings saved.');
        expect((utils.getByText('Save scheduling settings') as HTMLButtonElement).disabled).toBe(
          true,
        );
      },
    },
    {
      label: 'timezone field',
      setup: () => {},
      edit: async (utils: RenderUtils) => {
        await fireEvent.change(utils.getByLabelText('Timezone'), {
          target: { value: 'America/New_York' },
        });
      },
      check: () => {
        expect(fakeApi.updateConfig).toHaveBeenCalledWith(
          expect.objectContaining({ timezone: 'America/New_York' }),
        );
      },
    },
    {
      label: 'max continuous minutes field',
      setup: () => {
        fakeApi.updateConfig.mockResolvedValue({ ...CONFIG, max_continuous_minutes: 90 });
      },
      edit: async (utils: RenderUtils) => {
        await fireEvent.input(input(utils, 'Max continuous work (minutes)'), {
          target: { value: '90' },
        });
      },
      check: () => {
        expect(fakeApi.updateConfig).toHaveBeenCalledWith(
          expect.objectContaining({ max_continuous_minutes: 90 }),
        );
        expect(toastState.items.map((t) => t.text)).toContain('Scheduling settings saved.');
      },
    },
    {
      label: 'min break minutes field',
      setup: () => {
        fakeApi.updateConfig.mockResolvedValue({ ...CONFIG, min_break_minutes: 10 });
      },
      edit: async (utils: RenderUtils) => {
        await fireEvent.input(input(utils, 'Break between work blocks (minutes)'), {
          target: { value: '10' },
        });
      },
      check: () => {
        expect(fakeApi.updateConfig).toHaveBeenCalledWith(
          expect.objectContaining({ min_break_minutes: 10 }),
        );
      },
    },
  ])(
    '$label: enables Save once edited and sends the patched value',
    async ({ setup, edit, check }) => {
      setup();
      const utils = await renderLoaded();
      await edit(utils);
      const save = utils.getByText('Save scheduling settings') as HTMLButtonElement;
      expect(save.disabled).toBe(false);
      await fireEvent.click(save);
      await flush();
      check(utils);
    },
  );

  it('surfaces a backend validation message verbatim and stays editable', async () => {
    fakeApi.updateConfig.mockRejectedValue({
      error: 'validation',
      message: 'planning_horizon_days must be between 1 and 365',
    });
    const utils = await renderLoaded();

    await fireEvent.input(input(utils, 'Planning horizon (days)'), { target: { value: '999' } });
    await fireEvent.click(utils.getByText('Save scheduling settings'));
    await flush();

    expect(toastState.items.map((t) => t.text)).toContain(
      'planning_horizon_days must be between 1 and 365',
    );
    expect((utils.getByText('Save scheduling settings') as HTMLButtonElement).disabled).toBe(false);
  });

  it('rejects an empty numeric field locally without calling the backend', async () => {
    const utils = await renderLoaded();

    await fireEvent.input(input(utils, 'Break between work blocks (minutes)'), {
      target: { value: '' },
    });
    await fireEvent.click(utils.getByText('Save scheduling settings'));
    await flush();

    expect(fakeApi.updateConfig).not.toHaveBeenCalled();
    expect(toastState.items.map((t) => t.text)).toContain(
      'Break between work blocks must be a whole number.',
    );
  });
});

describe('SchedulingSection — quick-date anchor', () => {
  type AnchorCase = {
    label: string;
    arrange: () => void;
    act: (utils: Awaited<ReturnType<typeof renderLoaded>>) => Promise<void>;
    assert: (utils: Awaited<ReturnType<typeof renderLoaded>>) => void;
  };

  it.each<AnchorCase>([
    {
      label: 'defaults to Auto; clicking Friday persists to localStorage',
      arrange: () => {},
      act: async (utils) => {
        expect(utils.getByText('Auto').getAttribute('aria-pressed')).toBe('true');
        await fireEvent.click(utils.getByText('Friday'));
      },
      assert: (utils) => {
        expect(window.localStorage.getItem(QUICK_DATE_ANCHOR_STORAGE_KEY)).toBe('fri');
        expect(utils.getByText('Friday').getAttribute('aria-pressed')).toBe('true');
        expect(utils.getByText('Auto').getAttribute('aria-pressed')).toBe('false');
      },
    },
    {
      label: 'loads a persisted anchor on mount',
      arrange: () => {
        window.localStorage.setItem(QUICK_DATE_ANCHOR_STORAGE_KEY, 'sun');
      },
      act: async () => {},
      assert: (utils) => {
        expect(utils.getByText('Sunday').getAttribute('aria-pressed')).toBe('true');
      },
    },
  ])('$label', async ({ arrange, act, assert: doAssert }) => {
    arrange();
    const utils = await renderLoaded();
    await act(utils);
    doAssert(utils);
  });
});
