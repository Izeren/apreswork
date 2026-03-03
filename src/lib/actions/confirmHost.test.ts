// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { createConfirmHost } from './confirmHost.svelte';
import type { ConfirmSpec } from './taskActions';

function makeSpec(overrides: Partial<ConfirmSpec> = {}): ConfirmSpec {
  return {
    title: 'Delete item',
    message: 'Are you sure?',
    confirmLabel: 'Delete',
    destructive: true,
    ...overrides,
  };
}

describe('createConfirmHost', () => {
  it('initial spec is null', () => {
    const host = createConfirmHost();
    expect(host.spec).toBeNull();
  });

  it('request() sets spec to the given value', () => {
    const host = createConfirmHost();
    const spec = makeSpec({ title: 'Remove task', message: 'Permanent.', confirmLabel: 'Remove' });
    void host.request(spec);
    expect(host.spec).toEqual(spec);
  });

  it('request() returns a promise that is still pending before settle', async () => {
    const host = createConfirmHost();
    const promise = host.request(makeSpec());
    let resolved = false;
    void promise.then(() => {
      resolved = true;
    });
    // Flush microtask queue — promise must still be pending
    await Promise.resolve();
    expect(resolved).toBe(false);
  });

  it.each([{ value: true }, { value: false }])(
    'settle($value) resolves the promise',
    async ({ value }) => {
      const host = createConfirmHost();
      const promise = host.request(makeSpec());
      host.settle(value);
      await expect(promise).resolves.toBe(value);
    },
  );

  it.each([{ value: true }, { value: false }])(
    'settle($value) resets spec to null',
    ({ value }) => {
      const host = createConfirmHost();
      void host.request(makeSpec());
      host.settle(value);
      expect(host.spec).toBeNull();
    },
  );

  it.each([true, false])('settle(%s) when null spec does not throw', (value) => {
    const host = createConfirmHost();
    // No prior request — spec is null
    expect(() => host.settle(value)).not.toThrow();
  });
});
