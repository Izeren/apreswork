// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import type { Mocked } from 'vitest';
import { fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { Chunk } from '../../types';
import { baseTask, baseChunk } from './testFixtures';
import {
  setInputValue,
  installTaskFormHooks,
  renderTaskForm,
  taskFormFakeApi,
} from './taskFormTestSupport';
import type { TaskFormApi, TaskFormProps } from './taskFormTestSupport';

let fake: Mocked<TaskFormApi>;

beforeEach(() => {
  fake = taskFormFakeApi();
});

installTaskFormHooks();

async function importFocusState() {
  const mod = await import('../../stores/calendarFocus.svelte');
  return mod.calendarFocusState;
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
  await tick();
}

describe('TaskForm — chunk section', () => {
  afterEach(async () => {
    const focus = await importFocusState();
    focus.clear();
    window.location.hash = '';
  });

  async function renderEdit(chunks: Chunk[], props: Partial<TaskFormProps> = {}) {
    fake.listChunksForTask.mockResolvedValue(chunks);
    const utils = await renderTaskForm(fake, { task: baseTask(), ...props });
    await flush();
    return utils;
  }

  async function triggerFixedChunkAction(
    action: (chunkId: string) => Promise<Chunk>,
    buttonName: string,
  ) {
    vi.mocked(action).mockResolvedValue(baseChunk({ id: 'c-fixed' }));
    const onchunkschange = vi.fn();
    const { getByRole } = await renderEdit([baseChunk({ id: 'c-fixed', is_fixed: true })], {
      onchunkschange,
    });

    fake.listChunksForTask.mockClear();
    await fireEvent.click(getByRole('button', { name: buttonName }));
    await tick();
    await flush();

    expect(action).toHaveBeenCalledWith('c-fixed');
    expect(fake.listChunksForTask).toHaveBeenCalledWith('task-1');
    expect(onchunkschange).toHaveBeenCalled();
  }

  it('renders the chunks with times and a fixed badge in edit mode', async () => {
    const { container } = await renderEdit([
      baseChunk(),
      baseChunk({ id: 'chunk-2', is_fixed: true }),
    ]);

    expect(fake.listChunksForTask).toHaveBeenCalledWith('task-1');
    expect(container.querySelectorAll('.chunk-item')).toHaveLength(2);
    expect(container.querySelectorAll('.chunk-time')).toHaveLength(2);
    expect(container.querySelectorAll('.fixed-badge')).toHaveLength(1);
  });

  it('shows an empty state when the task has no chunks', async () => {
    const { getByText } = await renderEdit([]);
    expect(getByText('No chunks scheduled')).toBeTruthy();
  });

  it('does not fetch or render chunks in create mode', async () => {
    const { container } = await renderTaskForm(fake);
    await flush();

    expect(fake.listChunksForTask).not.toHaveBeenCalled();
    expect(container.querySelector('.chunk-section')).toBeNull();
  });

  it('offers Unlock and Delete only on fixed, non-completed chunks', async () => {
    const { container } = await renderEdit([
      baseChunk({ id: 'c-auto' }),
      baseChunk({ id: 'c-fixed', is_fixed: true }),
      baseChunk({ id: 'c-done', is_fixed: true, status: 'completed' }),
    ]);

    const rows = Array.from(container.querySelectorAll('.chunk-item'));
    const labelsIn = (row: Element) =>
      Array.from(row.querySelectorAll('button')).map((b) => b.textContent?.trim());
    expect(labelsIn(rows[0])).toEqual(['Show in calendar']);
    expect(labelsIn(rows[1])).toEqual(['Show in calendar', 'Unlock', 'Delete']);
    expect(labelsIn(rows[2])).toEqual(['Show in calendar']);
  });

  it.each([
    { verb: 'unlocks', actionKey: 'unlockChunk' as const, buttonName: 'Unlock' },
    { verb: 'deletes', actionKey: 'deleteFixedChunk' as const, buttonName: 'Delete' },
  ])(
    '$verb a fixed chunk, reloads the list, and notifies the parent',
    async ({ actionKey, buttonName }) => {
      await triggerFixedChunkAction(fake[actionKey], buttonName);
    },
  );

  async function showInCalendar(titleValue: string) {
    const focus = await importFocusState();
    const nonceBefore = focus.nonce;
    const onsubmit = vi.fn();
    window.location.hash = '#/tasks';
    const { getByRole, getByPlaceholderText } = await renderEdit([baseChunk()], { onsubmit });

    await setInputValue(getByPlaceholderText('Task title') as HTMLInputElement, titleValue);
    await fireEvent.click(getByRole('button', { name: 'Show in calendar' }));
    await flush();

    return { focus, nonceBefore, onsubmit };
  }

  it('Show in calendar saves the form, records the focus request, and navigates', async () => {
    // Dirty the form: a pristine edit form closes without submitting (see next test).
    const { focus, nonceBefore, onsubmit } = await showInCalendar('Changed Title');

    expect(onsubmit).toHaveBeenCalledTimes(1);
    expect(focus.chunkId).toBe('chunk-1');
    expect(focus.startTime).toBe('2026-05-10T09:00:00.000Z');
    expect(focus.nonce).toBe(nonceBefore + 1);
    expect(window.location.hash).toBe('#/calendar');
  });

  it('Show in calendar on a pristine form closes without saving and still navigates', async () => {
    const focus = await importFocusState();
    const onsubmit = vi.fn();
    const onclose = vi.fn();
    window.location.hash = '#/tasks';
    const { getByRole } = await renderEdit([baseChunk()], { onsubmit, onclose });

    await fireEvent.click(getByRole('button', { name: 'Show in calendar' }));

    expect(onsubmit).not.toHaveBeenCalled();
    expect(onclose).toHaveBeenCalledTimes(1);
    expect(focus.chunkId).toBe('chunk-1');
    expect(window.location.hash).toBe('#/calendar');
  });

  it('Show in calendar does not navigate when validation fails', async () => {
    const { focus, nonceBefore, onsubmit } = await showInCalendar('');

    expect(onsubmit).not.toHaveBeenCalled();
    expect(focus.nonce).toBe(nonceBefore);
    expect(window.location.hash).toBe('#/tasks');
  });
});
