// The core stores of Plan/40 section 6.2 (sessionStore, selfStore, worldStore, devStore) created in one
// registry per `ClientContext`. Feature stores (45) are added with `registry.defineStore`.

import type { StoreApi } from 'zustand/vanilla';
import { initialDevState, type DevState } from './dev';
import { createStoreRegistry, type StoreRegistry, type StoreRegistryOptions } from './registry';
import { initialSelfState, type SelfState } from './self';
import { initialSessionState, type SessionState } from './session';
import { initialWorldState, worldMapReset, type WorldState } from './world';

export interface CoreStores {
  /** The registry all stores of the context belong to (reset scopes, fixtures, probe dump). */
  readonly registry: StoreRegistry;
  readonly sessionStore: StoreApi<SessionState>;
  readonly selfStore: StoreApi<SelfState>;
  readonly worldStore: StoreApi<WorldState>;
  readonly devStore: StoreApi<DevState>;
}

/** Creates a fresh registry (or uses `registry`) with the four core stores. */
export function createCoreStores(
  options: StoreRegistryOptions & { readonly registry?: StoreRegistry } = {},
): CoreStores {
  const registry = options.registry ?? createStoreRegistry(options);
  return {
    registry,
    sessionStore: registry.defineStore<SessionState>({
      name: 'session',
      scope: 'never',
      owner: '40',
      initial: initialSessionState,
    }),
    selfStore: registry.defineStore<SelfState>({
      name: 'self',
      scope: 'world',
      owner: '40',
      initial: initialSelfState,
    }),
    worldStore: registry.defineStore<WorldState>({
      name: 'world',
      scope: 'world',
      owner: '40',
      initial: initialWorldState,
      resetMap: worldMapReset,
    }),
    devStore: registry.defineStore<DevState>({
      name: 'dev',
      scope: 'never',
      owner: '40',
      initial: initialDevState,
    }),
  };
}
