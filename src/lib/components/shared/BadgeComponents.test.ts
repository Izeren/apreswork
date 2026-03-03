// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import type { Priority } from '../../types';
import PriorityBadge from './PriorityBadge.svelte';
import StatusBadge from './StatusBadge.svelte';
import LabelChip from './LabelChip.svelte';
import Toast from './Toast.svelte';
import { toastState } from '../../stores/toast.svelte';
import { statusCases } from '../../testFixtures';

afterEach(() => {
  cleanup();
  toastState.items = [];
});

describe('PriorityBadge — renders each priority', () => {
  const cases: Array<{ priority: Priority; label: string }> = [
    { priority: 'Low', label: 'Low' },
    { priority: 'Medium', label: 'Medium' },
    { priority: 'High', label: 'High' },
    { priority: 'Critical', label: 'Critical' },
  ];

  it.each(cases)('renders "$priority" badge with text "$label"', ({ priority, label }) => {
    const { getByText } = render(PriorityBadge, { priority });
    expect(getByText(label)).toBeTruthy();
  });
});

describe('PriorityBadge — structure', () => {
  it('renders a single inline element (span)', () => {
    const { container } = render(PriorityBadge, { priority: 'Low' });
    const badge = container.querySelector('.priority-badge');
    expect(badge).toBeTruthy();
  });
});

describe('StatusBadge — renders each status', () => {
  it.each(statusCases)(
    'renders "$status" badge with capitalized text "$label"',
    ({ status, label }) => {
      const { getByText } = render(StatusBadge, { status });
      expect(getByText(label)).toBeTruthy();
    },
  );
});

describe('StatusBadge — structure', () => {
  it('renders a single inline element with class status-badge', () => {
    const { container } = render(StatusBadge, { status: 'backlog' });
    expect(container.querySelector('.status-badge')).toBeTruthy();
  });
});

describe('LabelChip — label text', () => {
  it.each([{ label: 'frontend' }, { label: '' }, { label: 'c++ / rust' }])(
    'renders chip-text as "$label"',
    ({ label }) => {
      const { container } = render(LabelChip, { label });
      expect(container.querySelector('.chip-text')?.textContent).toBe(label);
    },
  );
});

describe('LabelChip — remove button', () => {
  it('shows remove button when onremove is provided', () => {
    const { getByRole } = render(LabelChip, { label: 'tag', onremove: vi.fn() });
    expect(getByRole('button', { name: /remove/i })).toBeTruthy();
  });

  it('hides remove button when onremove is not provided', () => {
    const { queryByRole } = render(LabelChip, { label: 'tag' });
    expect(queryByRole('button')).toBeNull();
  });

  it('calls onremove callback when X is clicked', async () => {
    const onremove = vi.fn();
    const { getByRole } = render(LabelChip, { label: 'tag', onremove });
    await fireEvent.click(getByRole('button', { name: /remove/i }));
    expect(onremove).toHaveBeenCalledTimes(1);
  });

  it('does not throw when onremove is undefined and no button is present', () => {
    expect(() => render(LabelChip, { label: 'safe' })).not.toThrow();
  });
});

describe('Toast — renders items', () => {
  it('renders nothing when toast queue is empty', () => {
    const { container } = render(Toast);
    const items = container.querySelectorAll('.toast-item');
    expect(items).toHaveLength(0);
  });

  it('renders all items in the toast queue', () => {
    toastState.items = [
      { id: '1', level: 'success', text: 'Saved!' },
      { id: '2', level: 'error', text: 'Failed!' },
    ];
    const { getAllByRole } = render(Toast);
    const items = getAllByRole('status');
    expect(items).toHaveLength(2);
  });

  it('renders toast message text', () => {
    toastState.items = [{ id: '1', level: 'info', text: 'Hello toast' }];
    const { getByText } = render(Toast);
    expect(getByText('Hello toast')).toBeTruthy();
  });
});

describe('Toast — level classes', () => {
  const levels = ['success', 'error', 'info', 'warning'] as const;

  it.each(levels)('renders a toast item for level "%s"', (level) => {
    toastState.items = [{ id: '42', level, text: `A ${level} message` }];
    const { getByText } = render(Toast);
    expect(getByText(`A ${level} message`)).toBeTruthy();
  });
});

describe('Toast — dismiss', () => {
  it('renders dismiss button for each toast item', () => {
    toastState.items = [{ id: '1', level: 'info', text: 'Dismissible' }];
    const { getByRole } = render(Toast);
    expect(getByRole('button', { name: /dismiss/i })).toBeTruthy();
  });

  it.each([
    {
      label: 'removes the only toast when clicked',
      initial: [{ id: '99', level: 'warning' as const, text: 'Watch out' }],
      expectedLength: 0,
      expectedFirstId: undefined as string | undefined,
    },
    {
      label: 'only removes the clicked toast when multiple present',
      initial: [
        { id: 'a', level: 'success' as const, text: 'First' },
        { id: 'b', level: 'error' as const, text: 'Second' },
      ],
      expectedLength: 1,
      expectedFirstId: 'b' as string | undefined,
    },
  ])('$label', async ({ initial, expectedLength, expectedFirstId }) => {
    toastState.items = initial;
    const { getAllByRole } = render(Toast);
    await fireEvent.click(getAllByRole('button', { name: /dismiss/i })[0]);
    expect(toastState.items).toHaveLength(expectedLength);
    expect(toastState.items[0]?.id).toBe(expectedFirstId);
  });
});
