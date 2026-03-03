<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { ScheduleWindow } from '../../types';
  import { getWeekdayName, parseTimeToHours } from '../../utils';
  import { HOUR_HEIGHT_PX } from './calendarLayout';

  interface Props {
    windows: ScheduleWindow[];
    date: Date;
  }

  const { windows, date }: Props = $props();

  interface Band {
    id: string;
    top: number;
    height: number;
  }

  const bands = $derived.by((): Band[] => {
    const dayName = getWeekdayName(date);
    return windows
      .filter((w) => w.day_of_week === dayName)
      .map((w) => {
        const startHours = parseTimeToHours(w.start_time);
        const endHours = parseTimeToHours(w.end_time);
        return {
          id: w.id,
          top: startHours * HOUR_HEIGHT_PX,
          height: (endHours - startHours) * HOUR_HEIGHT_PX,
        };
      });
  });
</script>

{#each bands as band (band.id)}
  <div
    class="schedule-window-band"
    aria-hidden="true"
    style="top: {band.top}px; height: {band.height}px"
  ></div>
{/each}

<style>
  .schedule-window-band {
    position: absolute;
    left: 0;
    right: 0;
    background: var(--color-schedule-window);
    z-index: 1;
    pointer-events: none;
  }
</style>
