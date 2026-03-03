// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import ViewportRepositionFixture from './viewportRepositionFixture.svelte';

// repositionOnViewportChange uses $effect which requires a Svelte reactive root.
// ViewportRepositionFixture is a minimal host component that calls it and exposes
// isOpen/reposition as props so tests can drive the reactive state from outside.

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('repositionOnViewportChange', () => {
  it('does not attach listeners when isOpen is false', () => {
    const reposition = vi.fn();
    render(ViewportRepositionFixture, { isOpen: false, reposition });

    window.dispatchEvent(new Event('resize'));
    document.dispatchEvent(new Event('scroll'));

    expect(reposition).not.toHaveBeenCalled();
  });

  it('attaches a resize listener when isOpen is true', () => {
    const reposition = vi.fn();
    render(ViewportRepositionFixture, { isOpen: true, reposition });

    window.dispatchEvent(new Event('resize'));

    expect(reposition).toHaveBeenCalledOnce();
  });

  it('attaches a document scroll listener (capture) when isOpen is true', () => {
    const reposition = vi.fn();
    render(ViewportRepositionFixture, { isOpen: true, reposition });

    // Capture-phase listener on document responds to a scroll event targeted at document
    document.dispatchEvent(new Event('scroll', { bubbles: false }));

    expect(reposition).toHaveBeenCalledOnce();
  });

  it('detaches listeners when isOpen toggles from true to false', () => {
    const reposition = vi.fn();
    const { rerender } = render(ViewportRepositionFixture, { isOpen: true, reposition });

    window.dispatchEvent(new Event('resize'));
    expect(reposition).toHaveBeenCalledOnce();
    reposition.mockClear();

    // Toggle off — prop change triggers effect cleanup and re-run with early return
    rerender({ isOpen: false, reposition });
    flushSync();

    window.dispatchEvent(new Event('resize'));
    document.dispatchEvent(new Event('scroll', { bubbles: false }));
    expect(reposition).not.toHaveBeenCalled();
  });

  it('re-rendering with the same isOpen=true does not double-attach listeners', () => {
    const reposition = vi.fn();
    const { rerender } = render(ViewportRepositionFixture, { isOpen: true, reposition });

    // Same value — no reactive change, effect does not re-run, listeners not duplicated
    rerender({ isOpen: true, reposition });
    flushSync();

    window.dispatchEvent(new Event('resize'));

    expect(reposition).toHaveBeenCalledOnce();
  });
});
