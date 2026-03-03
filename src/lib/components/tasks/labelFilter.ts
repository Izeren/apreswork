// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/**
 * Tri-state label chip policy. Each label is neutral, included (part of the
 * match-all include filter), or excluded (part of the match-none exclude
 * filter). The ONE definition of chip click behavior:
 * - include gesture (left click): neutral → included; included → neutral;
 *   excluded → neutral (so on an excluded chip, one click clears it, a
 *   second click includes it);
 * - exclude gesture (right click, or ctrl+click as the keyboard path):
 *   excluded from any state; on an already-excluded chip it clears the
 *   exclusion.
 * A label is never in both lists at once. The gesture→boolean mapping lives
 * in the component; this module defines the transitions plus the per-state
 * chip tables (ordering, a11y/tooltip lookups, pseudo-chip filter mapping).
 */

import type { LabelCount } from '../../types';

export type LabelChipState = 'neutral' | 'included' | 'excluded';

export interface LabelSelection {
  readonly included: readonly string[];
  readonly excluded: readonly string[];
}

export const EMPTY_LABEL_SELECTION: LabelSelection = { included: [], excluded: [] };

export function labelChipState(selection: LabelSelection, label: string): LabelChipState {
  if (selection.included.includes(label)) return 'included';
  if (selection.excluded.includes(label)) return 'excluded';
  return 'neutral';
}

/** The transition table itself: next state for a chip click. Also used by
 * the "unlabeled" pseudo-chip, which is a single tri-state outside any
 * selection. */
export function nextChipState(state: LabelChipState, exclude: boolean): LabelChipState {
  if (exclude) {
    return state === 'excluded' ? 'neutral' : 'excluded';
  }
  return state === 'neutral' ? 'included' : 'neutral';
}

const STATE_RANK: Record<LabelChipState, number> = { included: 0, excluded: 1, neutral: 2 };

/**
 * Chip ordering: active chips first (included, then excluded — the user
 * interacted with those), the rest most-used first, ties alphabetical.
 */
export function compareLabelChips(selection: LabelSelection, a: LabelCount, b: LabelCount): number {
  return (
    STATE_RANK[labelChipState(selection, a.label)] -
      STATE_RANK[labelChipState(selection, b.label)] ||
    b.task_count - a.task_count ||
    a.label.localeCompare(b.label)
  );
}

/**
 * aria-pressed supports exactly these three tokens; 'mixed' marks the
 * excluded ("partially pressed") state.
 */
export const ARIA_PRESSED: Record<LabelChipState, 'true' | 'false' | 'mixed'> = {
  neutral: 'false',
  included: 'true',
  excluded: 'mixed',
};

export const CHIP_TITLE: Record<LabelChipState, string> = {
  neutral: 'Click to include; right-click to exclude',
  included: 'Included — click to clear; right-click to exclude',
  excluded: 'Excluded — click to clear',
};

/** What the "unlabeled" pseudo-chip's tri-state means as a TaskFilter.unlabeled value. */
export const UNLABELED_FILTER: Record<LabelChipState, boolean | null> = {
  neutral: null,
  included: true,
  excluded: false,
};

/** Apply a chip click to the selection (returns a new selection). */
export function clickLabelChip(
  selection: LabelSelection,
  label: string,
  exclude: boolean,
): LabelSelection {
  const next = nextChipState(labelChipState(selection, label), exclude);
  const without: LabelSelection = {
    included: selection.included.filter((l) => l !== label),
    excluded: selection.excluded.filter((l) => l !== label),
  };
  if (next === 'included') return { ...without, included: [...without.included, label] };
  if (next === 'excluded') return { ...without, excluded: [...without.excluded, label] };
  return without;
}
