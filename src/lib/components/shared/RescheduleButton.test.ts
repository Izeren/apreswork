// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importButton() {
  const mod = await import('./RescheduleButton.svelte');
  return mod.default;
}

describe('RescheduleButton', () => {
  it.each([
    {
      rescheduling: false,
      disabled: false,
      ariaLabel: 'Reschedule tasks',
      spinning: false,
      text: 'Reschedule',
    },
    {
      rescheduling: true,
      disabled: true,
      ariaLabel: 'Rescheduling…',
      spinning: true,
      text: 'Scheduling…',
    },
  ])(
    'rescheduling=$rescheduling — disabled=$disabled, ariaLabel="$ariaLabel", spinning=$spinning, text contains "$text"',
    async ({ rescheduling, disabled, ariaLabel, spinning, text }) => {
      const Button = await importButton();
      const { container } = render(Button, { rescheduling, onclick: vi.fn() });
      const btn = container.querySelector('button') as HTMLButtonElement;
      const icon = container.querySelector('.reschedule-icon') as HTMLElement;
      expect(btn.disabled).toBe(disabled);
      expect(btn.getAttribute('aria-label')).toBe(ariaLabel);
      expect(icon.classList.contains('spinning')).toBe(spinning);
      expect(btn.textContent).toContain(text);
    },
  );

  it('clicking fires onclick when idle (rescheduling=false)', async () => {
    const Button = await importButton();
    const onclick = vi.fn();
    const { container } = render(Button, { rescheduling: false, onclick });
    const btn = container.querySelector('button') as HTMLButtonElement;
    await fireEvent.click(btn);
    expect(onclick).toHaveBeenCalledOnce();
  });
});
