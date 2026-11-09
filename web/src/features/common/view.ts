// Read models for the window shells (Plan/42 §4.5 `*View` ports): `derivedView(compute, sources)` is a `ViewSource` whose
// value is recomputed after one of the observed stores changed (shallow-compared by the caller's `equal`, so the value keeps
// its identity while nothing the window reads changed). The sources are attached lazily with the first subscriber.

import type { ViewSource } from '../../ui/windows/login/ports';

export type { ViewSource };

/** Anything with the zustand `subscribe` shape (a store, a bundle loader, a local emitter). */
export interface Observable {
  subscribe(listener: () => void): () => void;
}

export function derivedView<T>(
  compute: () => T,
  sources: readonly Observable[],
  equal: (a: T, b: T) => boolean = shallowEqual,
): ViewSource<T> {
  let dirty = true;
  let value: T | undefined;
  const listeners = new Set<() => void>();
  let detach: (() => void)[] = [];

  const refresh = (): T => {
    if (dirty || value === undefined) {
      const next = compute();
      value = value !== undefined && equal(value, next) ? value : next;
      dirty = false;
    }
    return value;
  };

  const onSource = (): void => {
    dirty = true;
    const before = value;
    const after = refresh();
    if (before !== after) for (const fn of [...listeners]) fn();
  };

  return {
    get: () => {
      // without subscribers nothing marks the value dirty: recompute on every read
      if (listeners.size === 0) dirty = true;
      return refresh();
    },
    subscribe(fn) {
      listeners.add(fn);
      if (listeners.size === 1) {
        // the sources were not watched until now: whatever changed meanwhile is not in the cached value
        dirty = true;
        detach = sources.map((s) => s.subscribe(onSource));
      }
      return () => {
        listeners.delete(fn);
        if (listeners.size === 0) {
          for (const d of detach) d();
          detach = [];
        }
      };
    },
  };
}

/** Own-key shallow comparison of two plain objects (functions and nested objects by identity). */
export function shallowEqual<T>(a: T, b: T): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    if (!Object.is((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]))
      return false;
  }
  return true;
}

/** A tiny observable value holder (a controller's local state: busy flags, locks). */
export interface LocalState<T> extends Observable {
  get(): T;
  set(patch: Partial<T>): void;
}

export function localState<T extends object>(initial: T): LocalState<T> {
  let state = initial;
  const fns = new Set<() => void>();
  return {
    get: () => state,
    set(patch) {
      const next = { ...state, ...patch };
      if (shallowEqual(state, next)) return;
      state = next;
      for (const fn of [...fns]) fn();
    },
    subscribe(fn) {
      fns.add(fn);
      return () => {
        fns.delete(fn);
      };
    },
  };
}
