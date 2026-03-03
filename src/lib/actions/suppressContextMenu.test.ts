// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { shouldSuppressContextMenu } from './suppressContextMenu';

describe('shouldSuppressContextMenu', () => {
  it('suppresses for null target', () => {
    expect(shouldSuppressContextMenu(null)).toBe(true);
  });

  it.each([
    { tag: 'div', editable: false, label: 'plain div', expected: true },
    { tag: 'button', editable: false, label: 'button', expected: true },
    { tag: 'span', editable: false, label: 'span', expected: true },
    { tag: 'input', editable: false, label: 'input', expected: false },
    { tag: 'textarea', editable: false, label: 'textarea', expected: false },
    { tag: 'div', editable: true, label: 'contenteditable div', expected: false },
  ])('$label → suppress=$expected', ({ tag, editable, expected }) => {
    const el = document.createElement(tag);
    if (editable) el.setAttribute('contenteditable', 'true');
    expect(shouldSuppressContextMenu(el)).toBe(expected);
  });

  it.each([
    { parentEditable: false, label: 'plain div parent', expected: true },
    { parentEditable: true, label: 'contenteditable parent', expected: false },
  ])('child inside $label → suppress=$expected', ({ parentEditable, expected }) => {
    const outer = document.createElement('div');
    if (parentEditable) outer.setAttribute('contenteditable', 'true');
    const inner = document.createElement('span');
    outer.appendChild(inner);
    expect(shouldSuppressContextMenu(inner)).toBe(expected);
  });
});
