// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import type { LabelChipState, LabelSelection } from './labelFilter';
import { EMPTY_LABEL_SELECTION, clickLabelChip, labelChipState } from './labelFilter';

/** Build a selection where 'target' is in the given state, plus bystanders. */
function selectionWith(state: LabelChipState): LabelSelection {
  return {
    included: state === 'included' ? ['other-in', 'target'] : ['other-in'],
    excluded: state === 'excluded' ? ['other-ex', 'target'] : ['other-ex'],
  };
}

describe('labelChipState', () => {
  it.each([
    { state: 'neutral' as const },
    { state: 'included' as const },
    { state: 'excluded' as const },
  ])('reports $state', ({ state }) => {
    expect(labelChipState(selectionWith(state), 'target')).toBe(state);
  });

  it('reports neutral on the empty selection', () => {
    expect(labelChipState(EMPTY_LABEL_SELECTION, 'anything')).toBe('neutral');
  });
});

describe('clickLabelChip — transition table', () => {
  it.each([
    { from: 'neutral' as const, exclude: false, to: 'included' as const },
    { from: 'included' as const, exclude: false, to: 'neutral' as const },
    { from: 'excluded' as const, exclude: false, to: 'neutral' as const },
    { from: 'neutral' as const, exclude: true, to: 'excluded' as const },
    { from: 'included' as const, exclude: true, to: 'excluded' as const },
    { from: 'excluded' as const, exclude: true, to: 'neutral' as const },
  ])('$from + click(exclude=$exclude) → $to', ({ from, exclude, to }) => {
    const next = clickLabelChip(selectionWith(from), 'target', exclude);

    expect(labelChipState(next, 'target')).toBe(to);
    // Bystander labels are untouched.
    expect(next.included).toContain('other-in');
    expect(next.excluded).toContain('other-ex');
    // Invariant: never in both lists.
    expect(next.included.includes('target') && next.excluded.includes('target')).toBe(false);
  });

  it('two plain clicks flip an excluded label to included', () => {
    const cleared = clickLabelChip(selectionWith('excluded'), 'target', false);
    const flipped = clickLabelChip(cleared, 'target', false);

    expect(labelChipState(flipped, 'target')).toBe('included');
  });

  it('does not mutate the input selection', () => {
    const input = selectionWith('included');
    clickLabelChip(input, 'target', true);

    expect(input.included).toEqual(['other-in', 'target']);
    expect(input.excluded).toEqual(['other-ex']);
  });
});
