// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach, type Mock } from 'vitest';
import { render, cleanup, fireEvent, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { toastState } from '../../stores/toast.svelte';
import type { Comment, Task } from '../../types';

const { default: CommentSection } = await import('./CommentSection.svelte');

// Per-test mock functions — injected via props; re-created each beforeEach.
let mockListComments!: Mock;
let mockCreateComment!: Mock;
let mockUpdateComment!: Mock;
let mockDeleteComment!: Mock;

function baseTask(overrides: Partial<Task> = {}): Task {
  return {
    id: 'task-1',
    title: 'Test Task',
    description: null,
    duration_minutes: 90,
    time_logged_minutes: 30,
    priority: 'High',
    status: 'pending',
    start_date: null,
    deadline: null,
    schedule_id: 'sched-1',
    min_chunk_minutes: 30,
    no_split: false,
    recurring_template_id: null,
    labels: [],
    created_at: '2026-07-01T00:00:00Z',
    updated_at: '2026-07-01T00:00:00Z',
    ...overrides,
  };
}

function userComment(overrides: Partial<Comment> = {}): Comment {
  return {
    id: 'comment-1',
    task_id: 'task-1',
    author: 'User',
    content: 'First thoughts',
    created_at: '2026-07-10T10:00:00Z',
    updated_at: '2026-07-10T10:00:00Z',
    ...overrides,
  };
}

function systemComment(overrides: Partial<Comment> = {}): Comment {
  return userComment({
    id: 'comment-sys',
    author: 'SYSTEM',
    content: 'Chunk completed: +45m logged (1h 15m / 2h total)',
    ...overrides,
  });
}

/** Flush pending microtasks and Svelte reactivity after an async load. */
async function settle() {
  await Promise.resolve();
  await tick();
}

function textarea(container: HTMLElement): HTMLTextAreaElement {
  return container.querySelector('.comment-input') as HTMLTextAreaElement;
}

function form(container: HTMLElement): HTMLFormElement {
  return container.querySelector('.comment-form') as HTMLFormElement;
}

/** Render CommentSection for `task`, flush the load chain; return the render result. */
async function renderComments(task: Task = baseTask()) {
  const result = render(CommentSection, {
    task,
    listComments: mockListComments,
    createComment: mockCreateComment,
    updateComment: mockUpdateComment,
    deleteComment: mockDeleteComment,
  });
  await settle();
  return result;
}

/** Open a user comment's actions menu and pick Edit or Delete (queries hit document.body). */
async function openCommentAction(action: 'Edit' | 'Delete') {
  await fireEvent.click(screen.getByLabelText('Comment actions'));
  await fireEvent.click(screen.getByText(action));
  await tick();
}

/** Confirm the destructive delete dialog and flush the delete chain. */
async function confirmDelete() {
  await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
  await settle();
}

beforeEach(() => {
  mockListComments = vi.fn().mockResolvedValue([]);
  mockCreateComment = vi.fn();
  mockUpdateComment = vi.fn();
  mockDeleteComment = vi.fn();
});

afterEach(() => {
  cleanup();
  toastState.items = [];
});

describe('CommentSection — rendering', () => {
  it('loads and renders the task comments', async () => {
    mockListComments.mockResolvedValue([userComment(), systemComment()]);
    const { getByText } = await renderComments();

    expect(mockListComments).toHaveBeenCalledWith('task-1');
    expect(getByText('User')).toBeTruthy();
    expect(getByText('First thoughts')).toBeTruthy();
    expect(getByText('Chunk completed: +45m logged (1h 15m / 2h total)')).toBeTruthy();
  });

  it('shows an empty state when there are no comments', async () => {
    const { getByText } = await renderComments();
    expect(getByText('No comments yet')).toBeTruthy();
  });

  it('renders user comment content as markdown', async () => {
    mockListComments.mockResolvedValue([userComment({ content: '**bold** note' })]);
    const { container } = await renderComments();

    const content = container.querySelector('.comment-content') as HTMLElement;
    expect(content.querySelector('strong')).toBeTruthy();
    expect(content.querySelector('strong')!.textContent).toBe('bold');
  });

  it.each([
    { name: 'unedited', updated_at: '2026-07-10T10:00:00Z', edited: false },
    { name: 'edited', updated_at: '2026-07-10T11:00:00Z', edited: true },
  ])('marks $name comments with the (edited) suffix: $edited', async ({ updated_at, edited }) => {
    mockListComments.mockResolvedValue([userComment({ updated_at })]);
    const { queryByText } = await renderComments();

    expect(queryByText(/\(edited\)/) !== null).toBe(edited);
  });

  it('renders system comments muted, without author header or actions menu (M12.6)', async () => {
    mockListComments.mockResolvedValue([systemComment()]);
    const { container, queryByText, queryByLabelText } = await renderComments();

    expect(container.querySelector('.comment--system')).toBeTruthy();
    expect(queryByText('SYSTEM')).toBeNull();
    expect(queryByLabelText('Comment actions')).toBeNull();
  });

  it('shows a load failure as an error toast', async () => {
    mockListComments.mockRejectedValue({ error: 'database', message: 'boom' });
    await renderComments();

    expect(toastState.items.some((t) => t.text === 'Failed to load comments')).toBe(true);
  });
});

describe('CommentSection — reload semantics', () => {
  it('refetches when the task updated_at bumps and keeps the draft', async () => {
    const task = baseTask();
    const { container, getByText, rerender } = await renderComments(task);
    expect(mockListComments).toHaveBeenCalledTimes(1);

    await fireEvent.input(textarea(container), { target: { value: 'WIP draft' } });

    // A chunk completion bumps the task and records a SYSTEM comment (M12.5).
    mockListComments.mockResolvedValue([systemComment()]);
    await rerender({ task: { ...task, updated_at: '2026-07-02T00:00:00Z' } });
    await settle();

    expect(mockListComments).toHaveBeenCalledTimes(2);
    expect(getByText('Chunk completed: +45m logged (1h 15m / 2h total)')).toBeTruthy();
    expect(textarea(container).value).toBe('WIP draft');
  });

  it('resets the form when the task id changes', async () => {
    const { container, rerender } = await renderComments();

    await fireEvent.input(textarea(container), { target: { value: 'WIP draft' } });
    await rerender({ task: baseTask({ id: 'task-2' }) });
    await settle();

    expect(mockListComments).toHaveBeenLastCalledWith('task-2');
    expect(textarea(container).value).toBe('');
  });
});

describe('CommentSection — create', () => {
  it('submits a new comment, clears the input, and refetches', async () => {
    mockCreateComment.mockResolvedValue(userComment({ content: 'Hello **world**' }));
    const { container } = await renderComments();

    await fireEvent.input(textarea(container), { target: { value: 'Hello **world**' } });
    await fireEvent.submit(form(container));
    await settle();

    expect(mockCreateComment).toHaveBeenCalledWith({
      task_id: 'task-1',
      content: 'Hello **world**',
    });
    expect(textarea(container).value).toBe('');
    expect(mockListComments).toHaveBeenCalledTimes(2);
  });

  it.each([
    { name: 'blank', input: '', disabled: true },
    { name: 'whitespace-only', input: '   ', disabled: true },
    { name: 'non-blank', input: 'x', disabled: false },
  ])('submit button is disabled=$disabled when input is $name', async ({ input, disabled }) => {
    const { container, getByRole } = await renderComments();
    const submit = getByRole('button', { name: 'Comment' }) as HTMLButtonElement;

    await fireEvent.input(textarea(container), { target: { value: input } });
    expect(submit.disabled).toBe(disabled);
  });

  it('surfaces a validation error from create as a toast', async () => {
    mockCreateComment.mockRejectedValue({
      error: 'validation',
      message: 'Comment content must not be empty',
    });
    const { container } = await renderComments();

    await fireEvent.input(textarea(container), { target: { value: 'x' } });
    await fireEvent.submit(form(container));
    await settle();

    expect(toastState.items.some((t) => t.text === 'Comment content must not be empty')).toBe(true);
  });
});

describe('CommentSection — edit', () => {
  it('prepopulates the shared input from the menu and saves (M12.7)', async () => {
    mockListComments.mockResolvedValue([userComment()]);
    mockUpdateComment.mockResolvedValue(userComment({ content: 'Updated text' }));
    const { container, getByRole } = await renderComments();

    await openCommentAction('Edit');

    expect(textarea(container).value).toBe('First thoughts');
    expect(getByRole('button', { name: 'Save' })).toBeTruthy();

    await fireEvent.input(textarea(container), { target: { value: 'Updated text' } });
    await fireEvent.submit(form(container));
    await settle();

    expect(mockUpdateComment).toHaveBeenCalledWith('comment-1', 'Updated text');
    expect(textarea(container).value).toBe('');
    expect(getByRole('button', { name: 'Comment' })).toBeTruthy();
  });

  it('cancel exits edit mode and clears the input', async () => {
    mockListComments.mockResolvedValue([userComment()]);
    const { container, getByRole, queryByRole } = await renderComments();

    await openCommentAction('Edit');
    await fireEvent.click(getByRole('button', { name: 'Cancel' }));

    expect(textarea(container).value).toBe('');
    expect(queryByRole('button', { name: 'Save' })).toBeNull();
    expect(mockUpdateComment).not.toHaveBeenCalled();
  });
});

describe('CommentSection — delete', () => {
  it('confirms from the menu, deletes, and removes the comment', async () => {
    mockListComments.mockResolvedValue([userComment()]);
    mockDeleteComment.mockResolvedValue(undefined);
    const { getByRole, queryByText } = await renderComments();

    await openCommentAction('Delete');
    expect(getByRole('alertdialog')).toBeTruthy();
    await confirmDelete();

    expect(mockDeleteComment).toHaveBeenCalledWith('comment-1');
    expect(queryByText('First thoughts')).toBeNull();
    expect(toastState.items.some((t) => t.text === 'Comment deleted')).toBe(true);
  });

  it('deleting the comment being edited exits edit mode and clears the input', async () => {
    mockListComments.mockResolvedValue([userComment()]);
    mockDeleteComment.mockResolvedValue(undefined);
    const { container, getByRole, queryByRole } = await renderComments();

    await openCommentAction('Edit');
    expect(textarea(container).value).toBe('First thoughts');

    await openCommentAction('Delete');
    await confirmDelete();

    expect(textarea(container).value).toBe('');
    expect(queryByRole('button', { name: 'Save' })).toBeNull();
    expect(getByRole('button', { name: 'Comment' })).toBeTruthy();
  });

  it('shows an error toast and keeps the comment when delete fails', async () => {
    mockListComments.mockResolvedValue([userComment()]);
    mockDeleteComment.mockRejectedValue({ error: 'database', message: 'boom' });
    const { getByText } = await renderComments();

    await openCommentAction('Delete');
    await confirmDelete();

    expect(getByText('First thoughts')).toBeTruthy();
    expect(toastState.items.some((t) => t.text === 'Failed to delete comment')).toBe(true);
  });

  it('keeping the comment closes the dialog without deleting', async () => {
    mockListComments.mockResolvedValue([userComment()]);
    const { getByRole, queryByRole } = await renderComments();

    await openCommentAction('Delete');
    await fireEvent.click(getByRole('button', { name: 'Keep' }));
    await tick();

    expect(queryByRole('alertdialog')).toBeNull();
    expect(mockDeleteComment).not.toHaveBeenCalled();
  });
});
