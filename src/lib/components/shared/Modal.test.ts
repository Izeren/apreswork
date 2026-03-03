// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup, fireEvent, act } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import type { ComponentProps } from 'svelte';
import Modal from './Modal.svelte';
import ConfirmDialog from './ConfirmDialog.svelte';

afterEach(() => {
  cleanup();
});

const childrenSnippet = createRawSnippet(() => ({
  render: () => '<p>Modal content</p>',
}));

const footerSnippet = createRawSnippet(() => ({
  render: () => '<button type="button">Footer action</button>',
}));

function renderModal(props: Partial<ComponentProps<typeof Modal>> = {}) {
  return render(Modal, {
    open: true,
    title: 'Test',
    onclose: vi.fn(),
    children: childrenSnippet,
    ...props,
  });
}

function renderConfirm(props: Partial<ComponentProps<typeof ConfirmDialog>> = {}) {
  return render(ConfirmDialog, {
    open: true,
    title: 'Confirm',
    message: 'Are you sure?',
    onconfirm: vi.fn(),
    oncancel: vi.fn(),
    ...props,
  });
}

/**
 * Render an open Modal and return its overlay (the parent of the dialog
 * panel) together with the onclose spy.
 */
function renderOverlay() {
  const onclose = vi.fn();
  const { getByRole } = renderModal({ onclose });
  return { overlay: getByRole('dialog').parentElement!, onclose, getByRole };
}

/**
 * Stubs setPointerCapture / releasePointerCapture on an element.
 * jsdom does not implement these pointer-capture APIs.
 */
function stubPointerCapture(el: HTMLElement) {
  Object.assign(el, {
    setPointerCapture: vi.fn(),
    releasePointerCapture: vi.fn(),
  });
}

function getModalParts(container: HTMLElement) {
  const modal = container.querySelector('[role="dialog"]') as HTMLElement;
  const header = modal.querySelector('.modal-header') as HTMLElement;
  const resizeHandle = modal.querySelector('.resize-handle') as HTMLElement | null;
  return { modal, header, resizeHandle };
}

type FireFn = (typeof fireEvent)['pointerDown'];
// Factory for primary-button pointer events with a stable pointerId.
function pointerOp(fn: FireFn, extra?: object) {
  return (el: HTMLElement, clientX: number, clientY: number) =>
    fn(el, { ...extra, clientX, clientY, pointerId: 1 });
}
const pointerDown = pointerOp(fireEvent.pointerDown, { button: 0 });
const pointerMove = pointerOp(fireEvent.pointerMove);
const pointerUp = pointerOp(fireEvent.pointerUp);

const BASE_RECT = {
  top: 100,
  left: 100,
  width: 500,
  height: 400,
  bottom: 500,
  right: 600,
} as DOMRect;

/**
 * Render a resizable modal with pointer capture stubbed on the resize handle,
 * a fixed bounding rect, and an optional fixed scrollHeight.
 */
function setupResize(opts: { rect?: DOMRect; scrollHeight?: number } = {}) {
  const { container } = renderModal({ resizable: true });
  const { modal, resizeHandle } = getModalParts(container);
  stubPointerCapture(resizeHandle!);
  modal.getBoundingClientRect = () => opts.rect ?? BASE_RECT;
  if (opts.scrollHeight !== undefined) {
    Object.defineProperty(modal, 'scrollHeight', { value: opts.scrollHeight, configurable: true });
  }
  return { modal, resizeHandle: resizeHandle! };
}

/** Render a modal (movable by default) and stub pointer capture on its header. */
function setupDrag(props: Partial<ComponentProps<typeof Modal>> = {}) {
  const { container, rerender } = renderModal({ movable: true, ...props });
  const { modal, header } = getModalParts(container);
  stubPointerCapture(header);
  return { container, rerender, modal, header };
}

function stylePosition(modal: HTMLElement): { left: number; top: number } {
  const style = modal.getAttribute('style') ?? '';
  const left = /left:\s*([\d.]+)px/.exec(style);
  const top = /top:\s*([\d.]+)px/.exec(style);
  if (!left || !top) throw new Error(`stylePosition: could not parse style="${style}"`);
  return { left: Number(left[1]), top: Number(top[1]) };
}

describe('Modal — visibility', () => {
  it('renders content when open=true', () => {
    const { getByRole } = renderModal({ title: 'Test Modal' });

    expect(getByRole('dialog')).toBeTruthy();
    expect(getByRole('heading', { name: 'Test Modal' })).toBeTruthy();
  });

  it('does not render dialog when open=false', () => {
    const { queryByRole } = renderModal({ open: false });

    expect(queryByRole('dialog')).toBeNull();
  });
});

describe('Modal — role prop', () => {
  it.each([
    { props: {} as Partial<ComponentProps<typeof Modal>>, expected: 'dialog' as const },
    { props: { role: 'alertdialog' as const }, expected: 'alertdialog' as const },
  ])('uses role="$expected"', ({ props, expected }) => {
    const { getByRole } = renderModal(props);
    expect(getByRole(expected)).toBeTruthy();
  });
});

describe('Modal — size', () => {
  it('applies the large size class when size=lg', () => {
    const { getByRole } = renderModal({ size: 'lg' });

    expect(getByRole('dialog').classList.contains('modal--lg')).toBe(true);
  });
});

describe('Modal — footer slot', () => {
  it('renders footer content outside the scrollable body', () => {
    const { container, getByRole } = renderModal({ footer: footerSnippet });

    const action = getByRole('button', { name: 'Footer action' });
    const modalBody = container.querySelector('.modal-body');
    const modalFooter = container.querySelector('.modal-footer');

    expect(modalBody).toBeTruthy();
    expect(modalFooter).toBeTruthy();
    expect(modalBody?.contains(action)).toBe(false);
    expect(modalFooter?.contains(action)).toBe(true);
  });
});

describe('Modal — keyboard', () => {
  it('calls onclose when Escape key is pressed on the overlay', async () => {
    const { overlay, onclose } = renderOverlay();

    await fireEvent.keyDown(overlay, { key: 'Escape' });

    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('calls onbackdropclick (not onclose) when Escape is pressed and onbackdropclick is set', async () => {
    const onclose = vi.fn();
    const onbackdropclick = vi.fn();
    const { getByRole } = renderModal({ onclose, onbackdropclick });

    await fireEvent.keyDown(getByRole('dialog').parentElement!, { key: 'Escape' });

    expect(onbackdropclick).toHaveBeenCalledTimes(1);
    expect(onclose).not.toHaveBeenCalled();
  });

  it('does not call onclose for non-Escape keys', async () => {
    const { overlay, onclose } = renderOverlay();

    await fireEvent.keyDown(overlay, { key: 'Enter' });

    expect(onclose).not.toHaveBeenCalled();
  });
});

describe('Modal — backdrop click', () => {
  it('calls onclose when clicking directly on the overlay (backdrop)', async () => {
    const { overlay, onclose } = renderOverlay();

    await fireEvent.mouseDown(overlay);
    await fireEvent.click(overlay);

    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('does not call onclose when clicking inside the modal panel', async () => {
    const { onclose, getByRole } = renderOverlay();

    await fireEvent.click(getByRole('dialog'));

    expect(onclose).not.toHaveBeenCalled();
  });
});

describe('Modal — close button', () => {
  it('calls onclose when the close button is clicked', async () => {
    const { onclose, getByRole } = renderOverlay();

    await fireEvent.click(getByRole('button', { name: /close dialog/i }));

    expect(onclose).toHaveBeenCalledTimes(1);
  });
});

describe('Modal — focus management', () => {
  it('moves focus into the modal on open', async () => {
    const { getByRole } = renderModal();

    // Allow microtask queue to flush
    await act(() => Promise.resolve());

    const dialog = getByRole('dialog');
    // Focus should be within the modal (either dialog itself or a child)
    const focusInModal =
      dialog.contains(document.activeElement) || document.activeElement === dialog;
    expect(focusInModal).toBe(true);
  });
});

describe('ConfirmDialog — button clicks', () => {
  it.each([
    { clicked: 'confirm', other: 'cancel' },
    { clicked: 'cancel', other: 'confirm' },
  ] as const)(
    'calls on$clicked when the $clicked button is clicked',
    async ({ clicked, other }) => {
      const handlers = { onconfirm: vi.fn(), oncancel: vi.fn() };
      const { getByRole } = renderConfirm(handlers);

      await fireEvent.click(getByRole('button', { name: new RegExp(clicked, 'i') }));

      expect(handlers[`on${clicked}`]).toHaveBeenCalledTimes(1);
      expect(handlers[`on${other}`]).not.toHaveBeenCalled();
    },
  );
});

describe('ConfirmDialog — destructive mode', () => {
  it.each([
    { destructive: true, applied: 'btn-danger', absent: 'btn-primary' },
    { destructive: false, applied: 'btn-primary', absent: 'btn-danger' },
  ])(
    'applies $applied to the confirm button when destructive=$destructive',
    ({ destructive, applied, absent }) => {
      const { getByRole } = renderConfirm({ destructive });

      const confirmBtn = getByRole('button', { name: /confirm/i });
      expect(confirmBtn.classList.contains(applied)).toBe(true);
      expect(confirmBtn.classList.contains(absent)).toBe(false);
    },
  );
});

describe('ConfirmDialog — labels', () => {
  it.each<{
    props: Partial<ComponentProps<typeof ConfirmDialog>>;
    expectedConfirm: string;
    expectedCancel: string;
  }>([
    { props: {}, expectedConfirm: 'Confirm', expectedCancel: 'Cancel' },
    {
      props: { confirmLabel: 'Delete', cancelLabel: 'Keep' },
      expectedConfirm: 'Delete',
      expectedCancel: 'Keep',
    },
  ])(
    'renders confirm=$expectedConfirm, cancel=$expectedCancel',
    ({ props, expectedConfirm, expectedCancel }) => {
      const { getByRole } = renderConfirm(props);

      expect(getByRole('button', { name: expectedConfirm })).toBeTruthy();
      expect(getByRole('button', { name: expectedCancel })).toBeTruthy();
    },
  );
});

describe('ConfirmDialog — ARIA role', () => {
  it('renders with role="alertdialog"', () => {
    const { getByRole } = renderConfirm();

    expect(getByRole('alertdialog')).toBeTruthy();
  });
});

describe('Modal — resizable prop', () => {
  it.each([
    { label: 'false (default)', resizable: false, expectHandle: false },
    { label: 'true', resizable: true, expectHandle: true },
  ])('resizable=$label: resize handle present=$expectHandle', ({ resizable, expectHandle }) => {
    const { container } = renderModal({ resizable });
    expect(getModalParts(container).resizeHandle !== null).toBe(expectHandle);
  });

  it('resize handle drag changes modal width/height', async () => {
    const { modal, resizeHandle } = setupResize({ scrollHeight: 800 });

    await pointerDown(resizeHandle, 600, 500);
    await pointerMove(resizeHandle, 700, 600);

    const style = modal.getAttribute('style') ?? '';
    expect(style).toContain('width: 600px');
    expect(style).toContain('height: 500px');

    await pointerUp(resizeHandle, 700, 600);
  });

  it('resize height uses the content height captured at resize start', async () => {
    const { modal, resizeHandle } = setupResize();
    let currentScrollHeight = 450;
    Object.defineProperty(modal, 'scrollHeight', {
      get: () => currentScrollHeight,
      configurable: true,
    });

    await pointerDown(resizeHandle, 600, 500);
    currentScrollHeight = 620;
    // Even if content changes mid-drag, the clamp stays based on the
    // resize-start snapshot for stable pointer behavior.
    await pointerMove(resizeHandle, 600, 900);

    expect(modal.getAttribute('style') ?? '').toContain('height: 450px');
  });

  it('dragging downward does not shrink when content is shorter than the starting dialog height', async () => {
    const { modal, resizeHandle } = setupResize({ scrollHeight: 350 });

    await pointerDown(resizeHandle, 600, 500);
    await pointerMove(resizeHandle, 600, 550);

    expect(modal.getAttribute('style') ?? '').toContain('height: 400px');
  });

  it('resize clamps to minimum dimensions', async () => {
    const { modal, resizeHandle } = setupResize({ scrollHeight: 800 });

    await pointerDown(resizeHandle, 600, 500);
    // Drag far to the top-left to shrink below minimum
    await pointerMove(resizeHandle, 0, 0);

    const style = modal.getAttribute('style') ?? '';
    // MIN_RESIZE_WIDTH=320, MIN_RESIZE_HEIGHT=200
    expect(style).toContain('width: 320px');
    expect(style).toContain('height: 200px');
  });

  it('resize snaps modal to fixed position', async () => {
    const { modal, resizeHandle } = setupResize({
      rect: { top: 200, left: 300, width: 500, height: 400, bottom: 600, right: 800 } as DOMRect,
    });

    expect(modal.getAttribute('style') ?? '').not.toContain('position: fixed');

    await pointerDown(resizeHandle, 800, 600);

    const style = modal.getAttribute('style') ?? '';
    expect(style).toContain('position: fixed');
    expect(style).toContain('top: 200px');
    expect(style).toContain('left: 300px');
  });

  it('pointercancel clears resizing state', async () => {
    const { modal, resizeHandle } = setupResize({ scrollHeight: 800 });

    await pointerDown(resizeHandle, 600, 500);
    await fireEvent.pointerCancel(resizeHandle, { pointerId: 1 });

    // Should be able to resize again — no stuck state
    await pointerDown(resizeHandle, 600, 500);
    await pointerMove(resizeHandle, 650, 550);

    expect(modal.getAttribute('style') ?? '').toContain('width: 550px');
  });
});

describe('Modal — movable=false (default)', () => {
  it('pointerdown on header does not add position style', async () => {
    const { modal, header } = setupDrag({ movable: false });

    await pointerDown(header, 100, 100);

    expect(modal.getAttribute('style') ?? '').not.toContain('position: fixed');
  });
});

describe('Modal — movable=true drag behavior', () => {
  it('full drag sequence applies fixed position style', async () => {
    const { modal, header } = setupDrag();

    await pointerDown(header, 100, 100);
    await pointerMove(header, 150, 120);

    const style = modal.getAttribute('style') ?? '';
    expect(style).toContain('position: fixed');
    expect(style).toContain('margin: 0');

    await pointerUp(header, 150, 120);

    // After pointerup the position is retained but dragging class is removed
    expect(modal.classList.contains('modal-header--dragging')).toBe(false);
  });

  it('close button pointerdown does not start drag', async () => {
    const { modal, header } = setupDrag();
    const closeBtn = header.querySelector('.close-btn') as HTMLElement;
    stubPointerCapture(closeBtn);

    await pointerDown(closeBtn, 50, 50);
    await pointerMove(header, 200, 200);

    expect(modal.getAttribute('style') ?? '').not.toContain('position: fixed');
  });

  it('close button click still invokes onclose when movable=true', async () => {
    const onclose = vi.fn();
    const { header } = setupDrag({ onclose });
    const closeBtn = header.querySelector('.close-btn') as HTMLElement;

    await fireEvent.click(closeBtn);
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it.each([
    { button: 2, label: 'right-click (button=2)' },
    { button: 1, label: 'middle-click (button=1)' },
  ])('non-primary button ($label) does not start drag', async ({ button }) => {
    const { modal, header } = setupDrag();

    await fireEvent.pointerDown(header, { button, clientX: 100, clientY: 100, pointerId: 1 });
    await pointerMove(header, 200, 200);

    expect(modal.getAttribute('style') ?? '').not.toContain('position: fixed');
  });

  it.each([
    { label: 'right/bottom (max)', startX: 50, startY: 50, dragX: 99999, dragY: 99999 },
    { label: 'left/top (min)', startX: 500, startY: 400, dragX: -99999, dragY: -99999 },
  ])('dragging past $label edges clamps position', async ({ startX, startY, dragX, dragY }) => {
    const { modal, header } = setupDrag();
    await pointerDown(header, startX, startY);
    await pointerMove(header, dragX, dragY);
    const { left, top } = stylePosition(modal);
    expect(left).toBeGreaterThanOrEqual(0);
    expect(left).toBeLessThanOrEqual(window.innerWidth - 40);
    expect(top).toBeGreaterThanOrEqual(0);
    expect(top).toBeLessThanOrEqual(window.innerHeight - 40);
  });

  it('position is reset to null when modal is reopened', async () => {
    const { container, rerender, modal, header } = setupDrag();

    await pointerDown(header, 100, 100);
    await pointerMove(header, 200, 150);
    await pointerUp(header, 200, 150);

    expect(modal.getAttribute('style') ?? '').toContain('position: fixed');

    // Close then reopen
    await rerender({ open: false });
    await rerender({ open: true });

    const { modal: newModal } = getModalParts(container);
    expect(newModal.getAttribute('style') ?? '').not.toContain('position: fixed');
  });

  it('pointercancel clears dragging state', async () => {
    const { header } = setupDrag();

    await pointerDown(header, 100, 100);
    expect(header.classList.contains('modal-header--dragging')).toBe(true);

    await fireEvent.pointerCancel(header, { pointerId: 1 });
    expect(header.classList.contains('modal-header--dragging')).toBe(false);
  });
});
