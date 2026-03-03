// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

/** While `isOpen()` is true, calls `reposition()` on window resize / document scroll. */
export function repositionOnViewportChange(isOpen: () => boolean, reposition: () => void): void {
  $effect(() => {
    if (!isOpen()) return;

    const handleViewportChange = () => {
      reposition();
    };

    window.addEventListener('resize', handleViewportChange);
    document.addEventListener('scroll', handleViewportChange, true);

    return () => {
      window.removeEventListener('resize', handleViewportChange);
      document.removeEventListener('scroll', handleViewportChange, true);
    };
  });
}
