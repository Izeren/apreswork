// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockedFunction } from 'vitest';
import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Cadence } from '../../types';
import type { SharedFieldValues } from './SharedFormFields.svelte';
import { baseSchedule, baseTemplate } from './testFixtures';
import { resetTaskListStores } from './taskListViewTestSupport';
import { TemplateState } from '../../stores/templates.svelte';
import type { TemplatesClient } from '../../stores/templates.svelte';
import { TaskState } from '../../stores/tasks.svelte';
import type { TasksClient } from '../../stores/tasks.svelte';
import { scheduleState } from '../../stores/schedules.svelte';

afterEach(async () => {
  cleanup();
  vi.clearAllMocks();
  await resetTaskListStores();
});

type MockedTemplateClient = { [K in keyof TemplatesClient]: MockedFunction<TemplatesClient[K]> };
type MockedTaskClient = { [K in keyof TasksClient]: MockedFunction<TasksClient[K]> };

function makeStores(
  templateOverrides: Partial<MockedTemplateClient> = {},
  taskOverrides: Partial<MockedTaskClient> = {},
) {
  const templateClient: MockedTemplateClient = {
    listTemplates: vi.fn().mockResolvedValue([]),
    createTemplate: vi.fn(),
    updateTemplate: vi.fn(),
    deleteTemplate: vi.fn(),
    ...templateOverrides,
  };
  const taskClient: MockedTaskClient = {
    listTasks: vi.fn().mockResolvedValue([]),
    createTask: vi.fn(),
    updateTask: vi.fn(),
    deleteTask: vi.fn(),
    ...taskOverrides,
  };
  return {
    templateStore: new TemplateState(templateClient),
    taskStore: new TaskState(taskClient),
    templateClient,
    taskClient,
  };
}

type Stores = ReturnType<typeof makeStores>;

describe('RecurringListView', () => {
  let RecurringListView: typeof import('./RecurringListView.svelte').default;

  beforeEach(async () => {
    // Prevent the real scheduleState.load() from firing — tests seed items directly.
    scheduleState.loaded = true;
    RecurringListView = (await import('./RecurringListView.svelte')).default;
  });

  async function renderRecurring(stores: Stores = makeStores()) {
    const result = render(RecurringListView, {
      templateStore: stores.templateStore,
      taskStore: stores.taskStore,
    });
    await Promise.resolve();
    await tick();
    return { ...result, stores };
  }

  async function confirmDeleteDialog(
    getByText: (text: string) => HTMLElement,
    getByRole: (role: string) => HTMLElement,
  ) {
    await fireEvent.click(getByText('Delete'));
    await tick();
    const dialog = getByRole('alertdialog');
    await fireEvent.click(within(dialog).getByText('Delete'));
    await Promise.resolve();
    await tick();
  }

  it('shows loading state while templates are being fetched', () => {
    const stores = makeStores({
      listTemplates: vi.fn().mockImplementation(() => new Promise(() => {})),
    });

    const { getByText } = render(RecurringListView, {
      templateStore: stores.templateStore,
      taskStore: stores.taskStore,
    });

    expect(getByText(/loading recurring templates/i)).toBeTruthy();
  });

  it('shows empty state when no templates exist', async () => {
    const { getByText } = await renderRecurring();

    expect(getByText(/no recurring templates yet/i)).toBeTruthy();
  });

  it('renders templates with active status and schedule details', async () => {
    scheduleState.items = [baseSchedule()];

    const { getByText } = await renderRecurring(
      makeStores({ listTemplates: vi.fn().mockResolvedValue([baseTemplate()]) }),
    );

    expect(getByText('Weekly review')).toBeTruthy();
    expect(getByText('Active')).toBeTruthy();
    expect(getByText('Work Hours')).toBeTruthy();
    expect(getByText('Weekly on Mon, Thu')).toBeTruthy();
  });

  it('updates a template from the modal editor and refreshes tasks afterwards', async () => {
    const template = baseTemplate();
    scheduleState.items = [baseSchedule()];

    const stores = makeStores({
      listTemplates: vi.fn().mockResolvedValue([template]),
      updateTemplate: vi.fn().mockResolvedValue({
        ...template,
        title: 'Updated review',
        updated_at: '2026-01-03T00:00:00Z',
      }),
    });

    const { getByText, getByRole, queryByText } = await renderRecurring(stores);

    await fireEvent.click(getByText('Edit'));
    await tick();

    await fireEvent.input(getByRole('textbox', { name: /title/i }), {
      target: { value: 'Updated review' },
    });
    await fireEvent.click(getByText('Save changes'));
    await Promise.resolve();
    await tick();

    expect(stores.templateClient.updateTemplate).toHaveBeenCalledWith(
      'template-1',
      expect.objectContaining({ title: 'Updated review', schedule_id: 'sched-1' }),
    );
    expect(stores.taskClient.listTasks).toHaveBeenCalledTimes(1);
    expect(queryByText('Edit recurring template')).toBeNull();
  });

  it('toggles template activity and updates the visible action label', async () => {
    const template = baseTemplate();

    const stores = makeStores({
      listTemplates: vi.fn().mockResolvedValue([template]),
      updateTemplate: vi.fn().mockResolvedValue({ ...template, is_active: false }),
    });

    const { getByText } = await renderRecurring(stores);

    await fireEvent.click(getByText('Deactivate'));
    await Promise.resolve();
    await tick();

    expect(stores.templateClient.updateTemplate).toHaveBeenCalledWith('template-1', {
      is_active: false,
    });
    expect(getByText('Activate')).toBeTruthy();
  });

  it('confirms deletion before removing a template', async () => {
    const stores = makeStores({
      listTemplates: vi.fn().mockResolvedValue([baseTemplate()]),
    });

    const { getByText, queryByText, getByRole } = await renderRecurring(stores);

    await confirmDeleteDialog(getByText, getByRole);

    expect(stores.templateClient.deleteTemplate).toHaveBeenCalledWith('template-1');
    expect(queryByText('Weekly review')).toBeNull();
  });

  type RR = Awaited<ReturnType<typeof renderRecurring>>;

  it.each([
    {
      label: 'empty title',
      template: baseTemplate(),
      schedules: [baseSchedule()],
      modify: async (r: RR) => {
        await fireEvent.input(r.getByRole('textbox', { name: /title/i }), {
          target: { value: '' },
        });
      },
      expectedMessage: 'Title is required',
    },
    {
      label: 'no schedule',
      template: baseTemplate({ schedule_id: '' }),
      schedules: [] as ReturnType<typeof baseSchedule>[],
      modify: async () => {},
      expectedMessage: 'Schedule is required',
    },
  ])(
    'validation: $label shows message and keeps modal open',
    async ({ template, schedules, modify, expectedMessage }) => {
      scheduleState.items = schedules;

      const stores = makeStores({
        listTemplates: vi.fn().mockResolvedValue([template]),
      });

      const result = await renderRecurring(stores);

      await fireEvent.click(result.getByText('Edit'));
      await tick();

      await modify(result);

      await fireEvent.click(result.getByText('Save changes'));
      await tick();

      expect(result.getByText(expectedMessage)).toBeTruthy();
      expect(result.getByText('Edit recurring template')).toBeTruthy();
    },
  );

  const cadenceCases: Array<{ label: string; cadence: Cadence; expectedSummary: string }> = [
    {
      label: 'weekly cadence',
      cadence: {
        period: 'Weekly',
        interval: 1,
        windows: [
          { start: 0, end: 0 },
          { start: 3, end: 3 },
        ],
      },
      expectedSummary: 'Weekly on Mon, Thu',
    },
    {
      label: 'weekly span cadence',
      cadence: { period: 'Weekly', interval: 1, windows: [{ start: 5, end: 6 }] },
      expectedSummary: 'Weekly on Sat–Sun',
    },
    {
      label: 'weekly interval cadence',
      cadence: { period: 'Weekly', interval: 2, windows: [{ start: 0, end: 0 }] },
      expectedSummary: 'Every 2 weeks on Mon',
    },
    {
      label: 'monthly cadence',
      cadence: { period: 'Monthly', interval: 1, windows: [{ start: 14, end: 14 }] },
      expectedSummary: 'Monthly on day 15',
    },
  ];

  it.each(cadenceCases)(
    'cadence display: $label shows "$expectedSummary"',
    async ({ cadence, expectedSummary }) => {
      const { getByText } = await renderRecurring(
        makeStores({ listTemplates: vi.fn().mockResolvedValue([baseTemplate({ cadence })]) }),
      );

      expect(getByText(expectedSummary)).toBeTruthy();
    },
  );

  it('sorts active templates before inactive and alphabetically within each group', async () => {
    const templates = [
      baseTemplate({ id: 't1', title: 'Zebra', is_active: true }),
      baseTemplate({ id: 't2', title: 'Alpha', is_active: false }),
      baseTemplate({ id: 't3', title: 'Middle', is_active: true }),
      baseTemplate({ id: 't4', title: 'Beta', is_active: false }),
    ];

    const { container } = await renderRecurring(
      makeStores({ listTemplates: vi.fn().mockResolvedValue(templates) }),
    );

    const cards = container.querySelectorAll('.template-card');
    const titles = Array.from(cards).map((card) => {
      const h3 = card.querySelector('.template-title');
      return h3?.textContent?.trim() ?? '';
    });

    expect(titles).toEqual(['Middle', 'Zebra', 'Alpha', 'Beta']);
  });

  it('closes the editor when the template being edited is deleted', async () => {
    scheduleState.items = [baseSchedule()];

    const stores = makeStores({
      listTemplates: vi.fn().mockResolvedValue([baseTemplate()]),
      deleteTemplate: vi.fn().mockResolvedValue(undefined),
    });

    const { getByText, queryByText, getByRole } = await renderRecurring(stores);

    await fireEvent.click(getByText('Edit'));
    await tick();
    expect(getByText('Edit recurring template')).toBeTruthy();

    await confirmDeleteDialog(getByText, getByRole);

    expect(queryByText('Edit recurring template')).toBeNull();
  });

  const baseSeed = (overrides: Partial<SharedFieldValues> = {}): SharedFieldValues => ({
    title: 'Seeded title',
    description: 'from task',
    durationMinutes: 30,
    priority: 'Medium',
    scheduleId: 'sched-1',
    labels: ['focus'],
    ...overrides,
  });

  it('opens the editor in create mode from a seeded request and creates a template', async () => {
    scheduleState.items = [baseSchedule()];

    const stores = makeStores({
      createTemplate: vi.fn().mockResolvedValue(baseTemplate({ id: 'new-1' })),
    });

    const { getByText, getByRole, queryByText } = render(RecurringListView, {
      createRequest: { seed: baseSeed(), nonce: 1 },
      templateStore: stores.templateStore,
      taskStore: stores.taskStore,
    });

    await Promise.resolve();
    await tick();

    expect(getByText('New recurring template')).toBeTruthy();
    expect((getByRole('textbox', { name: /title/i }) as HTMLInputElement).value).toBe(
      'Seeded title',
    );

    await fireEvent.click(getByText('Create template'));
    await Promise.resolve();
    await tick();

    expect(stores.templateClient.createTemplate).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Seeded title',
        description: 'from task',
        duration_minutes: 30,
        schedule_id: 'sched-1',
        labels: ['focus'],
        cadence: { period: 'Weekly', interval: 1, windows: [{ start: 0, end: 0 }] },
      }),
    );
    expect(stores.taskClient.listTasks).toHaveBeenCalledTimes(1);
    expect(queryByText('New recurring template')).toBeNull();
  });

  it.each([
    {
      label: 'updateTemplate',
      modalText: 'Edit recurring template',
      prepare: async () => {
        scheduleState.items = [baseSchedule()];
        const stores = makeStores({
          listTemplates: vi.fn().mockResolvedValue([baseTemplate()]),
          updateTemplate: vi.fn().mockRejectedValue(new Error('server error')),
        });
        const r = await renderRecurring(stores);
        await fireEvent.click(r.getByText('Edit'));
        await tick();
        await fireEvent.input(r.getByRole('textbox', { name: /title/i }), {
          target: { value: 'Failing update' },
        });
        await fireEvent.click(r.getByText('Save changes'));
        await Promise.resolve();
        await tick();
        return r;
      },
    },
    {
      label: 'createTemplate',
      modalText: 'New recurring template',
      prepare: async () => {
        scheduleState.items = [baseSchedule()];
        const stores = makeStores({
          createTemplate: vi.fn().mockRejectedValue(new Error('server error')),
        });
        const r = render(RecurringListView, {
          createRequest: { seed: baseSeed({ labels: [] }), nonce: 1 },
          templateStore: stores.templateStore,
          taskStore: stores.taskStore,
        });
        await Promise.resolve();
        await tick();
        await fireEvent.click(r.getByText('Create template'));
        await Promise.resolve();
        await tick();
        return r;
      },
    },
  ])('$label rejection keeps modal open and shows error toast', async ({ prepare, modalText }) => {
    const toastMod = await import('../../stores/toast.svelte');
    toastMod.toastState.items = [];

    const { getByText } = await prepare();

    expect(getByText(modalText)).toBeTruthy();
    expect(toastMod.toastState.items).toHaveLength(1);
    expect(toastMod.toastState.items[0].level).toBe('error');
  });
});
