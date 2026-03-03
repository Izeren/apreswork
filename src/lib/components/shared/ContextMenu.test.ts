// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import type { ContextMenuItem } from '../../actions/taskActions';

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

async function importContextMenu() {
  const mod = await import('./ContextMenu.svelte');
  return mod;
}

/** Wait for the open-effect's queued microtask (initial focus). */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function makeItems(overrides?: Partial<ContextMenuItem>[]): ContextMenuItem[] {
  const base: ContextMenuItem[] = [
    { label: 'First', action: vi.fn() },
    { label: 'Second', action: vi.fn() },
    { label: 'Third', action: vi.fn(), destructive: true },
  ];
  return base.map((item, i) => ({ ...item, ...overrides?.[i] }));
}

type MenuProps = {
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  onclose: () => void;
};

/**
 * Render ContextMenu with the standard test props; pass overrides to change any.
 * Returns the render result plus the resolved items/onclose and the menu +
 * menuitem elements (queried once — the nodes are stable across settle/keydown).
 */
async function renderMenu(overrides: Partial<MenuProps> = {}) {
  const { default: ContextMenu } = await importContextMenu();
  const onclose = overrides.onclose ?? vi.fn();
  const items = overrides.items ?? makeItems();
  const result = render(ContextMenu, {
    open: overrides.open ?? true,
    x: overrides.x ?? 10,
    y: overrides.y ?? 10,
    items,
    onclose,
  });
  const menu = result.container.querySelector('[role="menu"]') as HTMLElement;
  const menuitems = Array.from(result.container.querySelectorAll<HTMLElement>('[role="menuitem"]'));
  return { ...result, items, onclose, menu, menuitems };
}

describe('ContextMenu — rendering', () => {
  it('renders nothing when closed', async () => {
    const { container } = await renderMenu({ open: false });
    expect(container.querySelector('[role="menu"]')).toBeNull();
  });

  it('renders all items as menuitems, in order', async () => {
    const { menuitems } = await renderMenu();
    expect(menuitems.map((el) => el.textContent?.trim())).toEqual(['First', 'Second', 'Third']);
  });

  it('marks destructive items with the destructive class', async () => {
    const { menuitems } = await renderMenu();
    expect(menuitems[0]?.classList.contains('menu-item--destructive')).toBe(false);
    expect(menuitems[2]?.classList.contains('menu-item--destructive')).toBe(true);
  });

  it('positions the menu at the requested coordinates', async () => {
    const { menu } = await renderMenu({ x: 100, y: 200 });
    expect(menu.style.left).toBe('100px');
    expect(menu.style.top).toBe('200px');
  });
});

describe('ContextMenu — keyboard and focus', () => {
  it('focuses the first item on open', async () => {
    const { menuitems } = await renderMenu();
    await settle();
    expect(document.activeElement).toBe(menuitems[0]);
  });

  it('ArrowDown/ArrowUp move focus and wrap', async () => {
    const { menu, menuitems } = await renderMenu();
    await settle();

    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(menuitems[1]);

    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    await fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(menuitems[0]);

    await fireEvent.keyDown(menu, { key: 'ArrowUp' });
    expect(document.activeElement).toBe(menuitems[2]);
  });

  it('traps Tab within the menu', async () => {
    const { menu, menuitems } = await renderMenu();
    await settle();

    menuitems[2].focus();
    await fireEvent.keyDown(menu, { key: 'Tab' });
    expect(document.activeElement).toBe(menuitems[0]);
  });

  it('Escape closes the menu', async () => {
    const { menu, onclose } = await renderMenu();
    await settle();

    await fireEvent.keyDown(menu, { key: 'Escape' });

    expect(onclose).toHaveBeenCalledOnce();
  });
});

describe('ContextMenu — activation and dismissal', () => {
  it('clicking an item closes the menu, then runs its action', async () => {
    const { menuitems, items, onclose } = await renderMenu();

    await fireEvent.click(menuitems[1]);

    expect(onclose).toHaveBeenCalledOnce();
    expect(items[1].action).toHaveBeenCalledOnce();
    expect(items[0].action).not.toHaveBeenCalled();
  });

  it('pointerdown outside the menu closes it', async () => {
    const { onclose } = await renderMenu();
    await settle();

    await fireEvent.pointerDown(document.body);

    expect(onclose).toHaveBeenCalledOnce();
  });

  it('pointerdown inside the menu does not close it', async () => {
    const { menu, onclose } = await renderMenu();
    await settle();

    await fireEvent.pointerDown(menu);

    expect(onclose).not.toHaveBeenCalled();
  });
});

describe('ContextMenu — submenu items', () => {
  function makeSubmenuItems(): ContextMenuItem[] {
    return [
      { label: 'Plain', action: vi.fn() },
      {
        label: 'With submenu',
        submenu: createRawSnippet(() => ({
          render: () => '<div data-testid="sub-content">Sub</div>',
        })),
      },
    ];
  }

  async function renderSubmenuMenu(onclose = vi.fn()) {
    const { default: ContextMenu } = await importContextMenu();
    const items = makeSubmenuItems();
    const result = render(ContextMenu, { open: true, x: 10, y: 10, items, onclose });
    await settle();
    const submenuButton = Array.from(
      result.container.querySelectorAll<HTMLElement>('[role="menuitem"]'),
    ).find((el) => el.textContent?.trim().startsWith('With submenu'))!;
    return { ...result, items, onclose, submenuButton };
  }

  it('marks the submenu item and renders no panel until it is opened', async () => {
    const { container, submenuButton } = await renderSubmenuMenu();

    expect(submenuButton.getAttribute('aria-haspopup')).toBe('true');
    expect(submenuButton.getAttribute('aria-expanded')).toBe('false');
    expect(container.querySelector('.submenu-caret')).toBeTruthy();
    expect(container.querySelector('.submenu-panel')).toBeNull();
  });

  it('opens the panel on hover and closes it when hovering another item', async () => {
    const { container, submenuButton, queryByTestId } = await renderSubmenuMenu();

    await fireEvent.mouseEnter(submenuButton);
    expect(queryByTestId('sub-content')).toBeTruthy();
    expect(submenuButton.getAttribute('aria-expanded')).toBe('true');

    const plain = Array.from(container.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
      (el) => el.textContent?.trim() === 'Plain',
    )!;
    await fireEvent.mouseEnter(plain);
    expect(queryByTestId('sub-content')).toBeNull();
  });

  it('click toggles the panel without closing the menu or running an action', async () => {
    const { submenuButton, onclose, queryByTestId } = await renderSubmenuMenu();

    await fireEvent.click(submenuButton);
    expect(queryByTestId('sub-content')).toBeTruthy();
    expect(onclose).not.toHaveBeenCalled();

    await fireEvent.click(submenuButton);
    expect(queryByTestId('sub-content')).toBeNull();
    expect(onclose).not.toHaveBeenCalled();
  });

  it('pointerdown inside the open panel does not dismiss the menu', async () => {
    const { container, submenuButton, onclose } = await renderSubmenuMenu();

    await fireEvent.mouseEnter(submenuButton);
    const panel = container.querySelector('.submenu-panel')!;

    await fireEvent.pointerDown(panel);

    expect(onclose).not.toHaveBeenCalled();
  });

  it('re-opening the menu starts with the submenu collapsed', async () => {
    const { submenuButton, items, onclose, rerender, queryByTestId } = await renderSubmenuMenu();

    await fireEvent.mouseEnter(submenuButton);
    expect(queryByTestId('sub-content')).toBeTruthy();

    await rerender({ open: false, x: 10, y: 10, items, onclose });
    await rerender({ open: true, x: 10, y: 10, items, onclose });

    expect(queryByTestId('sub-content')).toBeNull();
  });
});

describe('clampMenuPosition', () => {
  it.each([
    { name: 'fits as-is', x: 100, y: 100, w: 200, h: 150, expected: { left: 100, top: 100 } },
    { name: 'overflows right', x: 900, y: 100, w: 200, h: 150, expected: { left: 820, top: 100 } },
    { name: 'overflows bottom', x: 100, y: 700, w: 200, h: 150, expected: { left: 100, top: 614 } },
    { name: 'clamps to margin', x: -50, y: -50, w: 200, h: 150, expected: { left: 4, top: 4 } },
  ])('$name', async ({ x, y, w, h, expected }) => {
    const { clampMenuPosition } = await importContextMenu();
    // viewport 1024×768 (jsdom default)
    expect(clampMenuPosition(x, y, w, h, 1024, 768)).toEqual(expected);
  });
});
