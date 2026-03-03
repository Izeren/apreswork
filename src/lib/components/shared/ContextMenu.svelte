<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script module lang="ts">
  /** Gap kept between the menu and the viewport edges. */
  const VIEWPORT_MARGIN = 4;

  /** Assumed submenu panel width for the flip-left overflow heuristic. */
  const SUBMENU_ESTIMATED_WIDTH = 280;

  /** Keep the menu fully on-screen: shift left/up on overflow, never off-edge. */
  export function clampMenuPosition(
    x: number,
    y: number,
    menuWidth: number,
    menuHeight: number,
    viewportWidth: number,
    viewportHeight: number,
  ): { left: number; top: number } {
    return {
      left: Math.max(VIEWPORT_MARGIN, Math.min(x, viewportWidth - menuWidth - VIEWPORT_MARGIN)),
      top: Math.max(VIEWPORT_MARGIN, Math.min(y, viewportHeight - menuHeight - VIEWPORT_MARGIN)),
    };
  }
</script>

<script lang="ts">
  import type { ContextMenuItem } from '../../actions/taskActions';
  import { focusFirst, getFocusableElements, handleTabTrap } from './focusTrap';

  interface Props {
    open: boolean;
    /** Viewport coordinates to anchor the menu at (e.g. from a contextmenu event). */
    x: number;
    y: number;
    items: ContextMenuItem[];
    onclose: () => void;
  }

  const { open, x, y, items, onclose }: Props = $props();

  let menuEl: HTMLElement | null = $state(null);
  let menuWidth = $state(0);
  let menuHeight = $state(0);
  let previousFocus: HTMLElement | null = null;
  /** Label of the item whose submenu panel is open (labels are unique keys). */
  let submenuFor = $state<string | null>(null);
  let submenuOpensLeft = $state(false);

  const position = $derived(
    clampMenuPosition(x, y, menuWidth, menuHeight, window.innerWidth, window.innerHeight),
  );

  // Measure after mount (and when contents change) so clamping can use the
  // real size. getBoundingClientRect instead of dimension bindings: those
  // need ResizeObserver, which jsdom doesn't provide.
  $effect(() => {
    void items;
    if (open && menuEl) {
      const rect = menuEl.getBoundingClientRect();
      menuWidth = rect.width;
      menuHeight = rect.height;
    }
  });

  $effect(() => {
    if (open) {
      previousFocus = document.activeElement as HTMLElement | null;
      // Use a microtask so the DOM is mounted before we try to focus
      queueMicrotask(() => {
        if (menuEl) focusFirst(menuEl);
      });
      return () => {
        if (previousFocus) {
          previousFocus.focus();
          previousFocus = null;
        }
      };
    }
  });

  // Dismiss on any pointerdown outside the menu. Capture phase, so a press the
  // menu's own surfaces swallow (stopPropagation) still can't leak a close.
  $effect(() => {
    if (!open) return;
    const handleOutsidePointerDown = (event: PointerEvent) => {
      if (menuEl && !menuEl.contains(event.target as Node)) onclose();
    };
    document.addEventListener('pointerdown', handleOutsidePointerDown, true);
    return () => document.removeEventListener('pointerdown', handleOutsidePointerDown, true);
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose();
      return;
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!menuEl) return;
      const focusable = getFocusableElements(menuEl);
      if (focusable.length === 0) return;
      const index = focusable.indexOf(document.activeElement as HTMLElement);
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      focusable[(index + delta + focusable.length) % focusable.length].focus();
      return;
    }
    if (menuEl) {
      handleTabTrap(event, menuEl);
    }
  }

  // The whole menu unmounts on close but the component instance survives —
  // drop any open submenu so a re-open starts collapsed.
  $effect(() => {
    if (!open) submenuFor = null;
  });

  function openSubmenu(item: ContextMenuItem) {
    // Flip to the left edge when the panel would overflow the viewport.
    if (menuEl) {
      const rect = menuEl.getBoundingClientRect();
      submenuOpensLeft = rect.right + SUBMENU_ESTIMATED_WIDTH > window.innerWidth - VIEWPORT_MARGIN;
    }
    submenuFor = item.label;
  }

  /** Hovering a submenu item opens its panel; hovering any other item closes it. */
  function handleItemHover(item: ContextMenuItem) {
    if (item.submenu) openSubmenu(item);
    else submenuFor = null;
  }

  function handleItemClick(item: ContextMenuItem) {
    if (item.submenu) {
      if (submenuFor === item.label) submenuFor = null;
      else openSubmenu(item);
      return;
    }
    onclose();
    void item.action?.();
  }
</script>

{#if open}
  <div
    bind:this={menuEl}
    class="context-menu"
    role="menu"
    tabindex="-1"
    style:left={`${position.left}px`}
    style:top={`${position.top}px`}
    onkeydown={handleKeydown}
  >
    {#each items as item (item.label)}
      <div class="menu-item-wrap">
        <button
          class="menu-item"
          class:menu-item--destructive={item.destructive}
          role="menuitem"
          aria-haspopup={item.submenu ? 'true' : undefined}
          aria-expanded={item.submenu ? submenuFor === item.label : undefined}
          onclick={() => handleItemClick(item)}
          onmouseenter={() => handleItemHover(item)}
        >
          {item.label}
          {#if item.submenu}<span class="submenu-caret" aria-hidden="true">›</span>{/if}
        </button>
        {#if item.submenu && submenuFor === item.label}
          <!-- Inside menuEl on purpose: the outside-pointerdown closer must
               treat the panel as part of the menu. -->
          <div class="submenu-panel" class:submenu-panel--left={submenuOpensLeft}>
            {@render item.submenu()}
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 1200;
    display: flex;
    flex-direction: column;
    min-width: 180px;
    padding: var(--spacing-1);
  }

  .menu-item-wrap {
    position: relative;
    display: flex;
    flex-direction: column;
  }

  .menu-item {
    border-radius: var(--radius-sm);
    color: var(--color-text);
    transition: background var(--transition-fast);
  }

  .submenu-caret {
    color: var(--color-text-tertiary);
  }

  /* Flush against the menu edge (no gap) so the pointer never crosses a dead
     zone between the item and its panel. */
  .submenu-panel {
    position: absolute;
    top: calc(-1 * var(--spacing-1));
    left: 100%;
    z-index: 1;
    padding: var(--spacing-2);
  }

  .submenu-panel--left {
    left: auto;
    right: 100%;
  }

  .menu-item:hover {
    background: var(--color-surface-hover);
  }

  .menu-item:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .menu-item--destructive {
    color: var(--color-danger);
  }
</style>
