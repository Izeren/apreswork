// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// Shared in-memory Storage doubles for the pure preference load/save unit tests
// (weekStartPref, quickDateAnchorPref, taskListPrefs). Passed as the `storage`
// argument to the pref functions — no global stubbing involved.

/** Fresh in-memory Storage-ish double (getItem/setItem backed by a Map). */
export function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (key: string): string | null => map.get(key) ?? null,
    setItem: (key: string, value: string): void => void map.set(key, value),
  };
}

export const throwingReadStorage = {
  getItem: (_key: string): string | null => {
    throw new Error('denied');
  },
  setItem: (_key: string, _value: string): void => {},
};

export const throwingWriteStorage = {
  getItem: (_key: string): string | null => null,
  setItem: (_key: string, _value: string): void => {
    throw new Error('quota');
  },
};
