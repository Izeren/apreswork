// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { tick } from 'svelte';
import { toastState } from '../../stores/toast.svelte';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  toastState.items = [];
});

async function importComponent() {
  const mod = await import('./MarkdownView.svelte');
  return mod.default;
}

describe('MarkdownView — rendering', () => {
  it('renders bold markdown as a <strong> element', async () => {
    const MarkdownView = await importComponent();
    const { container } = render(MarkdownView, { source: '**bold text**' });
    await tick();
    expect(container.querySelector('strong')).toBeTruthy();
    expect(container.querySelector('strong')!.textContent).toBe('bold text');
  });

  it('renders empty string without errors', async () => {
    const MarkdownView = await importComponent();
    const { container } = render(MarkdownView, { source: '' });
    await tick();
    expect(container.querySelector('.markdown-view')).toBeTruthy();
  });
});

describe('MarkdownView — link click handling', () => {
  it('prevents default and calls openUrl when an anchor is clicked', async () => {
    const openUrl = vi.fn().mockResolvedValue(undefined);
    const MarkdownView = await importComponent();
    const { container } = render(MarkdownView, {
      source: '[example](https://example.com)',
      openUrl,
    });
    await tick();

    const anchor = container.querySelector('a[href]') as HTMLAnchorElement;
    expect(anchor).toBeTruthy();

    const clickEvent = new MouseEvent('click', { bubbles: true, cancelable: true });
    anchor.dispatchEvent(clickEvent);
    await tick();

    expect(clickEvent.defaultPrevented).toBe(true);
    expect(openUrl).toHaveBeenCalledWith(anchor.href);
  });

  it('shows an error toast when openUrl rejects', async () => {
    const openUrl = vi.fn().mockRejectedValue(new Error('opener unavailable'));
    const MarkdownView = await importComponent();
    const { container } = render(MarkdownView, {
      source: '[example](https://example.com)',
      openUrl,
    });
    await tick();

    const anchor = container.querySelector('a[href]') as HTMLAnchorElement;
    anchor.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    await tick();
    await tick(); // flush the rejection's catch microtask

    expect(toastState.items).toHaveLength(1);
    expect(toastState.items[0].level).toBe('error');
    expect(toastState.items[0].text).toBe('Failed to open link');
  });

  it('does not call openUrl when non-link content is clicked', async () => {
    const openUrl = vi.fn();
    const MarkdownView = await importComponent();
    const { container } = render(MarkdownView, { source: 'plain paragraph text', openUrl });
    await tick();

    const wrapper = container.querySelector('.markdown-view') as HTMLElement;
    await fireEvent.click(wrapper);
    await tick();

    expect(openUrl).not.toHaveBeenCalled();
  });
});
