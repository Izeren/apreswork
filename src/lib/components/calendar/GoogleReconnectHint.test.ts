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

describe('GoogleReconnectHint — content', () => {
  it('shows reconnect banner text when visible', () => {
    const { getByText } = render(Hint, { visible: true, onreconnect: vi.fn() });
    expect(getByText(/Google Calendar/i)).toBeDefined();
  });

  it('renders an "Open Settings" button when visible', () => {
    const { getByRole } = render(Hint, { visible: true, onreconnect: vi.fn() });
    expect(getByRole('button', { name: /open settings/i })).toBeDefined();
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
