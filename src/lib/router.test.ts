// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { parseHash } from './router.svelte';

describe('parseHash', () => {
  it.each([
    ['#/calendar', 'calendar'],
    ['#/tasks', 'tasks'],
    ['#/status', 'status'],
    ['#/settings', 'settings'],
    ['#/profiles', 'profiles'],
    ['#calendar', 'calendar'],
    ['#tasks', 'tasks'],
    ['#status', 'status'],
    ['#profiles', 'profiles'],
    ['', 'calendar'],
    ['#/', 'calendar'],
    ['#', 'calendar'],
    ['#/unknown', 'calendar'],
    ['#/TASKS', 'calendar'],
    ['garbage', 'calendar'],
  ])('parseHash(%j) → %j', (hash, expected) => {
    expect(parseHash(hash)).toBe(expected);
  });
});
