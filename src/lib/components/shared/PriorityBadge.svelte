<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { Priority } from '../../types';

  interface Props {
    priority: Priority;
  }

  const { priority }: Props = $props();

  const colorMap: Record<Priority, string> = {
    Low: 'var(--color-priority-low)',
    Medium: 'var(--color-priority-medium)',
    High: 'var(--color-priority-high)',
    Critical: 'var(--color-priority-critical)',
  };

  const bg = $derived(colorMap[priority]);
</script>

<span class="priority-badge" style="background: {bg};">{priority}</span>

<style>
  .priority-badge {
    color: #ffffff;
  }
</style>
