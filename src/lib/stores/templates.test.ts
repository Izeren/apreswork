// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { RecurringTemplate, CreateTemplateInput, UpdateTemplateInput } from '../types';

function buildClient() {
  return {
    listTemplates: vi.fn(),
    createTemplate: vi.fn(),
    updateTemplate: vi.fn(),
    deleteTemplate: vi.fn(),
  };
}

const mockTemplate = (overrides?: Partial<RecurringTemplate>): RecurringTemplate => ({
  id: 'tpl-1',
  title: 'Morning workout',
  description: 'Start the day right',
  duration_minutes: 30,
  priority: 'Medium',
  schedule_id: 'sched-1',
  cadence: {
    period: 'Weekly',
    interval: 1,
    windows: [
      { start: 0, end: 0 },
      { start: 2, end: 2 },
      { start: 4, end: 4 },
    ],
  },
  labels: ['health'],
  is_active: true,
  start_date: '2026-01-01T00:00:00Z',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  ...overrides,
});

describe('TemplateState', () => {
  let TemplateState: typeof import('./templates.svelte').TemplateState;
  let toastState: import('./toast.svelte').ToastState;
  let templates: InstanceType<typeof TemplateState>;
  let client!: ReturnType<typeof buildClient>;

  beforeEach(async () => {
    client = buildClient();
    const templateMod = await import('./templates.svelte');
    const toastMod = await import('./toast.svelte');
    TemplateState = templateMod.TemplateState;
    toastState = toastMod.toastState;
    toastState.items = [];
    templates = new TemplateState(client);
  });

  describe('load', () => {
    it('populates items, sets loaded=true, and manages loading states', async () => {
      const tpl1 = mockTemplate();
      const tpl2 = mockTemplate({ id: 'tpl-2', title: 'Evening walk' });
      client.listTemplates.mockResolvedValue([tpl1, tpl2]);

      expect(templates.loading).toBe(false);
      const promise = templates.load();
      expect(templates.loading).toBe(true);
      await promise;

      expect(templates.loading).toBe(false);
      expect(templates.loaded).toBe(true);
      expect(templates.items).toEqual([tpl1, tpl2]);
    });

    it('shows toast error and resets loading on failure, items unchanged', async () => {
      const existing = mockTemplate();
      templates.items = [existing];
      client.listTemplates.mockRejectedValue(new Error('network'));

      await templates.load();

      expect(templates.loading).toBe(false);
      expect(templates.loaded).toBe(false);
      expect(templates.items).toEqual([existing]);
      expect(toastState.items).toHaveLength(1);
      expect(toastState.items[0]).toMatchObject({
        level: 'error',
        text: 'Failed to load templates',
      });
    });

    it.each([
      {
        label: 'clears when result is empty',
        apiResult: [] as RecurringTemplate[],
        expectedId: null,
      },
      {
        label: 'preserves when template still present',
        apiResult: [mockTemplate()],
        expectedId: 'tpl-1',
      },
    ])('selectedId $label', async ({ apiResult, expectedId }) => {
      const tpl = mockTemplate();
      templates.items = [tpl];
      templates.select('tpl-1');
      client.listTemplates.mockResolvedValue(apiResult);
      await templates.load();
      expect(templates.selectedId).toBe(expectedId);
    });

    it.each<{ label: string; setup: (t: InstanceType<typeof TemplateState>) => void }>([
      {
        label: 'already loading',
        setup: (t) => {
          t.loading = true;
        },
      },
      {
        label: 'already loaded (loaded=true, force=false)',
        setup: (t) => {
          t.loaded = true;
        },
      },
    ])('skips load when $label', async ({ setup }) => {
      client.listTemplates.mockResolvedValue([]);
      setup(templates);

      await templates.load();

      expect(client.listTemplates).not.toHaveBeenCalled();
    });

    it('does NOT skip when loaded=true but force=true', async () => {
      client.listTemplates.mockResolvedValue([]);
      templates.loaded = true;

      await templates.load(true);

      expect(client.listTemplates).toHaveBeenCalledTimes(1);
    });
  });

  describe('create', () => {
    it.each([
      {
        label: 'success',
        setup: () => client.createTemplate.mockResolvedValue(mockTemplate()),
        expectedResult: mockTemplate() as RecurringTemplate | undefined,
        expectedItems: [mockTemplate()] as RecurringTemplate[],
        expectedLoaded: true,
        toastLevel: 'success' as const,
        toastText: 'Template created',
      },
      {
        label: 'failure',
        setup: () => client.createTemplate.mockRejectedValue(new Error('fail')),
        expectedResult: undefined as RecurringTemplate | undefined,
        expectedItems: [] as RecurringTemplate[],
        expectedLoaded: false,
        toastLevel: 'error' as const,
        toastText: 'Failed to create template',
      },
    ])(
      '$label: returns expected result, updates items and toast',
      async ({ setup, expectedResult, expectedItems, expectedLoaded, toastLevel, toastText }) => {
        setup();
        const input: CreateTemplateInput = {
          title: 'Morning workout',
          duration_minutes: 30,
          cadence: { period: 'Weekly', interval: 1, windows: [{ start: 0, end: 0 }] },
        };
        const result = await templates.create(input);
        expect(result).toEqual(expectedResult);
        expect(templates.items).toEqual(expectedItems);
        expect(templates.loaded).toBe(expectedLoaded);
        expect(toastState.items).toHaveLength(1);
        expect(toastState.items[0]).toMatchObject({ level: toastLevel, text: toastText });
      },
    );
  });

  describe('update (optimistic)', () => {
    it.each([
      {
        label: 'success',
        setupMock: () =>
          client.updateTemplate.mockResolvedValue(
            mockTemplate({ title: 'Patched', updated_at: '2026-02-01T00:00:00Z' }),
          ),
        expectedResult: mockTemplate({ title: 'Patched', updated_at: '2026-02-01T00:00:00Z' }) as
          | RecurringTemplate
          | undefined,
        expectedFinalItem: mockTemplate({ title: 'Patched', updated_at: '2026-02-01T00:00:00Z' }),
        expectedToastLevel: 'success' as const,
        expectedToastText: 'Template updated',
      },
      {
        label: 'error',
        setupMock: () => client.updateTemplate.mockRejectedValue(new Error('fail')),
        expectedResult: undefined as RecurringTemplate | undefined,
        expectedFinalItem: mockTemplate(),
        expectedToastLevel: 'error' as const,
        expectedToastText: 'Failed to update template',
      },
    ])(
      '$label: optimistic patch applied, then resolved or rolled back',
      async ({
        setupMock,
        expectedResult,
        expectedFinalItem,
        expectedToastLevel,
        expectedToastText,
      }) => {
        const tpl = mockTemplate();
        templates.items = [tpl];
        setupMock();

        const input: UpdateTemplateInput = { title: 'Patched' };
        const promise = templates.update('tpl-1', input);

        expect(templates.items[0].title).toBe('Patched');

        const result = await promise;

        expect(result).toEqual(expectedResult);
        expect(templates.items[0]).toEqual(expectedFinalItem);
        expect(toastState.items).toHaveLength(1);
        expect(toastState.items[0]).toMatchObject({
          level: expectedToastLevel,
          text: expectedToastText,
        });
      },
    );
  });

  describe('remove (optimistic)', () => {
    it.each([
      {
        label: 'success',
        setup: () => client.deleteTemplate.mockResolvedValue(undefined),
        expectedResult: true,
        expectedItems: [
          mockTemplate({ id: 'tpl-2', title: 'Evening walk' }),
        ] as RecurringTemplate[],
        expectedToastLevel: 'success' as const,
        expectedToastText: 'Template deleted',
      },
      {
        label: 'error',
        setup: () => client.deleteTemplate.mockRejectedValue(new Error('fail')),
        expectedResult: false,
        expectedItems: [
          mockTemplate(),
          mockTemplate({ id: 'tpl-2', title: 'Evening walk' }),
        ] as RecurringTemplate[],
        expectedToastLevel: 'error' as const,
        expectedToastText: 'Failed to delete template',
      },
    ])(
      '$label: result and items match expected outcome',
      async ({ setup, expectedResult, expectedItems, expectedToastLevel, expectedToastText }) => {
        const tpl1 = mockTemplate();
        const tpl2 = mockTemplate({ id: 'tpl-2', title: 'Evening walk' });
        templates.items = [tpl1, tpl2];
        setup();

        const result = await templates.remove('tpl-1');

        expect(result).toBe(expectedResult);
        expect(templates.items).toEqual(expectedItems);
        expect(toastState.items).toHaveLength(1);
        expect(toastState.items[0]).toMatchObject({
          level: expectedToastLevel,
          text: expectedToastText,
        });
      },
    );

    it.each([
      {
        label: 'clears selectedId when the deleted template was selected',
        setupSelectedId: 'tpl-1',
        expectedSelectedIdAfter: null as string | null,
      },
      {
        label: 'does not affect selectedId when removing a non-selected template',
        setupSelectedId: 'tpl-2',
        expectedSelectedIdAfter: 'tpl-2' as string | null,
      },
    ])('$label', async ({ setupSelectedId, expectedSelectedIdAfter }) => {
      const tpl1 = mockTemplate();
      const tpl2 = mockTemplate({ id: 'tpl-2' });
      templates.items = [tpl1, tpl2];
      templates.select(setupSelectedId);
      client.deleteTemplate.mockResolvedValue(undefined);

      await templates.remove('tpl-1');

      expect(templates.selectedId).toBe(expectedSelectedIdAfter);
    });
  });

  describe('toggleActive', () => {
    it.each<{ label: string; templateId: string; initialActive: boolean; expectedActive: boolean }>(
      [
        { label: 'true → false', templateId: 'tpl-1', initialActive: true, expectedActive: false },
        { label: 'false → true', templateId: 'tpl-1', initialActive: false, expectedActive: true },
      ],
    )('$label', async ({ templateId, initialActive, expectedActive }) => {
      const tpl = mockTemplate({ is_active: initialActive });
      templates.items = [tpl];
      const updated = mockTemplate({
        is_active: expectedActive,
        updated_at: '2026-02-01T00:00:00Z',
      });
      client.updateTemplate.mockResolvedValue(updated);
      const result = await templates.toggleActive(templateId);
      expect(client.updateTemplate).toHaveBeenCalledWith(templateId, { is_active: expectedActive });
      expect(result).toEqual(updated);
    });

    it('returns undefined for nonexistent id', async () => {
      const tpl = mockTemplate({ is_active: true });
      templates.items = [tpl];
      const result = await templates.toggleActive('nonexistent');
      expect(result).toBeUndefined();
      expect(client.updateTemplate).not.toHaveBeenCalled();
    });
  });

  describe('select', () => {
    it.each([
      { input: 'tpl-1' as string | null, expected: 'tpl-1' as string | null },
      { input: null as string | null, expected: null as string | null },
    ])('select($input) sets selectedId to $expected', ({ input, expected }) => {
      templates.select(input);
      expect(templates.selectedId).toBe(expected);
    });
  });

  describe('selected (derived)', () => {
    it.each<{ selectedId: string | null; expected: ReturnType<typeof mockTemplate> | undefined }>([
      { selectedId: 'tpl-1', expected: mockTemplate() },
      { selectedId: null, expected: undefined },
      { selectedId: 'nonexistent', expected: undefined },
    ])('selectedId=$selectedId', ({ selectedId, expected }) => {
      const tpl = mockTemplate();
      templates.items = [tpl];
      templates.select(selectedId);
      expect(templates.selected).toEqual(expected);
    });
  });

  describe('reset', () => {
    it('drops all profile-scoped state so the next load refetches', () => {
      templates.items = [mockTemplate()];
      templates.loaded = true;
      templates.select('tpl-1');

      templates.reset();

      expect(templates.items).toEqual([]);
      expect(templates.loading).toBe(false);
      expect(templates.loaded).toBe(false);
      expect(templates.selectedId).toBeNull();
    });
  });
});
