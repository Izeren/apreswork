// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  registerShortcuts,
  activeShortcuts,
  handleShortcutKeydown,
  resetShortcutsForTest,
} from './shortcuts.svelte';

function makeEvent(
  key: string,
  opts: {
    ctrlKey?: boolean;
    metaKey?: boolean;
    altKey?: boolean;
    shiftKey?: boolean;
  } = {},
): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key,
    ctrlKey: opts.ctrlKey ?? false,
    metaKey: opts.metaKey ?? false,
    altKey: opts.altKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    bubbles: true,
    cancelable: true,
  });
}

beforeEach(() => {
  resetShortcutsForTest();
});

describe('shortcuts registry — dispatch basics', () => {
  it('fires handler and marks event as default-prevented for a registered key', () => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    const event = makeEvent('n');
    handleShortcutKeydown(event);
    expect(handler).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it('does not fire for an unregistered key', () => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    handleShortcutKeydown(makeEvent('x'));
    expect(handler).not.toHaveBeenCalled();
  });

  it('unregister removes the binding so the handler is not called', () => {
    const handler = vi.fn();
    const unregister = registerShortcuts([
      { key: 'n', description: 'New', group: 'Global', handler },
    ]);
    unregister();
    handleShortcutKeydown(makeEvent('n'));
    expect(handler).not.toHaveBeenCalled();
  });

  it('activeShortcuts reflects currently registered bindings', () => {
    expect(activeShortcuts()).toHaveLength(0);
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler: vi.fn() }]);
    expect(activeShortcuts()).toHaveLength(1);
  });
});

describe('shortcuts registry — modifier keys', () => {
  it.each([
    ['ctrlKey', { ctrlKey: true }],
    ['metaKey', { metaKey: true }],
    ['altKey', { altKey: true }],
  ] as const)('ignores events with %s set', (_, opts) => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    handleShortcutKeydown(makeEvent('n', opts));
    expect(handler).not.toHaveBeenCalled();
  });

  it('fires when only shiftKey is true — ? arrives as Shift+/', () => {
    const handler = vi.fn();
    registerShortcuts([{ key: '?', description: 'Help', group: 'Global', handler }]);
    handleShortcutKeydown(makeEvent('?', { shiftKey: true }));
    expect(handler).toHaveBeenCalledTimes(1);
  });
});

describe('shortcuts registry — editable targets', () => {
  function expectKeydownIgnored(handler: ReturnType<typeof vi.fn>, el: HTMLElement) {
    document.body.appendChild(el);
    const event = new KeyboardEvent('keydown', { key: 'n', bubbles: true, cancelable: true });
    Object.defineProperty(event, 'target', { value: el, configurable: true });
    handleShortcutKeydown(event);

    expect(handler).not.toHaveBeenCalled();
    document.body.removeChild(el);
  }

  it.each([
    ['input', 'input'],
    ['textarea', 'textarea'],
    ['select', 'select'],
  ])('ignores keydown whose target is <%s>', (_, tagName) => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    expectKeydownIgnored(handler, document.createElement(tagName));
  });

  it('ignores keydown whose target is contenteditable — stubs isContentEditable since jsdom may not set it', () => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    const el = document.createElement('div');
    // jsdom's isContentEditable is typically false even with contenteditable attr; stub it
    Object.defineProperty(el, 'isContentEditable', { value: true, configurable: true });
    expectKeydownIgnored(handler, el);
  });
});

describe('shortcuts registry — modal/menu suppression', () => {
  it.each(['dialog', 'alertdialog', 'menu', 'listbox'])(
    'suppresses when [role="%s"] is in the DOM',
    (role) => {
      const handler = vi.fn();
      registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
      const el = document.createElement('div');
      el.setAttribute('role', role);
      document.body.appendChild(el);

      handleShortcutKeydown(makeEvent('n'));

      expect(handler).not.toHaveBeenCalled();
      document.body.removeChild(el);
    },
  );

  it('ignores events that are already default-prevented before reaching the dispatcher', () => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    const event = new KeyboardEvent('keydown', { key: 'n', bubbles: true, cancelable: true });
    event.preventDefault();
    handleShortcutKeydown(event);
    expect(handler).not.toHaveBeenCalled();
  });
});

describe('shortcuts registry — last registration wins for duplicate keys', () => {
  it('the most recently registered binding for a key is dispatched', () => {
    const first = vi.fn();
    const second = vi.fn();
    registerShortcuts([{ key: 'n', description: 'First', group: 'Global', handler: first }]);
    registerShortcuts([{ key: 'n', description: 'Second', group: 'Global', handler: second }]);
    handleShortcutKeydown(makeEvent('n'));
    expect(second).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
  });

  it('unregistering the later binding falls back to the earlier one', () => {
    const first = vi.fn();
    const second = vi.fn();
    registerShortcuts([{ key: 'n', description: 'First', group: 'Global', handler: first }]);
    const unreg = registerShortcuts([
      { key: 'n', description: 'Second', group: 'Global', handler: second },
    ]);
    unreg();
    handleShortcutKeydown(makeEvent('n'));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
  });
});

describe('shortcuts registry — reset', () => {
  it('resetShortcutsForTest clears all bindings', () => {
    const handler = vi.fn();
    registerShortcuts([{ key: 'n', description: 'New', group: 'Global', handler }]);
    resetShortcutsForTest();
    handleShortcutKeydown(makeEvent('n'));
    expect(handler).not.toHaveBeenCalled();
    expect(activeShortcuts()).toHaveLength(0);
  });
});
