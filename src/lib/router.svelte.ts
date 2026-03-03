// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

const ROUTES = ['calendar', 'tasks', 'status', 'settings', 'profiles'] as const;
export type Route = (typeof ROUTES)[number];
const VALID_ROUTES: ReadonlySet<string> = new Set(ROUTES);

function parseHash(hash: string): Route {
  const stripped = hash.replace(/^#\/?/, '');
  return VALID_ROUTES.has(stripped) ? (stripped as Route) : 'calendar';
}

class Router {
  current: Route = $state(parseHash(window.location.hash));
  readonly #onHashChange: () => void;

  constructor() {
    this.#onHashChange = () => {
      this.current = parseHash(window.location.hash);
    };
    window.addEventListener('hashchange', this.#onHashChange);
  }

  navigate(route: Route): void {
    window.location.hash = `#/${route}`;
  }

  destroy(): void {
    window.removeEventListener('hashchange', this.#onHashChange);
  }
}

export { parseHash };
export const router = new Router();
