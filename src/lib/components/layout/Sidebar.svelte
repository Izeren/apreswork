<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { router, type Route } from '../../router.svelte';
  import ProfileSwitcher from '../profile/ProfileSwitcher.svelte';

  interface Props {
    warningCount?: number;
    /** True when any warning is blocking (Unschedulable) — badge turns danger. */
    warningBlocking?: boolean;
    /** Clicking the badge itself (not the nav item) — opens the warnings modal. */
    onwarningsclick?: () => void;
  }

  const { warningCount = 0, warningBlocking = false, onwarningsclick }: Props = $props();

  interface NavItem {
    route: Route;
    label: string;
  }

  const navItems: NavItem[] = [
    { route: 'calendar', label: 'Calendar' },
    { route: 'tasks', label: 'Tasks' },
    { route: 'status', label: 'Status' },
    { route: 'settings', label: 'Settings' },
  ];

  function handleNav(route: Route) {
    router.navigate(route);
  }
</script>

<aside class="sidebar">
  <div class="sidebar-brand">
    <span class="brand-text">Après Work</span>
  </div>

  <nav class="sidebar-nav" aria-label="Main navigation">
    <ul class="nav-list">
      {#each navItems as item (item.route)}
        <li class="nav-row">
          <button
            class="nav-link"
            class:active={router.current === item.route}
            aria-current={router.current === item.route ? 'page' : undefined}
            onclick={() => handleNav(item.route)}
          >
            {item.label}
          </button>
          {#if item.route === 'status' && warningCount > 0}
            <!-- Sibling, not child: a button cannot nest inside the nav button,
                 and the badge is its own control (opens the warnings modal). -->
            <button
              class="warning-badge"
              class:warning-badge--blocking={warningBlocking}
              aria-label="Show {warningCount} {warningCount === 1 ? 'warning' : 'warnings'}"
              onclick={() => onwarningsclick?.()}
            >
              {warningCount}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  </nav>

  <ProfileSwitcher />
</aside>

<style>
  .sidebar {
    width: var(--sidebar-width);
    height: 100vh;
    min-height: 100vh;
    background: var(--color-bg-secondary);
    border-right: 1px solid var(--color-border-light);
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    overflow: hidden;
  }

  .sidebar-brand {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    padding: var(--spacing-4) var(--spacing-4);
    height: var(--header-height);
    border-bottom: 1px solid var(--color-border-light);
  }

  .brand-text {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-bold);
    color: var(--color-primary);
  }

  .nav-row {
    display: flex;
    align-items: center;
  }

  .warning-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 20px;
    height: 20px;
    margin-right: var(--spacing-4);
    padding: 0 var(--spacing-1);
    border: none;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
    background: var(--color-warning);
    border-radius: var(--radius-full);
    cursor: pointer;
  }

  .warning-badge:hover {
    filter: brightness(0.92);
  }

  .warning-badge:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .warning-badge--blocking {
    color: #ffffff;
    background: var(--color-danger);
  }

  /* Grow past the nav items so the profile switcher pins to the bottom. */
  .sidebar-nav {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .nav-list {
    list-style: none;
    padding: var(--spacing-2) 0;
  }

  .nav-link {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
    flex: 1;
    min-width: 0;
    text-align: left;
    padding: var(--spacing-2) var(--spacing-4);
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--color-text-secondary);
    font-size: var(--font-size-base);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .nav-link:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .nav-link:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .nav-link.active {
    background: var(--color-primary-light);
    color: var(--color-primary);
    font-weight: var(--font-weight-semibold);
  }
</style>
