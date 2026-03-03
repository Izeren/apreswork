<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  interface Props {
    heightPx: number;
  }

  const { heightPx }: Props = $props();
</script>

{#if heightPx > 0}
  <div class="past-wash" aria-hidden="true" style="height: {heightPx}px"></div>
{/if}

<style>
  .past-wash {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    background: var(--color-calendar-past-wash);
    pointer-events: none;
    z-index: 1;
  }
</style>
