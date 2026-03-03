// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { demoSchedules } from './taskFormTestSupport';
import { priorityCases } from './testFixtures';

afterEach(async () => {
  cleanup();
  vi.clearAllMocks();

  const scheduleMod = await import('../../stores/schedules.svelte');
  scheduleMod.scheduleState.items = [];
  scheduleMod.scheduleState.loading = false;
  scheduleMod.scheduleState.loaded = false;
});

import type { SharedFieldValues, SharedFormController } from './SharedFormFields.svelte';

function defaultInitial(overrides: Partial<SharedFieldValues> = {}): SharedFieldValues {
  return {
    title: '',
    description: '',
    durationMinutes: 60,
    priority: 'Medium',
    scheduleId: '',
    labels: [],
    ...overrides,
  };
}

async function importComponent() {
  const mod = await import('./SharedFormFields.svelte');
  return mod.default;
}

async function importScheduleState() {
  const mod = await import('../../stores/schedules.svelte');
  return mod.scheduleState;
}

type RenderOpenResult = {
  container: ReturnType<typeof render>['container'];
  getByLabelText: ReturnType<typeof render>['getByLabelText'];
  getByPlaceholderText: ReturnType<typeof render>['getByPlaceholderText'];
  getByText: ReturnType<typeof render>['getByText'];
  getByRole: ReturnType<typeof render>['getByRole'];
  getAllByText: ReturnType<typeof render>['getAllByText'];
  queryByText: ReturnType<typeof render>['queryByText'];
  queryAllByText: ReturnType<typeof render>['queryAllByText'];
  controller: SharedFormController;
};

/** Helper: render SharedFormFields with open=true and capture the controller. */
async function renderOpen(
  initial: SharedFieldValues = defaultInitial(),
  props: Record<string, unknown> = {},
): Promise<RenderOpenResult> {
  const SharedFormFields = await importComponent();

  let capturedController: SharedFormController | null = null;

  const result = render(SharedFormFields, {
    open: true,
    initial,
    idPrefix: 'test',
    onready: (ctrl: SharedFormController) => {
      capturedController = ctrl;
    },
    ...props,
  });

  await tick();

  return {
    ...result,
    controller: capturedController!,
  };
}

async function addLabel(
  utils: Pick<RenderOpenResult, 'getByPlaceholderText'>,
  label: string,
  key = 'Enter',
): Promise<void> {
  const input = utils.getByPlaceholderText('Add label and press Enter') as HTMLInputElement;
  input.value = label;
  await fireEvent.input(input);
  await fireEvent.keyDown(input, { key });
  await tick();
}

describe('SharedFormFields — rendering', () => {
  it.each([
    { name: 'title', label: /title/i, tag: 'INPUT', options: [] as string[] },
    { name: 'description', label: /description/i, tag: 'TEXTAREA', options: [] as string[] },
    {
      name: 'priority',
      label: /priority/i,
      tag: 'SELECT',
      options: ['Low', 'Medium', 'High', 'Critical'],
    },
    { name: 'schedule', label: /schedule/i, tag: 'SELECT', options: [] as string[] },
  ])('renders $name field correctly', async ({ label, tag, options }) => {
    const { getByLabelText, getByText } = await renderOpen();
    expect((getByLabelText(label) as HTMLElement).tagName).toBe(tag);
    for (const opt of options) {
      getByText(opt);
    }
  });

  it('renders labels input with placeholder', async () => {
    const { getByPlaceholderText } = await renderOpen();
    expect((getByPlaceholderText('Add label and press Enter') as HTMLElement).tagName).toBe(
      'INPUT',
    );
  });

  it.each([
    {
      field: 'title',
      initial: defaultInitial({ title: 'My title' }),
      getEl: (r: RenderOpenResult) => r.getByPlaceholderText('Task title'),
      expected: 'My title',
    },
    {
      field: 'description',
      initial: defaultInitial({ description: 'My desc' }),
      getEl: (r: RenderOpenResult) => r.getByPlaceholderText('Optional description'),
      expected: 'My desc',
    },
    {
      field: 'priority',
      initial: defaultInitial({ priority: 'Critical' }),
      getEl: (r: RenderOpenResult) => r.getByLabelText(/priority/i),
      expected: 'Critical',
    },
  ])('pre-fills $field from initial prop', async ({ initial, getEl, expected }) => {
    const result = await renderOpen(initial);
    expect((getEl(result) as HTMLInputElement).value).toBe(expected);
  });

  it('pre-fills labels from initial prop and renders chips', async () => {
    const { queryByText } = await renderOpen(defaultInitial({ labels: ['alpha', 'beta'] }));
    expect(queryByText('alpha')).not.toBeNull();
    expect(queryByText('beta')).not.toBeNull();
  });

  it('does not render "— None —" option when scheduleNullable=false (default)', async () => {
    const { queryByText } = await renderOpen();
    expect(queryByText('— None —')).toBeNull();
  });

  it('renders "— None —" option when scheduleNullable=true', async () => {
    const { queryByText } = await renderOpen(defaultInitial(), { scheduleNullable: true });
    expect(queryByText('— None —')).not.toBeNull();
  });

  it('renders schedule options from scheduleState', async () => {
    const scheduleState = await importScheduleState();
    scheduleState.items = [demoSchedules[0]];
    const { queryByText } = await renderOpen();
    expect(queryByText('Work Week')).not.toBeNull();
  });
});

describe('SharedFormFields — getValues()', () => {
  it('returns current field values via getValues()', async () => {
    const initial = defaultInitial({
      title: 'My Task',
      description: 'desc',
      durationMinutes: 90,
      priority: 'High',
      scheduleId: 'sched-1',
      labels: ['a', 'b'],
    });

    const { controller } = await renderOpen(initial);
    const values = controller.getValues();

    expect(values.title).toBe('My Task');
    expect(values.description).toBe('desc');
    expect(values.durationMinutes).toBe(90);
    expect(values.priority).toBe('High');
    expect(values.scheduleId).toBe('sched-1');
    expect(values.labels).toEqual(['a', 'b']);
  });

  it('returns a copy of the labels array (mutation safety)', async () => {
    const initial = defaultInitial({ labels: ['x'] });
    const { controller } = await renderOpen(initial);

    const values = controller.getValues();
    values.labels.push('y');

    expect(controller.getValues().labels).toEqual(['x']);
  });

  it('returns empty labels when none are set', async () => {
    const { controller } = await renderOpen(defaultInitial({ labels: [] }));
    expect(controller.getValues().labels).toEqual([]);
  });
});

describe('SharedFormFields — validate()', () => {
  it.each([
    { label: 'empty', title: '' },
    { label: 'whitespace-only', title: '   ' },
  ])('returns false and shows error when title is $label', async ({ title }) => {
    const { controller, getByText } = await renderOpen(defaultInitial({ title }));
    const result = controller.validate();
    await tick();
    expect(result).toBe(false);
    getByText('Title is required');
  });

  it('returns false when durationMinutes is 0', async () => {
    const { controller, getByText } = await renderOpen(
      defaultInitial({ title: 'ok', durationMinutes: 0 }),
    );
    const result = controller.validate();
    await tick();
    expect(result).toBe(false);
    getByText('Duration must be greater than 0');
  });

  it('returns false when scheduleRequired=true and scheduleId is empty', async () => {
    const { controller, getByText } = await renderOpen(
      defaultInitial({ title: 'ok', scheduleId: '' }),
      { scheduleRequired: true },
    );
    const result = controller.validate();
    await tick();
    expect(result).toBe(false);
    getByText('Schedule is required');
  });

  it.each([
    { scheduleRequired: false, scheduleId: '' },
    { scheduleRequired: true, scheduleId: 'sched-1' },
  ])(
    'returns true when all required fields are valid (scheduleRequired=$scheduleRequired)',
    async ({ scheduleRequired, scheduleId }) => {
      const { controller } = await renderOpen(
        defaultInitial({ title: 'Valid', durationMinutes: 60, scheduleId }),
        { scheduleRequired },
      );
      const result = controller.validate();
      expect(result).toBe(true);
    },
  );

  it('does not show schedule error when scheduleRequired=false and scheduleId is empty', async () => {
    const { controller, queryByText } = await renderOpen(
      defaultInitial({ title: 'ok', scheduleId: '' }),
      { scheduleRequired: false },
    );
    controller.validate();
    await tick();
    expect(queryByText('Schedule is required')).toBeNull();
  });

  it('title input has aria-invalid when title error is shown', async () => {
    const { controller, getByPlaceholderText } = await renderOpen(defaultInitial({ title: '' }));
    controller.validate();
    await tick();
    const input = getByPlaceholderText('Task title') as HTMLInputElement;
    expect(input.getAttribute('aria-invalid')).toBe('true');
  });
});

describe('SharedFormFields — resetErrors()', () => {
  it('clears validation errors after resetErrors()', async () => {
    const { controller, queryByText } = await renderOpen(defaultInitial({ title: '' }));
    controller.validate();
    await tick();
    controller.resetErrors();
    await tick();
    expect(queryByText('Title is required')).toBeNull();
  });
});

describe('SharedFormFields — labels', () => {
  it.each([
    {
      desc: 'pressing Enter adds a chip',
      adds: [{ label: 'frontend', key: 'Enter' }],
      queryText: 'frontend',
      expectedCount: 1,
    },
    {
      desc: 'does not add duplicate labels',
      adds: [
        { label: 'dup', key: 'Enter' },
        { label: 'dup', key: 'Enter' },
      ],
      queryText: 'dup',
      expectedCount: 1,
    },
    {
      desc: 'does not add whitespace-only labels',
      adds: [{ label: '   ', key: 'Enter' }],
      queryText: '   ',
      expectedCount: 0,
    },
    {
      desc: 'non-Enter key does not add a label',
      adds: [{ label: 'notadded', key: 'Tab' }],
      queryText: 'notadded',
      expectedCount: 0,
    },
  ])('label input: $desc', async ({ adds, queryText, expectedCount }) => {
    const utils = await renderOpen();
    for (const { label, key } of adds) {
      await addLabel(utils, label, key);
    }
    expect(utils.queryAllByText(queryText)).toHaveLength(expectedCount);
  });

  it('clears label input after adding chip', async () => {
    const utils = await renderOpen();
    await addLabel(utils, 'tag');
    const input = utils.getByPlaceholderText('Add label and press Enter') as HTMLInputElement;
    expect(input.value).toBe('');
  });

  it('clicking chip remove button removes that label', async () => {
    const utils = await renderOpen();
    const { queryByText, getByRole } = utils;
    await addLabel(utils, 'removeme');

    const removeBtn = getByRole('button', { name: /remove removeme/i }) as HTMLButtonElement;
    await fireEvent.click(removeBtn);
    await tick();

    expect(queryByText('removeme')).toBeNull();
  });

  it('removed label is excluded from getValues()', async () => {
    const { getByRole, controller } = await renderOpen(
      defaultInitial({ labels: ['keep', 'drop'] }),
    );

    await tick();

    const removeBtn = getByRole('button', { name: /remove drop/i }) as HTMLButtonElement;
    await fireEvent.click(removeBtn);
    await tick();

    expect(controller.getValues().labels).toEqual(['keep']);
  });

  it('added label appears in getValues()', async () => {
    const utils = await renderOpen();
    await addLabel(utils, 'newlabel');
    expect(utils.controller.getValues().labels).toContain('newlabel');
  });
});

describe('SharedFormFields — priority dropdown', () => {
  it.each(priorityCases)('priority $priority renders correctly', async ({ priority }) => {
    const { controller } = await renderOpen(defaultInitial({ priority }));
    expect(controller.getValues().priority).toBe(priority);
  });
});

describe('SharedFormFields — schedule dropdown', () => {
  beforeEach(async () => {
    const scheduleState = await importScheduleState();
    scheduleState.items = demoSchedules;
  });

  it('renders schedule options from scheduleState', async () => {
    const { queryByText } = await renderOpen();
    expect(queryByText('Work Week')).not.toBeNull();
    expect(queryByText('Weekend')).not.toBeNull();
  });

  it('pre-fills schedule from initial prop', async () => {
    const { getByLabelText } = await renderOpen(defaultInitial({ scheduleId: 'sched-2' }));
    const select = getByLabelText(/schedule/i) as HTMLSelectElement;
    expect(select.value).toBe('sched-2');
  });

  it('selecting a schedule updates getValues()', async () => {
    const { getByLabelText, controller } = await renderOpen(
      defaultInitial({ scheduleId: 'sched-1' }),
    );
    const select = getByLabelText(/schedule/i) as HTMLSelectElement;
    select.value = 'sched-2';
    await fireEvent.change(select);
    await tick();
    expect(controller.getValues().scheduleId).toBe('sched-2');
  });

  it('shows a stub option for unknown scheduleId in initial values', async () => {
    const { getByLabelText } = await renderOpen(defaultInitial({ scheduleId: 'unknown-id' }));
    const select = getByLabelText(/schedule/i) as HTMLSelectElement;
    expect(select.value).toBe('unknown-id');
  });
});

describe('SharedFormFields — field reset when open becomes true', () => {
  it('resets title when open prop transitions from false to true', async () => {
    const SharedFormFields = await importComponent();

    const { getByPlaceholderText, rerender } = render(SharedFormFields, {
      open: false,
      initial: defaultInitial({ title: 'Initial' }),
      idPrefix: 'test',
    });

    await rerender({
      open: true,
      initial: defaultInitial({ title: 'Reloaded' }),
      idPrefix: 'test',
    });
    await tick();

    const input = getByPlaceholderText('Task title') as HTMLInputElement;
    expect(input.value).toBe('Reloaded');
  });

  it('clears errors when open becomes true', async () => {
    const SharedFormFields = await importComponent();
    let capturedController: SharedFormController | null = null;

    const { queryByText, rerender } = render(SharedFormFields, {
      open: true,
      initial: defaultInitial({ title: '' }),
      idPrefix: 'test',
      onready: (ctrl: SharedFormController) => {
        capturedController = ctrl;
      },
    });
    await tick();

    capturedController!.validate();
    await tick();

    await rerender({
      open: false,
      initial: defaultInitial({ title: 'fresh' }),
      idPrefix: 'test',
    });
    await tick();

    await rerender({
      open: true,
      initial: defaultInitial({ title: 'fresh' }),
      idPrefix: 'test',
    });
    await tick();

    expect(queryByText('Title is required')).toBeNull();
  });
});

describe('SharedFormFields — description preview toggle', () => {
  it('shows the Preview button next to the description label', async () => {
    const { getByRole } = await renderOpen(defaultInitial({ description: 'hello' }));
    expect((getByRole('button', { name: 'Preview' }) as HTMLElement).tagName).toBe('BUTTON');
  });

  it('toggles to Preview: textarea disappears and rendered markdown appears', async () => {
    const { getByRole, getByPlaceholderText, container } = await renderOpen(
      defaultInitial({ description: '**bold**' }),
    );

    getByPlaceholderText('Optional description');

    await fireEvent.click(getByRole('button', { name: 'Preview' }) as HTMLButtonElement);
    await tick();

    expect(container.querySelector('textarea')).toBeNull();
    expect(container.querySelector('strong')).toBeTruthy();
    getByRole('button', { name: 'Edit' });
  });

  it('toggles back to Edit: textarea returns with value preserved', async () => {
    const { getByRole, getByPlaceholderText } = await renderOpen(
      defaultInitial({ description: '**bold**' }),
    );

    await fireEvent.click(getByRole('button', { name: 'Preview' }) as HTMLButtonElement);
    await tick();

    await fireEvent.click(getByRole('button', { name: 'Edit' }) as HTMLButtonElement);
    await tick();

    const textarea = getByPlaceholderText('Optional description') as HTMLTextAreaElement;
    expect(textarea.value).toBe('**bold**');
  });

  it('shows "Nothing to preview" when description is empty and preview is toggled', async () => {
    const { getByRole, queryByText } = await renderOpen(defaultInitial({ description: '' }));

    await fireEvent.click(getByRole('button', { name: 'Preview' }) as HTMLButtonElement);
    await tick();

    expect(queryByText('Nothing to preview')).not.toBeNull();
  });

  it('resets preview to Edit mode when the form is reopened', async () => {
    const SharedFormFields = await importComponent();

    const { getByRole, getByPlaceholderText, rerender } = render(SharedFormFields, {
      open: true,
      initial: defaultInitial({ description: 'some text' }),
      idPrefix: 'test',
    });
    await tick();

    await fireEvent.click(getByRole('button', { name: 'Preview' }));
    await tick();

    await rerender({
      open: false,
      initial: defaultInitial({ description: 'new text' }),
      idPrefix: 'test',
    });
    await tick();

    await rerender({
      open: true,
      initial: defaultInitial({ description: 'new text' }),
      idPrefix: 'test',
    });
    await tick();

    expect((getByPlaceholderText('Optional description') as HTMLElement).tagName).toBe('TEXTAREA');
  });
});

describe('SharedFormFields — extraFields snippet', () => {
  it('onready callback is called with a controller', async () => {
    const SharedFormFields = await importComponent();
    const onready = vi.fn();

    render(SharedFormFields, {
      open: true,
      initial: defaultInitial(),
      idPrefix: 'test',
      onready,
    });
    await tick();

    expect(onready).toHaveBeenCalledOnce();
    const ctrl = onready.mock.calls[0][0] as SharedFormController;
    expect(typeof ctrl.validate).toBe('function');
    expect(typeof ctrl.getValues).toBe('function');
    expect(typeof ctrl.resetErrors).toBe('function');
  });
});
