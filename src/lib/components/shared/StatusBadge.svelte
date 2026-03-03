<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { TaskStatus, ChunkStatus } from '../../types';

  interface Props {
    // ChunkStatus ('scheduled' | 'completed') is a strict subset of TaskStatus,
    // so widening the prop type here avoids a type error when rendering chunk rows.
    status: TaskStatus | ChunkStatus;
  }

  const { status }: Props = $props();

  const colorMap: Record<TaskStatus, string> = {
    backlog: 'var(--color-status-backlog)',
    pending: 'var(--color-status-pending)',
    scheduled: 'var(--color-status-scheduled)',
    completed: 'var(--color-status-completed)',
    cancelled: 'var(--color-status-cancelled)',
  };

  // ChunkStatus values ('scheduled' | 'completed') are always present in colorMap.
  const bg = $derived(colorMap[status as TaskStatus]);
  const label = $derived(status.charAt(0).toUpperCase() + status.slice(1));
</script>

<span class="status-badge" style="background: {bg};">{label}</span>

<style>
  .status-badge {
    color: #ffffff;
  }
</style>
