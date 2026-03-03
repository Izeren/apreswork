// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Mocked } from 'vitest';
import { fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { UpdateTaskInput } from '../../types';
import {
  formTask,
  setInputValue,
  installTaskFormHooks,
  renderTaskForm,
  taskFormFakeApi,
  ISO_START,
} from './taskFormTestSupport';
import type { TaskFormApi } from './taskFormTestSupport';

let fake: Mocked<TaskFormApi>;

beforeEach(() => {
  fake = taskFormFakeApi();
});

async function backdropClick(container: HTMLElement) {
  const overlay = container.querySelector('.overlay')!;
  await fireEvent.mouseDown(overlay);
  await fireEvent.click(overlay);
}

async function escapeKey(container: HTMLElement) {
  const overlay = container.querySelector('.overlay')!;
  await fireEvent.keyDown(overlay, { key: 'Escape' });
}

async function renderEditForm() {
  const onsubmit = vi.fn();
  const onclose = vi.fn();
  const result = await renderTaskForm(fake, { task: formTask(), onsubmit, onclose });
  return { onsubmit, onclose, ...result };
}

installTaskFormHooks();

const softDismissTriggers = [
  { label: 'backdrop click', trigger: (c: HTMLElement) => backdropClick(c) },
  { label: 'Escape key', trigger: (c: HTMLElement) => escapeKey(c) },
];

const allDismissTriggers = [
  ...softDismissTriggers,
  { label: 'form submit', trigger: (c: HTMLElement) => fireEvent.submit(c.querySelector('form')!) },
];

describe('TaskForm — dirty-gated auto-save', () => {
  it.each(allDismissTriggers)('$label without edits closes without saving', async ({ trigger }) => {
    const { container, onsubmit, onclose } = await renderEditForm();

    await trigger(container);
    await tick();

    expect(onsubmit).not.toHaveBeenCalled();
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it.each(allDismissTriggers)('$label after an edit saves', async ({ trigger }) => {
    const { container, getByPlaceholderText, onsubmit, onclose } = await renderEditForm();

    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    await setInputValue(titleInput, 'Updated Title');
    await trigger(container);
    await tick();

    expect(onsubmit).toHaveBeenCalledTimes(1);
    const input = onsubmit.mock.calls[0][0] as UpdateTaskInput;
    expect(input.title).toBe('Updated Title');
    // The parent closes the form after a successful update, not the form itself.
    expect(onclose).not.toHaveBeenCalled();
  });

  it('reverted edit closes without saving', async () => {
    const { container, getByPlaceholderText, onsubmit, onclose } = await renderEditForm();

    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    await setInputValue(titleInput, 'X');
    await setInputValue(titleInput, 'My Task');
    await backdropClick(container);
    await tick();

    expect(onsubmit).not.toHaveBeenCalled();
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it.each(softDismissTriggers)(
    'create mode $label submits when form has content',
    async ({ trigger }) => {
      const onsubmit = vi.fn();
      const onclose = vi.fn();
      const { getByPlaceholderText, container } = await renderTaskForm(fake, {
        onsubmit,
        onclose,
        initialStartDate: ISO_START,
      });

      const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
      await setInputValue(titleInput, 'Brand new task');
      await trigger(container);
      await tick();

      expect(onsubmit).toHaveBeenCalledTimes(1);
    },
  );
});

describe('TaskForm — dirty-gated footer visibility', () => {
  it.each([
    { label: 'pristine', dirty: false, wantFooter: false },
    { label: 'dirty', dirty: true, wantFooter: true },
  ])('edit mode $label — footer visible=$wantFooter', async ({ dirty, wantFooter }) => {
    const { container, getByPlaceholderText, getByRole, queryByRole } = await renderEditForm();
    if (dirty) {
      const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
      await setInputValue(titleInput, 'New Title');
      await tick();
    }
    if (wantFooter) {
      expect(getByRole('button', { name: 'Save' })).toBeTruthy();
      expect(getByRole('button', { name: 'Cancel edits' })).toBeTruthy();
      expect(container.querySelector('.modal-footer')).toBeTruthy();
    } else {
      expect(queryByRole('button', { name: 'Save' })).toBeNull();
      expect(queryByRole('button', { name: 'Cancel edits' })).toBeNull();
    }
  });

  it('create mode always shows Create and Cancel buttons', async () => {
    const onsubmit = vi.fn();
    const onclose = vi.fn();
    const { getByRole, container } = await renderTaskForm(fake, { onsubmit, onclose });

    expect(getByRole('button', { name: 'Create' })).toBeTruthy();
    // In create mode the label is "Cancel", not "Cancel edits".
    expect(getByRole('button', { name: 'Cancel' })).toBeTruthy();
    expect(container.querySelector('.modal-footer')).toBeTruthy();
  });

  it('Cancel edits button discards changes and calls onclose', async () => {
    const { getByPlaceholderText, getByRole, onsubmit, onclose } = await renderEditForm();

    const titleInput = getByPlaceholderText('Task title') as HTMLInputElement;
    await setInputValue(titleInput, 'Changed Title');
    await tick();

    await fireEvent.click(getByRole('button', { name: 'Cancel edits' }));
    await tick();

    expect(onsubmit).not.toHaveBeenCalled();
    expect(onclose).toHaveBeenCalledTimes(1);
  });
});
