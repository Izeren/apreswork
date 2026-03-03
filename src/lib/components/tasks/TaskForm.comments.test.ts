// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Mocked } from 'vitest';
import { fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import {
  formTask,
  installTaskFormHooks,
  renderTaskForm,
  taskFormFakeApi,
} from './taskFormTestSupport';
import type { TaskFormApi } from './taskFormTestSupport';
import { baseComment } from './testFixtures';

let fake: Mocked<TaskFormApi>;

beforeEach(() => {
  fake = taskFormFakeApi();
});

/** Flush the load promise chain (then → catch → finally) + Svelte reactivity. */
async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await tick();
}

installTaskFormHooks();

describe('TaskForm — comments section', () => {
  it('renders the comments section in edit mode and loads the task comments', async () => {
    fake.listComments.mockResolvedValue([baseComment({ content: 'First thoughts' })]);
    const { getByText } = await renderTaskForm(fake, { task: formTask() });
    await settle();

    expect(fake.listComments).toHaveBeenCalledWith('task-1');
    expect(getByText('Comments')).toBeTruthy();
    expect(getByText('First thoughts')).toBeTruthy();
  });

  it('renders no comments section in create mode', async () => {
    const { queryByText } = await renderTaskForm(fake);
    await settle();

    expect(queryByText('Comments')).toBeNull();
    expect(fake.listComments).not.toHaveBeenCalled();
  });

  it('posts a comment to the edited task without saving the task', async () => {
    const onsubmit = vi.fn();
    fake.createComment.mockResolvedValue(
      baseComment({ id: 'comment-2', content: 'Hello from the form' }),
    );
    const { container } = await renderTaskForm(fake, { task: formTask(), onsubmit });
    await settle();

    const input = container.querySelector('.comment-input') as HTMLTextAreaElement;
    input.value = 'Hello from the form';
    await fireEvent.input(input);
    await fireEvent.submit(container.querySelector('.comment-form')!);
    await settle();

    expect(fake.createComment).toHaveBeenCalledWith({
      task_id: 'task-1',
      content: 'Hello from the form',
    });
    expect(onsubmit).not.toHaveBeenCalled();
  });
});
