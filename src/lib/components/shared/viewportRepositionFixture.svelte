<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!--
  Test-only fixture: mounts repositionOnViewportChange inside a Svelte 5 reactive
  root so its $effect can run in vitest (jsdom). Not imported by any production code.
-->

<script lang="ts">
  import { repositionOnViewportChange } from './viewportReposition.svelte';

  interface Props {
    isOpen: boolean;
    reposition: () => void;
  }

  const { isOpen, reposition }: Props = $props();

  // isOpen is a reactive Svelte 5 prop: reading it inside repositionOnViewportChange's
  // $effect creates a reactive dependency, so toggling the prop re-runs the effect
  // (removes old listeners, conditionally adds new ones).
  repositionOnViewportChange(
    () => isOpen,
    // Wrap in a closure so the effect always calls the current prop value,
    // not the one captured at component initialisation time.
    () => reposition(),
  );
</script>
