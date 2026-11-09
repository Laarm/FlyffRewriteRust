// Store framework (Plan/40 section 6.1): `defineStore`, scopes, `resetScope`, the React bindings
// `StoresProvider`, `useStoreSel` and `useNow`. This is the only file of `stores/` that imports React.
// The registry itself (no React) is `registry.ts`; everything is re-exported here so feature code has one
// import path: `import { defineStore, useStoreSel } from '../stores/framework'`.

import {
  createContext,
  createElement,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from 'react';
import { useStore, type StoreApi } from 'zustand';
import { useShallow } from 'zustand/react/shallow';
import { now } from '../platform/time';
import type { CoreStores } from './core';
import {
  createStoreRegistry,
  ObserverWriteError,
  type PersistSpec,
  type ResettableScope,
  type StoreDef,
  type StoreInfo,
  type StoreRegistry,
  type StoreRegistryOptions,
  type StoreScope,
} from './registry';

export {
  createStoreRegistry,
  ObserverWriteError,
  type PersistSpec,
  type ResettableScope,
  type StoreDef,
  type StoreInfo,
  type StoreRegistry,
  type StoreRegistryOptions,
  type StoreScope,
};

/**
 * `defineStore(def)` of Plan/40 section 6.1, with the registry of the owning `ClientContext` made
 * explicit (stores are never module-level singletons, ST2): `defineStore(ctx.stores.registry, def)`.
 */
export function defineStore<S>(registry: StoreRegistry, def: StoreDef<S>): StoreApi<S> {
  return registry.defineStore(def);
}

/** Resets the stores of one registry by scope (section 6.5). */
export function resetScope(registry: StoreRegistry, scope: ResettableScope): void {
  registry.resetScope(scope);
}

// ---- React bindings -----------------------------------------------------------------------------

const StoresContext = createContext<CoreStores | null>(null);

/** Provides the context's stores to the React tree (mounted once by `Root`). */
export function StoresProvider({
  stores,
  children,
}: {
  readonly stores: CoreStores;
  readonly children?: ReactNode;
}) {
  return createElement(StoresContext.Provider, { value: stores }, children);
}

/** The stores of the nearest `StoresProvider`. */
export function useCoreStores(): CoreStores {
  const stores = useContext(StoresContext);
  if (!stores) throw new Error('useCoreStores: no StoresProvider above this component');
  return stores;
}

/**
 * Subscribes a component to a slice of a store. Object/array selections are compared shallowly, so a
 * selector that builds a new object each call does not re-render on every store update.
 */
export function useStoreSel<S, T>(store: StoreApi<S>, selector: (state: S) => T): T {
  return useStore(store, useShallow(selector));
}

/**
 * Re-renders the component `hz` times per second while mounted and returns `now()` (ms), for cooldowns
 * and timers that are derived from the clock rather than from a store value.
 */
export function useNow(hz: 1 | 2 | 4 | 10): number {
  const [t, setT] = useState(() => now());
  useEffect(() => {
    const id = setInterval(() => {
      setT(now());
    }, 1000 / hz);
    return () => {
      clearInterval(id);
    };
  }, [hz]);
  return t;
}
