// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

export type ToastLevel = 'success' | 'error' | 'info' | 'warning';

export interface ToastMessage {
  id: string;
  level: ToastLevel;
  text: string;
}

/** Default auto-dismiss delay for every level (ms). Pass autoMs=0 for a persistent toast. */
const DEFAULT_AUTO_MS = 3_000;

export class ToastState {
  items: ToastMessage[] = $state([]);
  #nextId = 0;

  push(level: ToastLevel, text: string, autoMs?: number): void {
    const id = String(this.#nextId++);
    this.items = [...this.items, { id, level, text }];

    const delay = autoMs ?? DEFAULT_AUTO_MS;
    if (delay > 0) {
      setTimeout(() => {
        this.dismiss(id);
      }, delay);
    }
  }

  success(text: string): void {
    this.push('success', text);
  }

  error(text: string): void {
    this.push('error', text);
  }

  info(text: string): void {
    this.push('info', text);
  }

  warn(text: string): void {
    this.push('warning', text);
  }

  dismiss(id: string): void {
    this.items = this.items.filter((t) => t.id !== id);
  }

  reset(): void {
    this.items = [];
  }
}

export const toastState = new ToastState();
