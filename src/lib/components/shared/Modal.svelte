<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { focusFirst, handleTabTrap } from './focusTrap';

  /** Minimum pixels of the header that must stay on-screen when dragging. */
  const MIN_VISIBLE_PX = 40;
  const MIN_RESIZE_WIDTH = 320;
  const MIN_RESIZE_HEIGHT = 200;

  interface Props {
    open: boolean;
    title: string;
    role?: 'dialog' | 'alertdialog';
    size?: 'md' | 'lg';
    closeOnBackdrop?: boolean;
    movable?: boolean;
    resizable?: boolean;
    /** Called on backdrop click AND Escape key — the consumer's "soft dismiss" path. */
    onbackdropclick?: () => void;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
  }

  const {
    open,
    title,
    role = 'dialog',
    size = 'md',
    closeOnBackdrop = true,
    movable = false,
    resizable = false,
    onbackdropclick,
    onclose,
    children,
    footer,
  }: Props = $props();

  let dialogEl: HTMLElement | null = $state(null);
  let previousFocus: HTMLElement | null = null;
  let mouseDownTarget: EventTarget | null = null;
  const uid = $props.id();
  const titleId = `modal-title-${uid}`;

  interface Position {
    top: number;
    left: number;
  }

  let position: Position | null = $state(null);
  let modalWidth: number | null = $state(null);
  let modalHeight: number | null = $state(null);
  let dragging = $state(false);
  let resizing = $state(false);

  /** Absolute start coords of the pointer at drag start. */
  let dragStartX = 0;
  let dragStartY = 0;

  /** Modal rect captured at drag start. */
  let dragStartRect: { top: number; left: number } = { top: 0, left: 0 };

  /** Resize start coords and initial size. */
  let resizeStartX = 0;
  let resizeStartY = 0;
  let resizeStartWidth = 0;
  let resizeStartHeight = 0;
  let resizeMaxHeight = 0;
  $effect(() => {
    if (open) {
      previousFocus = document.activeElement as HTMLElement | null;
      // Use a microtask so the DOM is mounted before we try to focus
      queueMicrotask(() => {
        if (dialogEl) focusFirst(dialogEl);
      });
      return () => {
        if (previousFocus) {
          previousFocus.focus();
          previousFocus = null;
        }
      };
    }
  });

  // Reset position and size each time the modal opens so it appears centered.
  $effect(() => {
    if (open) {
      position = null;
      modalWidth = null;
      modalHeight = null;
    }
  });

  function handleOverlayMouseDown(event: MouseEvent) {
    mouseDownTarget = event.target;
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target !== event.currentTarget) return;
    // Ignore drag-release: mousedown started inside the modal, released on backdrop
    if (mouseDownTarget !== event.currentTarget) return;

    if (onbackdropclick) {
      onbackdropclick();
    } else if (closeOnBackdrop) {
      onclose();
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      // Innermost dialog wins: modals can nest (task editor inside the status
      // modal), and without this the same Escape would close every layer.
      event.stopPropagation();
      if (onbackdropclick) {
        onbackdropclick();
      } else {
        // Note: closeOnBackdrop does not guard Escape (pre-existing asymmetry; no
        // current consumer sets closeOnBackdrop={false}, so this is theoretical).
        onclose();
      }
      return;
    }
    if (dialogEl) {
      handleTabTrap(event, dialogEl);
    }
  }

  function releasePointerCapture(event: PointerEvent) {
    try {
      (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
    } catch {
      // jsdom may not implement releasePointerCapture
    }
  }

  // Drag handlers — only active when movable=true

  function handleHeaderPointerDown(event: PointerEvent) {
    if (!movable) return;
    if (event.button !== 0) return;
    // Do not hijack clicks on the close button or its children
    if ((event.target as HTMLElement).closest('.close-btn')) return;

    event.preventDefault();

    const header = event.currentTarget as HTMLElement;
    header.setPointerCapture(event.pointerId);

    dragStartX = event.clientX;
    dragStartY = event.clientY;

    if (dialogEl) {
      const rect = dialogEl.getBoundingClientRect();
      dragStartRect = { top: rect.top, left: rect.left };
      // Snap to fixed and preserve current size so dragging doesn't reset a user resize
      if (!position) {
        position = { top: rect.top, left: rect.left };
      }
      if (resizable) {
        if (modalWidth == null) modalWidth = rect.width;
        if (modalHeight == null) modalHeight = rect.height;
      }
    }

    dragging = true;
  }

  function handleHeaderPointerMove(event: PointerEvent) {
    if (!dragging) return;

    const dx = event.clientX - dragStartX;
    const dy = event.clientY - dragStartY;

    let newLeft = dragStartRect.left + dx;
    let newTop = dragStartRect.top + dy;

    newLeft = Math.max(0, Math.min(newLeft, window.innerWidth - MIN_VISIBLE_PX));
    newTop = Math.max(0, Math.min(newTop, window.innerHeight - MIN_VISIBLE_PX));

    position = { top: newTop, left: newLeft };
  }

  function handleHeaderPointerUp(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    releasePointerCapture(event);
  }

  // Resize handlers — only active when resizable=true

  function handleResizePointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();

    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);

    resizeStartX = event.clientX;
    resizeStartY = event.clientY;

    if (dialogEl) {
      const rect = dialogEl.getBoundingClientRect();
      resizeStartWidth = rect.width;
      resizeStartHeight = rect.height;
      // Snapshot the content height once at resize start so the drag behavior
      // stays stable during this gesture. Account for any scroll overflow inside
      // the modal body (e.g. a manually resized textarea) so the modal can grow
      // to fit the full content. Never cap below the current dialog height.
      const bodyEl = dialogEl.querySelector('.modal-body');
      const bodyOverflow = bodyEl ? bodyEl.scrollHeight - bodyEl.clientHeight : 0;
      resizeMaxHeight = Math.max(rect.height, dialogEl.scrollHeight) + bodyOverflow;
      // Snap to fixed position so resize grows from bottom-right
      // instead of symmetrically from the flex center.
      if (!position) {
        position = { top: rect.top, left: rect.left };
      }
    }

    resizing = true;
  }

  function handleResizePointerMove(event: PointerEvent) {
    if (!resizing) return;

    const dx = event.clientX - resizeStartX;
    const dy = event.clientY - resizeStartY;
    const pos = position;
    const maxW = pos ? window.innerWidth - pos.left : window.innerWidth;
    const maxH = pos ? window.innerHeight - pos.top : window.innerHeight;

    modalWidth = Math.min(maxW, Math.max(MIN_RESIZE_WIDTH, resizeStartWidth + dx));

    modalHeight = Math.min(
      resizeMaxHeight,
      maxH,
      Math.max(MIN_RESIZE_HEIGHT, resizeStartHeight + dy),
    );
  }

  function handleResizePointerUp(event: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    releasePointerCapture(event);
  }

  // $derived.by is needed because $derived(ternary) cannot narrow $state unions.
  const positionStyles = $derived.by(() => {
    const p = position;
    return {
      position: p ? ('fixed' as const) : undefined,
      top: p ? `${p.top}px` : undefined,
      left: p ? `${p.left}px` : undefined,
      margin: p ? '0' : undefined,
    };
  });
  const styleWidth = $derived(modalWidth != null ? `${modalWidth}px` : undefined);
  const styleHeight = $derived(modalHeight != null ? `${modalHeight}px` : undefined);
</script>

{#if open}
  <div
    class="overlay"
    role="presentation"
    onmousedown={handleOverlayMouseDown}
    onclick={handleBackdropClick}
    onkeydown={handleKeydown}
  >
    <div
      bind:this={dialogEl}
      class="modal"
      class:modal--lg={size === 'lg'}
      class:modal--resizable={resizable}
      style:position={positionStyles.position}
      style:top={positionStyles.top}
      style:left={positionStyles.left}
      style:margin={positionStyles.margin}
      style:width={styleWidth}
      style:height={styleHeight}
      {role}
      aria-modal="true"
      aria-labelledby={titleId}
      tabindex="-1"
    >
      <!-- role="presentation" suppresses the a11y lint for pointer handlers on a non-interactive element;
           the header is a drag zone only and contains no interactive semantics beyond the close button. -->
      <header
        class="modal-header"
        class:modal-header--movable={movable}
        class:modal-header--dragging={dragging}
        role="presentation"
        onpointerdown={handleHeaderPointerDown}
        onpointermove={handleHeaderPointerMove}
        onpointerup={handleHeaderPointerUp}
        onpointercancel={handleHeaderPointerUp}
      >
        <h2 id={titleId} class="modal-title">{title}</h2>
        <button class="close-btn" aria-label="Close dialog" onclick={onclose}>✕</button>
      </header>
      <div class="modal-body">
        {@render children()}
      </div>
      {#if footer}
        <div class="modal-footer">
          {@render footer()}
        </div>
      {/if}
      {#if resizable}
        <!-- Custom resize handle — native CSS resize: both gets clipped by border-radius
             and is unreliable when content overflows. -->
        <div
          class="resize-handle"
          role="presentation"
          onpointerdown={handleResizePointerDown}
          onpointermove={handleResizePointerMove}
          onpointerup={handleResizePointerUp}
          onpointercancel={handleResizePointerUp}
        ></div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    width: min(480px, calc(100vw - var(--spacing-8)));
    max-height: calc(100vh - var(--spacing-8));
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .modal--lg {
    width: min(760px, calc(100vw - var(--spacing-8)));
  }

  .modal--resizable {
    position: relative;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-4) var(--spacing-6);
    border-bottom: 1px solid var(--color-border-light);
    flex-shrink: 0;
  }

  .modal-header--movable {
    cursor: grab;
    user-select: none;
  }

  .modal-header--dragging {
    cursor: grabbing;
  }

  .modal-title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .close-btn {
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
  }

  .close-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .modal-body {
    padding: var(--spacing-6);
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    flex: 1;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-3);
    padding: var(--spacing-3) var(--spacing-6) var(--spacing-4);
    border-top: 1px solid var(--color-border-light);
    flex-shrink: 0;
  }

  .resize-handle {
    position: absolute;
    bottom: 0;
    right: 0;
    width: 20px;
    height: 20px;
    cursor: nwse-resize;
    z-index: 1;
    border-bottom-right-radius: var(--radius-lg);
  }

  .resize-handle::after {
    content: '';
    position: absolute;
    right: 4px;
    bottom: 4px;
    width: 0;
    height: 0;
    border-style: solid;
    border-width: 0 0 10px 10px;
    border-color: transparent transparent var(--color-border) transparent;
    opacity: 0.6;
  }
</style>
