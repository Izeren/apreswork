<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script module lang="ts">
  import type { Snippet } from 'svelte';
  import {
    deadlineExtendItems,
    type ContextMenuItem,
    type TaskActions,
  } from '../../actions/taskActions';
  import type { ScheduleWarning } from '../../types';
  import { formatDateTime } from '../../utils';
  import { customDeadlineIso } from '../shared/deadlinePresets';

  /**
   * The one definition of which resolution verbs a warning offers.
   * "Scheduled date" maps to `earliest_completion`, which only exists for
   * `DeadlineViolation` — Unschedulable tasks have no projected completion.
   *
   * Preset labels show the concrete target datetime, so build the items when
   * the menu opens (passing that instant as `now`), not once per mount.
   */
  export function resolutionMenuItems(
    warning: ScheduleWarning,
    actions: TaskActions,
    deadlineCalendar: Snippet,
    now: Date,
  ): ContextMenuItem[] {
    const { task_id: taskId, task_title: title } = warning;
    const items: ContextMenuItem[] = [...deadlineExtendItems(taskId, actions, now)];
    if ('DeadlineViolation' in warning.kind) {
      const earliest = warning.kind.DeadlineViolation.earliest_completion;
      items.push({
        label: `Extend to scheduled date (${formatDateTime(earliest)})`,
        action: () => actions.extendDeadline(taskId, earliest),
      });
    }
    items.push(
      { label: 'Custom deadline', submenu: deadlineCalendar },
      { label: 'Do now', action: () => actions.doNow(taskId, now) },
      { label: 'Complete task', action: () => actions.completeTask(taskId, title) },
      { label: 'Cancel task', destructive: true, action: () => actions.cancelTask(taskId, title) },
    );
    return items;
  }
</script>

<script lang="ts">
  import ContextMenu from '../shared/ContextMenu.svelte';
  import MiniCalendar from '../shared/MiniCalendar.svelte';
  import { isoToLocalDate } from '../shared/dateTimePickerShared';
  import { appClock } from '../../app-clock';

  interface Props {
    warning: ScheduleWarning;
    actions: TaskActions;
  }

  const { warning, actions }: Props = $props();

  let open = $state(false);
  let menuX = $state(0);
  let menuY = $state(0);
  let menuOpenedAt = $state(appClock());

  const existingDeadline = $derived(
    'DeadlineViolation' in warning.kind ? warning.kind.DeadlineViolation.deadline : null,
  );

  function toggleMenu(event: MouseEvent): void {
    if (open) {
      open = false;
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    menuX = rect.left;
    menuY = rect.bottom + 4;
    menuOpenedAt = new Date();
    open = true;
  }

  function pickCustomDeadline(localDate: string): void {
    open = false;
    void actions.extendDeadline(warning.task_id, customDeadlineIso(localDate));
  }
</script>

{#snippet deadlineCalendar()}
  <MiniCalendar
    selected={existingDeadline ? isoToLocalDate(existingDeadline) : null}
    onpick={pickCustomDeadline}
    today={menuOpenedAt}
  />
{/snippet}

<button
  class="resolve-trigger"
  type="button"
  aria-haspopup="menu"
  aria-expanded={open}
  aria-label={`Resolve ${warning.task_title}`}
  onclick={toggleMenu}
>
  Resolve ▾
</button>

<ContextMenu
  {open}
  x={menuX}
  y={menuY}
  items={resolutionMenuItems(warning, actions, deadlineCalendar, menuOpenedAt)}
  onclose={() => (open = false)}
/>

<style>
  .resolve-trigger {
    flex-shrink: 0;
    padding: var(--spacing-1) var(--spacing-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .resolve-trigger:hover {
    background: var(--color-surface-hover);
  }

  .resolve-trigger:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
