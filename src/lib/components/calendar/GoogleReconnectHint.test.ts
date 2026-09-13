// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import Hint from './GoogleReconnectHint.svelte';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('GoogleReconnectHint — visibility', () => {
  it.each([
    { visible: true, shouldExist: true },
    { visible: false, shouldExist: false },
  ])('renders the banner when visible=$visible', ({ visible, shouldExist }) => {
    const { container } = render(Hint, { visible, onreconnect: vi.fn() });
    const element = container.querySelector('.reconnect-hint');
    if (shouldExist) {
      expect(element).not.toBeNull();
    } else {
      expect(element).toBeNull();
    }
  });
});

type HintRtl = ReturnType<typeof render<typeof Hint>>;

describe('GoogleReconnectHint — content', () => {
  it.each<{ name: string; query: (rtl: HintRtl) => unknown }>([
    {
      name: 'shows reconnect banner text when visible',
      query: (rtl) => rtl.getByText(/Google Calendar is disconnected/i),
    },
    {
      name: 'mentions OAuth credentials in the banner text',
      query: (rtl) => rtl.getByText(/OAuth credentials/i),
    },
    {
      name: 'renders an "Open Settings" button when visible',
      query: (rtl) => rtl.getByRole('button', { name: /open settings/i }),
    },
  ])('$name', ({ query }) => {
    const rtl = render(Hint, { visible: true, onreconnect: vi.fn() });
    expect(query(rtl)).toBeDefined();
  });
});

describe('GoogleReconnectHint — interaction', () => {
  it('calls onreconnect when "Open Settings" is clicked', async () => {
    const onreconnect = vi.fn();
    const { getByRole } = render(Hint, { visible: true, onreconnect });
    await fireEvent.click(getByRole('button', { name: /open settings/i }));
    expect(onreconnect).toHaveBeenCalledTimes(1);
  });
});
