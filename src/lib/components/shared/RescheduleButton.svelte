<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  interface Props {
    rescheduling: boolean;
    onclick: () => void;
  }

  const { rescheduling, onclick }: Props = $props();
</script>

<button
  class="reschedule-btn"
  {onclick}
  disabled={rescheduling}
  aria-label={rescheduling ? 'Rescheduling…' : 'Reschedule tasks'}
>
  <span class="reschedule-icon" class:spinning={rescheduling}>&#x21bb;</span>
  {rescheduling ? 'Scheduling…' : 'Reschedule'}
</button>
