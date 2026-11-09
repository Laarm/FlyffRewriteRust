// The current route (Plan/25 section 2.3): a store over `location.hash`. `navigate` pushes (or replaces) the hash and updates the
// store at once; back/forward and edits of the address bar arrive as `popstate` / `hashchange`. Pure parsing is `app/route.ts`.
import { create } from 'zustand';
import { formatHash, parseHash, sameRoute, type Route } from '../app/route';

interface RouteState {
  readonly route: Route | null;
}

function current(): Route | null {
  return typeof location === 'undefined' ? null : parseHash(location.hash);
}

export const useRouteStore = create<RouteState>(() => ({ route: current() }));

/** The route now (outside React). */
export function getRoute(): Route | null {
  return useRouteStore.getState().route;
}

/** Reads `location.hash` into the store (listeners and tests). */
export function syncRouteFromLocation(): void {
  const next = current();
  if (!sameRoute(next, useRouteStore.getState().route)) useRouteStore.setState({ route: next });
}

/** Opens a route (`null`: the start page, no hash): a new history entry, or `replace` for redirects and in-place state (a drawer's width). */
export function navigate(route: Route | null, options: { readonly replace?: boolean } = {}): void {
  if (sameRoute(route, useRouteStore.getState().route)) return;
  const hash = route === null ? `${location.pathname}${location.search}` : formatHash(route);
  try {
    if (options.replace === true) history.replaceState(null, '', hash);
    else history.pushState(null, '', hash);
  } catch {
    location.hash = hash;
  }
  useRouteStore.setState({ route });
}

/** Installs the `popstate` / `hashchange` listeners; returns the remover. */
export function startRouteListener(): () => void {
  const onChange = (): void => {
    syncRouteFromLocation();
  };
  window.addEventListener('popstate', onChange);
  window.addEventListener('hashchange', onChange);
  syncRouteFromLocation();
  return () => {
    window.removeEventListener('popstate', onChange);
    window.removeEventListener('hashchange', onChange);
  };
}
