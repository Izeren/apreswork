// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import type { Mocked } from 'vitest';
import { fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { CreateTaskInput, UpdateTaskInput } from '../../types';
import {
  ISO_DEADLINE,
  ISO_START,
  demoSchedules,
  formTask,
  getDateTimeTrigger,
  installTaskFormHooks,
  renderTaskForm,
  setInputValue,
  setSelectValue,
  taskFormFakeApi,
} from './taskFormTestSupport';
import type { TaskFormApi } from './taskFormTestSupport';
import { priorityCases, TEST_NOW } from './testFixtures';

let fake: Mocked<TaskFormApi>;

beforeEach(() => {
  fake = taskFormFakeApi();
});

installTaskFormHooks();

async function importScheduleState() {
  const mod = await import('../../stores/schedules.svelte');
  return mod.scheduleState;
}

describe('TaskForm — create mode rendering', () => {
  it.each<{
    field: string;
    check: (r: Awaited<ReturnType<typeof renderTaskForm>>) => void;
  }>([
    {
      field: 'modal title',
      check: (r) => {
        expect(r.getByText('Create Task')).toBeTruthy();
      },
    },
    {
      field: 'submit button',
      check: (r) => {
        expect(r.getByRole('button', { name: 'Create' })).toBeTruthy();
      },
    },
    {
      field: 'title empty',
      check: (r) => {
        expect((r.getByPlaceholderText('Task title') as HTMLInputElement).value).toBe('');
      },
    },
    {
      field: 'priority Medium',
      check: (r) => {
        expect((r.getByLabelText('Priority') as HTMLSelectElement).value).toBe('Medium');
      },
    },
    {
      field: 'no-split unchecked',
      check: (r) => {
        expect((r.getByRole('checkbox') as HTMLInputElement).checked).toBe(false);
      },
    },
  ])('create mode default: $field', async ({ check }) => {
    check(await renderTaskForm(fake, { initialStartDate: ISO_START }));
  });

  it('updates the min chunk max when duration changes', async () => {
    const { getByLabelText } = await renderTaskForm(fake, { initialStartDate: ISO_START });

    const minChunkInput = getByLabelText(/min chunk/i) as HTMLInputElement;
    expect(minChunkInput.getAttribute('max')).toBe('60');

    const durationInput = getByLabelText('Duration *') as HTMLInputElement;
    durationInput.value = '2h';
    await fireEvent.input(durationInput);
    await fireEvent.blur(durationInput);
    await tick();

    expect(minChunkInput.getAttribute('max')).toBe('120');
  });
});

describe('TaskForm — edit mode rendering', () => {
  it.each<{
    field: string;
    taskOverride?: Parameters<typeof formTask>[0];
    check: (r: Awaited<ReturnType<typeof renderTaskForm>>) => void;
  }>([
    {
      field: 'modal title',
      check: (r) => {
        expect(r.getByText('Edit Task')).toBeTruthy();
      },
    },
    {
      field: 'title',
      taskOverride: { title: 'Pre-filled Title' },
      check: (r) => {
        expect((r.getByPlaceholderText('Task title') as HTMLInputElement).value).toBe(
          'Pre-filled Title',
        );
      },
    },
    {
      field: 'priority',
      taskOverride: { priority: 'Critical' },
      check: (r) => {
        expect((r.getByLabelText('Priority') as HTMLSelectElement).value).toBe('Critical');
      },
    },
    {
      field: 'no-split checkbox',
      taskOverride: { no_split: true },
      check: (r) => {
        expect((r.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
      },
    },
    {
      field: 'description',
      taskOverride: { description: 'A description' },
      check: (r) => {
        expect((r.getByPlaceholderText('Optional description') as HTMLTextAreaElement).value).toBe(
          'A description',
        );
      },
    },
  ])('edit mode pre-fill: $field', async ({ taskOverride, check }) => {
    check(
      await renderTaskForm(fake, { task: formTask(taskOverride), initialStartDate: ISO_START }),
    );
  });

  it('pre-fills labels from task prop and renders chips', async () => {
    const { getByText } = await renderTaskForm(fake, {
      task: formTask({ labels: ['backend', 'urgent'] }),
      initialStartDate: ISO_START,
    });
    expect(getByText('backend')).toBeTruthy();
    expect(getByText('urgent')).toBeTruthy();
  });

  it('renders actions in the modal footer and binds submit to the form', async () => {
    const { container, getByRole, getByPlaceholderText } = await renderTaskForm(fake, {
      task: formTask(),
      initialStartDate: ISO_START,
    });

    // Make the form dirty so the footer appears (it is hidden when pristine in edit mode).
    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    titleInput.value = 'Changed';
    await fireEvent.input(titleInput);
    await tick();

    const form = container.querySelector('form.task-form') as HTMLFormElement | null;
    const modalBody = container.querySelector('.modal-body') as HTMLElement | null;
    const modalFooter = container.querySelector('.modal-footer') as HTMLElement | null;
    const saveButton = getByRole('button', { name: 'Save' }) as HTMLButtonElement;

    expect(form).toBeTruthy();
    expect(modalBody).toBeTruthy();
    expect(modalFooter).toBeTruthy();
    expect(form?.querySelector('.form-footer')).toBeNull();
    expect(modalBody?.contains(saveButton)).toBe(false);
    expect(modalFooter?.contains(saveButton)).toBe(true);
    expect(saveButton.getAttribute('form')).toBe(form?.id);
  });
});

describe('TaskForm — recurring instance start date', () => {
  it.each([
    {
      label: 'recurring instance',
      templateId: 'tmpl-1' as string | null,
      expectedDisabled: true,
      hintVisible: true,
    },
    {
      label: 'non-recurring task',
      templateId: null as string | null,
      expectedDisabled: false,
      hintVisible: false,
    },
  ])(
    '$label: start date disabled=$expectedDisabled',
    async ({ templateId, expectedDisabled, hintVisible }) => {
      const { container, queryByText } = await renderTaskForm(fake, {
        task: formTask({ recurring_template_id: templateId }),
        initialStartDate: ISO_START,
      });
      expect(getDateTimeTrigger(container, 'Start date').disabled).toBe(expectedDisabled);
      expect(queryByText('Set by the recurring schedule.') !== null).toBe(hintVisible);
    },
  );
});

describe('TaskForm — title validation', () => {
  it.each<{
    label: string;
    check: (
      r: Awaited<ReturnType<typeof renderTaskForm>>,
      onsubmit: ReturnType<typeof vi.fn>,
    ) => void;
  }>([
    {
      label: 'shows error message',
      check: (r) => {
        expect(r.getByText(/title is required/i)).toBeTruthy();
      },
    },
    {
      label: 'does not call onsubmit',
      check: (_, s) => {
        expect(s).not.toHaveBeenCalled();
      },
    },
    {
      label: 'title input has aria-invalid=true',
      check: (r) => {
        expect(
          (r.getByPlaceholderText('Task title') as HTMLInputElement).getAttribute('aria-invalid'),
        ).toBe('true');
      },
    },
  ])('title validation: $label', async ({ check }) => {
    const onsubmit = vi.fn();
    const result = await renderTaskForm(fake, { onsubmit, initialStartDate: ISO_START });
    await fireEvent.submit(result.container.querySelector('form')!);
    await tick();
    check(result, onsubmit);
  });
});

describe('TaskForm — deadline defaults', () => {
  it('prefills new task deadline to the end of the selected day', async () => {
    const { container } = await renderTaskForm(fake, {
      initialStartDate: '2026-03-29T09:30:00.000Z',
    });
    const trigger = getDateTimeTrigger(container, 'Deadline');
    expect(trigger.textContent).toContain('29/03/2026');
    expect(trigger.textContent).toContain('23:59');
  });

  it('prefills create-mode deadline to end-of-today via getNow when no initialStartDate', async () => {
    const { container } = await renderTaskForm(fake, {
      getNow: () => TEST_NOW,
    });
    const trigger = getDateTimeTrigger(container, 'Deadline');
    expect(trigger.textContent).toContain('Today');
    expect(trigger.textContent).toContain('23:59');
  });

  it('does not show deadline error in edit mode (deadline already set)', async () => {
    const { queryByText, container } = await renderTaskForm(fake, {
      task: formTask({ deadline: ISO_DEADLINE }),
      initialStartDate: ISO_START,
    });
    await fireEvent.submit(container.querySelector('form')!);
    await tick();
    expect(queryByText(/deadline is required/i)).toBeNull();
  });
});

type CreateFormResult = Awaited<ReturnType<typeof renderTaskForm>>;

async function runSubmitItEach(
  result: CreateFormResult,
  onsubmit: ReturnType<typeof vi.fn>,
  setup: (r: CreateFormResult) => Promise<void>,
): Promise<unknown> {
  await setup(result);
  await fireEvent.submit(result.container.querySelector('form')!);
  await tick();
  expect(onsubmit).toHaveBeenCalledTimes(1);
  return onsubmit.mock.calls[0][0];
}

describe('TaskForm — submit create mode', () => {
  it.each<{
    label: string;
    setup: (r: CreateFormResult) => Promise<void>;
    check: (input: CreateTaskInput) => void;
  }>([
    {
      label: 'title + priority',
      setup: async ({ getByPlaceholderText, getByLabelText }) => {
        await setInputValue(getByPlaceholderText('Task title') as HTMLInputElement, 'New Task');
        await setSelectValue(getByLabelText('Priority') as HTMLSelectElement, 'High');
      },
      check: (input) => {
        expect(input.title).toBe('New Task');
        expect(input.priority).toBe('High');
        expect(typeof input.deadline).toBe('string');
      },
    },
    {
      label: 'with labels',
      setup: async ({ getByPlaceholderText }) => {
        await setInputValue(
          getByPlaceholderText('Task title') as HTMLInputElement,
          'Task with labels',
        );
        const labelInputEl = getByPlaceholderText('Add label and press Enter') as HTMLInputElement;
        await setInputValue(labelInputEl, 'mytag');
        await fireEvent.keyDown(labelInputEl, { key: 'Enter' });
        await tick();
      },
      check: (input) => {
        expect(input.labels).toContain('mytag');
      },
    },
    {
      label: 'no_split=false by default',
      setup: async ({ getByPlaceholderText }) => {
        await setInputValue(getByPlaceholderText('Task title') as HTMLInputElement, 'Test Task');
      },
      check: (input) => {
        expect(input.no_split).toBe(false);
      },
    },
  ])('creates task: $label', async ({ setup, check }) => {
    const onsubmit = vi.fn();
    const result = await renderTaskForm(fake, { onsubmit, initialStartDate: ISO_START });
    check((await runSubmitItEach(result, onsubmit, setup)) as CreateTaskInput);
  });
});

describe('TaskForm — submit edit mode', () => {
  it.each<{
    label: string;
    taskOverride: Parameters<typeof formTask>[0];
    setup: (r: Awaited<ReturnType<typeof renderTaskForm>>) => Promise<void>;
    check: (input: UpdateTaskInput) => void;
  }>([
    {
      label: 'title',
      taskOverride: { title: 'Old Title' },
      setup: async (r) => {
        await setInputValue(
          r.getByPlaceholderText('Task title') as HTMLInputElement,
          'Updated Title',
        );
      },
      check: (input) => {
        expect(input.title).toBe('Updated Title');
      },
    },
    {
      label: 'priority',
      taskOverride: { priority: 'Low' },
      setup: async (r) => {
        await setSelectValue(r.getByLabelText('Priority') as HTMLSelectElement, 'Critical');
      },
      check: (input) => {
        expect(input.priority).toBe('Critical');
      },
    },
    {
      label: 'no_split from task',
      taskOverride: { no_split: true },
      setup: async (r) => {
        await setInputValue(
          r.getByPlaceholderText('Task title') as HTMLInputElement,
          'Changed Title',
        );
      },
      check: (input) => {
        expect(input.no_split).toBe(true);
      },
    },
  ])(
    'edit mode submit: includes $label in UpdateTaskInput',
    async ({ taskOverride, setup, check }) => {
      const onsubmit = vi.fn();
      const result = await renderTaskForm(fake, {
        task: formTask(taskOverride),
        onsubmit,
        initialStartDate: ISO_START,
      });
      check((await runSubmitItEach(result, onsubmit, setup)) as UpdateTaskInput);
    },
  );
});

describe('TaskForm — cancel button', () => {
  it('calls onclose when Cancel is clicked in create mode', async () => {
    const onclose = vi.fn();
    const { getByRole } = await renderTaskForm(fake, { onclose, initialStartDate: ISO_START });
    await fireEvent.click(getByRole('button', { name: 'Cancel' }));
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});

describe('TaskForm — labels', () => {
  it.each([
    {
      label: 'Enter key adds chip for valid input',
      input: 'frontend',
      key: 'Enter',
      present: true,
    },
    { label: 'whitespace-only input is rejected', input: '   ', key: 'Enter', present: false },
    { label: 'non-Enter key does not add chip', input: 'notadded', key: 'Tab', present: false },
  ])('label chip: $label', async ({ input, key, present }) => {
    const { getByPlaceholderText, queryByText } = await renderTaskForm(fake, {
      initialStartDate: ISO_START,
    });
    const labelInputEl = getByPlaceholderText('Add label and press Enter') as HTMLInputElement;
    await setInputValue(labelInputEl, input);
    await fireEvent.keyDown(labelInputEl, { key });
    await tick();
    expect(Boolean(queryByText(input))).toBe(present);
  });

  it('clears the label input after adding a chip', async () => {
    const { getByPlaceholderText } = await renderTaskForm(fake, { initialStartDate: ISO_START });

    const labelInputEl = getByPlaceholderText('Add label and press Enter') as HTMLInputElement;
    await setInputValue(labelInputEl, 'mytag');
    await fireEvent.keyDown(labelInputEl, { key: 'Enter' });
    await tick();

    expect(labelInputEl.value).toBe('');
  });

  it('does not add duplicate labels', async () => {
    const { getByPlaceholderText, getAllByText } = await renderTaskForm(fake, {
      initialStartDate: ISO_START,
    });

    const labelInputEl = getByPlaceholderText('Add label and press Enter') as HTMLInputElement;

    await setInputValue(labelInputEl, 'dup');
    await fireEvent.keyDown(labelInputEl, { key: 'Enter' });
    await tick();

    await setInputValue(labelInputEl, 'dup');
    await fireEvent.keyDown(labelInputEl, { key: 'Enter' });
    await tick();

    expect(getAllByText('dup')).toHaveLength(1);
  });

  it('clicking chip remove button removes that label', async () => {
    const { getByPlaceholderText, queryByText, getByRole } = await renderTaskForm(fake, {
      initialStartDate: ISO_START,
    });

    const labelInputEl = getByPlaceholderText('Add label and press Enter') as HTMLInputElement;
    await setInputValue(labelInputEl, 'removeme');
    await fireEvent.keyDown(labelInputEl, { key: 'Enter' });
    await tick();

    const removeBtn = getByRole('button', { name: /remove removeme/i });
    await fireEvent.click(removeBtn);
    await tick();

    expect(queryByText('removeme')).toBeNull();
  });
});

describe('TaskForm — priority dropdown', () => {
  it.each(priorityCases)('priority $priority can be selected', async ({ priority }) => {
    const { getByLabelText } = await renderTaskForm(fake, { initialStartDate: ISO_START });
    const select = getByLabelText('Priority') as HTMLSelectElement;
    await setSelectValue(select, priority);
    await tick();
    expect(select.value).toBe(priority);
  });
});

describe('TaskForm — schedule dropdown', () => {
  beforeEach(async () => {
    const scheduleState = await importScheduleState();
    scheduleState.items = demoSchedules;
  });

  afterEach(async () => {
    const scheduleState = await importScheduleState();
    scheduleState.items = [];
  });

  it('renders schedule options from scheduleState', async () => {
    const { getByText } = await renderTaskForm(fake, { initialStartDate: ISO_START });
    expect(getByText('Work Week')).toBeTruthy();
    expect(getByText('Weekend')).toBeTruthy();
  });

  it('selecting a schedule updates the dropdown value', async () => {
    const { getByLabelText } = await renderTaskForm(fake, { initialStartDate: ISO_START });
    const scheduleSelect = getByLabelText('Schedule') as HTMLSelectElement;
    await setSelectValue(scheduleSelect, 'sched-2');
    await tick();
    expect(scheduleSelect.value).toBe('sched-2');
  });

  it('renders "— None —" option as first option', async () => {
    const { getByLabelText } = await renderTaskForm(fake, { initialStartDate: ISO_START });
    const scheduleSelect = getByLabelText('Schedule') as HTMLSelectElement;
    expect(scheduleSelect.options[0].text).toBe('— None —');
  });

  it('pre-fills schedule from task prop in edit mode', async () => {
    const { getByLabelText } = await renderTaskForm(fake, {
      task: formTask({ schedule_id: 'sched-2' }),
      initialStartDate: ISO_START,
    });
    const scheduleSelect = getByLabelText('Schedule') as HTMLSelectElement;
    expect(scheduleSelect.value).toBe('sched-2');
  });
});

describe('TaskForm — closed state', () => {
  it('renders nothing when open=false', async () => {
    const { queryByText, container } = await renderTaskForm(fake, {
      open: false,
      initialStartDate: ISO_START,
    });
    expect(queryByText('Create Task')).toBeNull();
    expect(queryByText('Edit Task')).toBeNull();
    expect(container.querySelector('form')).toBeNull();
  });
});

describe('TaskForm — make recurring', () => {
  it('shows "Make recurring" in create mode and hands the shared values to the parent', async () => {
    const onmakerecurring = vi.fn();
    const { getByText, getByPlaceholderText } = await renderTaskForm(fake, {
      onmakerecurring,
      initialStartDate: ISO_START,
    });

    await setInputValue(getByPlaceholderText('Task title') as HTMLInputElement, 'Gym session');
    await tick();

    await fireEvent.click(getByText('Make recurring'));

    expect(onmakerecurring).toHaveBeenCalledTimes(1);
    expect(onmakerecurring.mock.calls[0][0]).toEqual(
      expect.objectContaining({ title: 'Gym session' }),
    );
  });

  it('does not show "Make recurring" in edit mode', async () => {
    const { getByPlaceholderText, queryByText } = await renderTaskForm(fake, {
      task: formTask(),
      onmakerecurring: vi.fn(),
      initialStartDate: ISO_START,
    });
    // Make the form dirty so the footer renders; the !isEdit guard is what must
    // suppress "Make recurring" — not the footer being absent.
    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    titleInput.value = 'Changed';
    await fireEvent.input(titleInput);
    await tick();
    expect(queryByText('Make recurring')).toBeNull();
  });

  it('does not show "Make recurring" when no handler is provided', async () => {
    const { queryByText } = await renderTaskForm(fake, { initialStartDate: ISO_START });
    expect(queryByText('Make recurring')).toBeNull();
  });
});
