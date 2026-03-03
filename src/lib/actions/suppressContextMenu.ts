// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

export function shouldSuppressContextMenu(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return true;
  return target.closest('input, textarea, [contenteditable]') === null;
}
