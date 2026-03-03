<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { ExternalEvent } from '../../types';
  import type { RangeLayoutItem } from './overlapLayout';
  import ExternalEventBlock from './ExternalEventBlock.svelte';

  interface Props {
    /** Timed external events laid out for one day column, in overlap-lane order. */
    externals: RangeLayoutItem<ExternalEvent>[];
    eventOpenHandler: (ext: ExternalEvent) => ((event: ExternalEvent) => void) | null;
    disconnected?: boolean;
  }

  const { externals, eventOpenHandler, disconnected = false }: Props = $props();
</script>

{#each externals as extLayout (extLayout.item.event_id)}
  <ExternalEventBlock
    event={extLayout.item}
    overlapIndex={extLayout.overlapIndex}
    overlapCount={extLayout.overlapCount}
    onopen={eventOpenHandler(extLayout.item)}
    {disconnected}
  />
{/each}
