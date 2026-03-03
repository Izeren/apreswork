// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { it, expect } from 'vitest';
import type { ExternalEvent } from '../../types';
import { resolveEventOpenHandler } from './externalEventInteractivity';

function ev(overrides: Partial<ExternalEvent> = {}): ExternalEvent {
  return {
    id: 'row-1',
    calendar_id: 'cal-primary',
    event_id: 'provider-event-1',
    title: 'Team meeting',
    description: null,
    start_time: '2026-03-28T12:00:00Z',
    end_time: '2026-03-28T13:00:00Z',
    busy: true,
    declined: false,
    all_day: false,
    updated_at: '2026-03-28T10:00:00Z',
    ...overrides,
  };
}

const handler = (): void => {};

type ResolveCase = {
  name: string;
  openHandler: (() => void) | null;
  editableId: string | null;
  event: ExternalEvent;
  expected: (() => void) | null;
};

it.each<ResolveCase>([
  {
    name: 'returns the handler for an event on the editable (primary) calendar',
    openHandler: handler,
    editableId: 'cal-primary',
    event: ev(),
    expected: handler,
  },
  {
    name: 'returns null for an event on a non-editable calendar',
    openHandler: handler,
    editableId: 'cal-primary',
    event: ev({ calendar_id: 'other' }),
    expected: null,
  },
  {
    name: 'returns null when there is no editable calendar',
    openHandler: handler,
    editableId: null,
    event: ev(),
    expected: null,
  },
  {
    name: 'returns null when no open handler is provided',
    openHandler: null,
    editableId: 'cal-primary',
    event: ev(),
    expected: null,
  },
])('resolveEventOpenHandler — $name', ({ openHandler, editableId, event, expected }) => {
  expect(resolveEventOpenHandler(openHandler, editableId, event)).toBe(expected);
});
