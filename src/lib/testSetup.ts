// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { beforeEach, afterEach } from 'vitest';
import { scheduleState } from './stores/schedules.svelte';

// Mark the schedules singleton as already loaded so components that call
// scheduleState.load() in $effects get a no-op instead of a real Tauri invoke.
// Tests that need specific schedule data seed scheduleState.items directly before
// rendering, and tests that inject their own ScheduleState instance via the
// schedulesStore prop are unaffected.
beforeEach(() => {
  scheduleState.loaded = true;
});

afterEach(() => {
  scheduleState.reset();
});
