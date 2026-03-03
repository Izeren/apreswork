<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Chunk } from '../../types';
  import { formatDateTime } from '../../utils';

  interface Props {
    chunks: Chunk[];
    loading: boolean;
    label?: string;
    /** Rendered after the chunk-time span for each chunk (badges, actions, ...). */
    trailing: Snippet<[Chunk]>;
  }

  const { chunks, loading, label = 'Scheduled chunks', trailing }: Props = $props();
</script>

<span class="section-label">{label}</span>
{#if loading}
  <p class="chunks-state">Loading chunks…</p>
{:else if chunks.length === 0}
  <p class="chunks-state chunks-state--empty">No chunks scheduled</p>
{:else}
  <ul class="chunks-list" aria-label={label}>
    {#each chunks as chunk (chunk.id)}
      <li class="chunk-item">
        <span class="chunk-time"
          >{formatDateTime(chunk.start_time)} – {formatDateTime(chunk.end_time)}</span
        >
        {@render trailing(chunk)}
      </li>
    {/each}
  </ul>
{/if}
