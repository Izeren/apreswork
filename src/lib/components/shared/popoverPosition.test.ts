// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { describe, it, expect } from 'vitest';
import { computePositioningStyle } from './popoverPosition';

describe('computePositioningStyle — rounding', () => {
  it.each([
    {
      label: 'rounds down',
      top: 10.4,
      left: 10.4,
      width: 10.4,
      expected: 'top:10px;left:10px;width:10px;',
    },
    {
      label: 'rounds up',
      top: 10.6,
      left: 10.6,
      width: 10.6,
      expected: 'top:11px;left:11px;width:11px;',
    },
    {
      label: 'rounds half up',
      top: 10.5,
      left: 10.5,
      width: 10.5,
      expected: 'top:11px;left:11px;width:11px;',
    },
    { label: 'zero inputs', top: 0, left: 0, width: 0, expected: 'top:0px;left:0px;width:0px;' },
    {
      label: 'negative inputs',
      top: -5.6,
      left: -3.2,
      width: 100,
      expected: 'top:-6px;left:-3px;width:100px;',
    },
  ])('$label', ({ top, left, width, expected }) => {
    const result = computePositioningStyle(top, left, width, '0px');
    expect(result).toContain(expected);
  });
});

describe('computePositioningStyle — maxHeight verbatim', () => {
  it.each([
    {
      label: 'calc form (DateTimePicker)',
      maxHeight: 'calc(100vh - 24px)',
      expected: 'max-height:calc(100vh - 24px);',
    },
    {
      label: 'px form (TimeMenu)',
      maxHeight: '320px',
      expected: 'max-height:320px;',
    },
  ])('appends $label verbatim', ({ maxHeight, expected }) => {
    const result = computePositioningStyle(0, 0, 0, maxHeight);
    expect(result).toContain(expected);
  });
});

describe('computePositioningStyle — full-string equality', () => {
  it.each([
    {
      label: 'DateTimePicker call-site output (calc max-height)',
      top: 150,
      left: 20,
      width: 480,
      maxHeight: 'calc(100vh - 24px)',
      expected: 'top:150px;left:20px;width:480px;max-height:calc(100vh - 24px);',
    },
    {
      label: 'TimeMenu call-site output (px max-height)',
      top: 200,
      left: 100,
      width: 220,
      maxHeight: '320px',
      expected: 'top:200px;left:100px;width:220px;max-height:320px;',
    },
  ])('$label', ({ top, left, width, maxHeight, expected }) => {
    expect(computePositioningStyle(top, left, width, maxHeight)).toBe(expected);
  });
});
