// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared harness for the TaskListView test files (TaskListView.test,
// TaskListView.filters.test). Not collected by vitest (no .test suffix).

import { afterEach, beforeEach, vi } from 'vitest';
import type { MockedFunction } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import type { TasksClient } from '../../stores/tasks.svelte';
import type { TemplatesClient, TemplateState } from '../../stores/templates.svelte';
import type { Chunk, ScheduleResult, Task, UpdateTaskInput } from '../../types';
import { baseTask, TEST_NOW } from './testFixtures';
import { warningState } from '../../stores/warnings.svelte';
import type { TaskListViewApi } from './taskListViewShared';

// Node's experimental `localStorage` global shadows jsdom's in this runner and
// its methods are unusable (started with `--localstorage-file` but no valid
// path). Install a working in-memory Storage on `window` before every test so
// the component and the tests share one functional store.
function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => {
      map.delete(key);
    },
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

/** Install a fresh in-memory `localStorage` on `window` (call from `beforeEach`). */
export function installMemoryStorage() {
  Object.defineProperty(window, 'localStorage', {
    value: memoryStorage(),
    configurable: true,
  });
}

/** Reset the task/template/schedule/warning stores (call from `afterEach`). */
export async function resetTaskListStores() {
  const [{ taskState }, { templateState }, { scheduleState }] = await Promise.all([
    import('../../stores/tasks.svelte'),
    import('../../stores/templates.svelte'),
    import('../../stores/schedules.svelte'),
  ]);
  Object.assign(taskState, {
    items: [],
    loading: false,
    selectedId: null,
    templateEditRequestId: null,
    templateEditRequestNonce: 0,
    filter: {},
  });
  Object.assign(templateState, { items: [], loading: false, loaded: false, selectedId: null });
  Object.assign(scheduleState, { items: [], loading: false, loaded: false });
  warningState.items = [];
}

// ---------------------------------------------------------------------------
// DI-based fake factories: injected via props instead of vi.mock.
// TaskState and TemplateState are dynamically imported inside renderTaskListView
// (they import api at module level; dynamic imports avoid ESM circular-init races).
// ---------------------------------------------------------------------------

export type MockedTasksClient = { [K in keyof TasksClient]: MockedFunction<TasksClient[K]> };

export type MockedTemplatesClient = {
  [K in keyof TemplatesClient]: MockedFunction<TemplatesClient[K]>;
};

export function taskListViewFakeTasksClient(listResponse: Task[] = []): MockedTasksClient {
  return {
    listTasks: vi.fn().mockResolvedValue(listResponse),
    createTask: vi.fn(),
    updateTask: vi.fn(),
    deleteTask: vi.fn(),
  };
}

export function taskListViewFakeTemplatesClient(): MockedTemplatesClient {
  return {
    listTemplates: vi.fn().mockResolvedValue([]),
    createTemplate: vi.fn(),
    updateTemplate: vi.fn(),
    deleteTemplate: vi.fn(),
  };
}

/** Creates a vi.fn()-based fake implementing TaskListViewApi for prop injection.
 *
 * Covers the full reachable surface of TaskActionsApiSubset from the task row
 * context menu so TaskActions never falls back to the real api module in tests.
 * Note: TaskDetail and TaskForm (rendered when a task is selected or edited) use
 * defaultTaskDetailApi/defaultTaskFormApi directly — their calls to listComments,
 * listChunksForTask etc. reach Tauri's invoke(), which rejects with a TypeError
 * in the test env (no runtime). Each component catches the rejection via .catch()
 * and shows a toast; there are no unhandled rejections, and test assertions remain
 * valid. Tracked for proper seam injection once TaskListView forwards an apiClient
 * to TaskDetail/TaskForm (M38+ scope).
 */
export function taskListViewFakeApi() {
  return {
    triggerReschedule: vi.fn<() => Promise<ScheduleResult>>().mockResolvedValue({
      placed_chunks: [],
      warnings: [],
    }),
    apiErrorMessage: vi
      .fn<(e: unknown, fallback: string) => string>()
      .mockImplementation((_, fallback) => fallback),
    listChunksForTask: vi.fn<(taskId: string) => Promise<Chunk[]>>().mockResolvedValue([]),
    updateTask: vi
      .fn<(taskId: string, input: UpdateTaskInput) => Promise<Task>>()
      .mockResolvedValue(baseTask()),
    deleteTask: vi.fn<(taskId: string) => Promise<void>>().mockResolvedValue(undefined),
    completeTask: vi.fn<(taskId: string) => Promise<Task>>().mockResolvedValue(baseTask()),
    cancelTask: vi.fn<(taskId: string) => Promise<Task>>().mockResolvedValue(baseTask()),
    getTask: vi.fn<(taskId: string) => Promise<Task>>().mockResolvedValue(baseTask()),
    createFixedChunk:
      vi.fn<(taskId: string, start: string, end: string) => Promise<[Chunk, Task]>>(),
  } satisfies TaskListViewApi;
}

/**
 * Renders TaskListView with DI-injected taskStore, templateStore, and apiClient fakes.
 * Settles the mount-time load before returning render utils plus the injected
 * stores and fakeApi so tests can assert state and mock calls.
 */
export async function renderTaskListView(
  fakeTasksClient: TasksClient,
  opts: {
    fakeApi?: ReturnType<typeof taskListViewFakeApi>;
    getNow?: () => Date;
    fakeTemplatesClient?: TemplatesClient;
  } = {},
) {
  const { TaskState } = await import('../../stores/tasks.svelte');
  const store = new TaskState(fakeTasksClient);
  const fakeApi = opts.fakeApi ?? taskListViewFakeApi();
  let templateStore: TemplateState | undefined;
  if (opts.fakeTemplatesClient) {
    const { TemplateState: TemplateStateClass } = await import('../../stores/templates.svelte');
    templateStore = new TemplateStateClass(opts.fakeTemplatesClient);
  }
  const { default: TaskListView } = await import('./TaskListView.svelte');
  const utils = render(TaskListView, {
    props: {
      getNow: opts.getNow ?? (() => TEST_NOW),
      apiClient: fakeApi,
      taskStore: store,
      ...(templateStore ? { templateStore } : {}),
    },
  });
  await tick();
  return { store, fakeApi, templateStore, ...utils };
}

/**
 * Register the shared TaskListView test lifecycle (call at module top level):
 * install in-memory `localStorage` before each test; after each, unmount, clear
 * mock history, run `afterEachExtra` (suite-specific teardown), then reset the
 * task-list stores.
 */
export function installTaskListLifecycle(afterEachExtra?: () => void) {
  beforeEach(() => {
    installMemoryStorage();
  });
  afterEach(async () => {
    cleanup();
    vi.clearAllMocks();
    afterEachExtra?.();
    await resetTaskListStores();
  });
}
