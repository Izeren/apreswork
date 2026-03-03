// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { getFocusableElements, handleTabTrap, focusFirst } from './focusTrap';

function makeContainer(html: string): HTMLElement {
  const div = document.createElement('div');
  div.innerHTML = html;
  document.body.appendChild(div);
  return div;
}

function cleanup(el: HTMLElement) {
  document.body.removeChild(el);
}

function makeTabEvent(shiftKey = false): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: 'Tab', shiftKey, bubbles: true, cancelable: true });
}

describe('getFocusableElements', () => {
  it('returns buttons and links inside a container', () => {
    const container = makeContainer(`
      <button>A</button>
      <a href="#">B</a>
      <button disabled>C</button>
    `);
    const result = getFocusableElements(container);
    expect(result).toHaveLength(2);
    cleanup(container);
  });

  it('returns empty array when no focusable elements', () => {
    const container = makeContainer('<p>No interactive</p>');
    expect(getFocusableElements(container)).toHaveLength(0);
    cleanup(container);
  });

  it('excludes elements with tabindex="-1"', () => {
    const container = makeContainer('<button tabindex="-1">Skip</button><button>Keep</button>');
    const result = getFocusableElements(container);
    expect(result).toHaveLength(1);
    expect(result[0].textContent).toBe('Keep');
    cleanup(container);
  });

  it('includes elements with positive tabindex', () => {
    const container = makeContainer('<span tabindex="0">Span</span><button>Btn</button>');
    const result = getFocusableElements(container);
    expect(result).toHaveLength(2);
    cleanup(container);
  });

  it('includes input, select, and textarea elements', () => {
    const container = makeContainer(`
      <input type="text" />
      <select><option>A</option></select>
      <textarea></textarea>
    `);
    const result = getFocusableElements(container);
    expect(result).toHaveLength(3);
    cleanup(container);
  });

  it('excludes disabled input', () => {
    const container = makeContainer('<input disabled /><input />');
    const result = getFocusableElements(container);
    expect(result).toHaveLength(1);
    cleanup(container);
  });
});

describe('handleTabTrap', () => {
  let container: HTMLElement;

  beforeEach(() => {
    container = makeContainer(`
      <button id="first">First</button>
      <button id="second">Second</button>
      <button id="last">Last</button>
    `);
  });

  it('returns false for non-Tab keys', () => {
    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    expect(handleTabTrap(event, container)).toBe(false);
    cleanup(container);
  });

  it('returns false when container has no focusable elements', () => {
    const empty = makeContainer('<p>text</p>');
    const event = makeTabEvent();
    expect(handleTabTrap(event, empty)).toBe(false);
    cleanup(container);
    cleanup(empty);
  });

  function expectTabWraps(elementToFocus: HTMLElement, shiftKey: boolean) {
    elementToFocus.focus();

    const event = makeTabEvent(shiftKey);
    const focusSpy = vi.spyOn(HTMLElement.prototype, 'focus');
    const handled = handleTabTrap(event, container);

    expect(handled).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(focusSpy).toHaveBeenCalled();
    focusSpy.mockRestore();
    cleanup(container);
  }

  it.each([
    {
      label: 'Tab from last element wraps to first',
      getEl: (buttons: NodeListOf<Element>) => buttons[buttons.length - 1] as HTMLElement,
      shiftKey: false,
    },
    {
      label: 'Shift+Tab from first element wraps to last',
      getEl: (buttons: NodeListOf<Element>) => buttons[0] as HTMLElement,
      shiftKey: true,
    },
  ])('$label', ({ getEl, shiftKey }) => {
    const buttons = container.querySelectorAll('button');
    expectTabWraps(getEl(buttons), shiftKey);
  });

  it.each([{ shiftKey: false }, { shiftKey: true }])(
    'middle element (shiftKey=$shiftKey): does not prevent default or trap focus',
    ({ shiftKey }) => {
      const buttons = container.querySelectorAll('button');
      const middle = buttons[1] as HTMLElement;
      middle.focus();

      const event = makeTabEvent(shiftKey);
      const handled = handleTabTrap(event, container);

      expect(handled).toBe(false);
      expect(event.defaultPrevented).toBe(false);
      cleanup(container);
    },
  );

  it.each([
    [false, 'Tab'],
    [true, 'Shift+Tab'],
  ])('wraps correctly with a single focusable element — %s', (shiftKey) => {
    const single = makeContainer('<button id="only">Only</button>');
    const btn = single.querySelector<HTMLElement>('#only')!;
    btn.focus();

    const event = makeTabEvent(shiftKey);
    const focusSpy = vi.spyOn(HTMLElement.prototype, 'focus');
    const handled = handleTabTrap(event, single);

    // When there's only one element, first === last, so wrapping occurs
    expect(handled).toBe(true);
    expect(focusSpy).toHaveBeenCalled();
    focusSpy.mockRestore();
    cleanup(single);
  });
});

describe('focusFirst', () => {
  it('focuses the first focusable element in the container', () => {
    const container = makeContainer('<button id="first">A</button><button id="second">B</button>');
    // Spy on the prototype so jsdom prototype-inherited method is interceptable.
    // We track call count; document.activeElement is not reliable when prototype is mocked.
    const spy = vi.spyOn(HTMLElement.prototype, 'focus');
    focusFirst(container);
    expect(spy).toHaveBeenCalledTimes(1);
    spy.mockRestore();
    cleanup(container);
  });

  it('focuses the container itself when no focusable children and container has tabIndex >= 0', () => {
    const container = makeContainer('<p>No focusable</p>');
    container.tabIndex = 0;
    const spy = vi.spyOn(HTMLElement.prototype, 'focus');
    focusFirst(container);
    expect(spy).toHaveBeenCalledTimes(1);
    spy.mockRestore();
    cleanup(container);
  });

  it('does not throw when no focusable children and container is not focusable', () => {
    const container = makeContainer('<p>Nothing</p>');
    expect(() => focusFirst(container)).not.toThrow();
    cleanup(container);
  });
});
