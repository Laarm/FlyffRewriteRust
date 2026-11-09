// Store registry (Plan/40 section 6.1, ST2/ST7, 6.5, 6.6): every store of one `ClientContext` is created
// through `defineStore` of a registry, so tests and the /dev surfaces build their own isolated set.
// The registry knows each store's reset scope (`resetScope`), its fixtures (`loadFixture`), dumps all
// states for the debug probe and implements the observer write guard (registry phases `Pre`/`Fx` must not
// write stores, Plan/40 section 5.4). No React in this file (React bindings: `framework.ts`).

import type { StoreApi } from 'zustand/vanilla';
import { createStore } from 'zustand/vanilla';
import { persistedStore, type PersistedStoreOptions } from '../platform/persistedStore';

/** Thrown by a store write inside `runReadOnly` (observer handlers must not write stores, section 5.4). */
export class ObserverWriteError extends Error {
  constructor(readonly store: string) {
    super(
      `store "${store}" was written while a read-only observer ran (Plan/40 section 5.4: Pre/Fx handlers are observers)`,
    );
    this.name = 'ObserverWriteError';
  }
}

/** Reset scopes (Plan/40 section 6.5): `session` implies `world` and `map`; `world` implies `map`. */
export type StoreScope = 'never' | 'session' | 'world' | 'map';

/** The scopes `resetScope` accepts. */
export type ResettableScope = Exclude<StoreScope, 'never'>;

/** Persistence of a store (`persistedStore`, section 11.2); the state must carry `v: number`. */
export type PersistSpec<S> = Omit<PersistedStoreOptions<S & { v: number }>, 'defaults'>;

export interface StoreDef<S> {
  /** Unique store name within a registry; also the prefix of its fixtures (`<name>.<fixture>`). */
  readonly name: string;
  readonly scope: StoreScope;
  initial(): S;
  /** Owning document, e.g. `'40'` or `'45'`. */
  readonly owner: string;
  /**
   * Named states for tests and /dev surfaces (section 6.6): `loadFixture('<name>.<fixture>')` replaces
   * the store state with `initial()` overlaid with the fixture.
   */
  readonly fixtures?: Readonly<Record<string, Partial<S>>>;
  /**
   * For stores of scope `world` (or `session`) that hold per-map fields (`worldStore`, 45's `hudStore`):
   * the patch applied by `resetScope('map')`.
   */
  resetMap?(state: S): Partial<S>;
  /** Persist the state as JSON under a `localStorage` key (the state must contain `v`). */
  readonly persist?: PersistSpec<S>;
}

export interface StoreInfo {
  readonly name: string;
  readonly scope: StoreScope;
  readonly owner: string;
  readonly fixtures: readonly string[];
}

export interface StoreRegistryOptions {
  /** Throw when a store is written inside `runReadOnly` (default: dev builds only). */
  readonly assertObservers?: boolean;
}

export interface StoreRegistry {
  /** Creates, registers and returns a store (duplicate names throw). */
  defineStore<S>(def: StoreDef<S>): StoreApi<S>;
  /**
   * Resets the stores of `scope` and of the scopes it implies (section 6.5). A non-persisted store whose state has a
   * numeric `version` (a change counter) keeps it monotonic: the reset state carries `version + 1`, never 0 again, so a
   * read model keyed on the counter never meets an old number (BFIX1 #012-H).
   */
  resetScope(scope: ResettableScope): void;
  /** The store registered as `name` (state type unknown to the registry). */
  get(name: string): StoreApi<unknown> | undefined;
  names(): readonly string[];
  info(): readonly StoreInfo[];
  /** `name -> state` of every store (a shallow snapshot, used by the probe and the fixture tests). */
  snapshot(): Record<string, unknown>;
  /** `'<store>.<fixture>'` -> replaces that store's state. Throws for unknown names. */
  loadFixture(id: string): void;
  /** Runs `fn` with store writes forbidden (observer phases, section 5.4); a write throws in dev builds. */
  runReadOnly<T>(fn: () => T): T;
  /** `true` while inside `runReadOnly`. */
  readonly readOnly: boolean;
}

/** Scopes reset by `resetScope(scope)` (itself plus the narrower implied ones). */
const IMPLIED: Readonly<Record<ResettableScope, readonly ResettableScope[]>> = {
  map: ['map'],
  world: ['world', 'map'],
  session: ['session', 'world', 'map'],
};

interface Entry {
  readonly def: StoreDef<unknown>;
  readonly api: StoreApi<unknown>;
  /** The unguarded `setState` (reset and fixtures bypass the observer guard). */
  readonly rawSetState: StoreApi<unknown>['setState'];
}

function hasVersion(v: unknown): v is { v: number } {
  return typeof v === 'object' && v !== null && typeof (v as { v?: unknown }).v === 'number';
}

/** A state with a numeric `version` change counter (not the persisted `v`, see `hasVersion`). */
function hasCounter(v: unknown): v is { version: number } {
  return (
    typeof v === 'object' && v !== null && typeof (v as { version?: unknown }).version === 'number'
  );
}

export function createStoreRegistry(options: StoreRegistryOptions = {}): StoreRegistry {
  const guardWrites = options.assertObservers ?? import.meta.env.DEV;
  const entries = new Map<string, Entry>();
  let readOnlyDepth = 0;

  const defineStore = <S>(def: StoreDef<S>): StoreApi<S> => {
    if (entries.has(def.name)) {
      throw new Error(`store "${def.name}" is already defined in this registry`);
    }
    let api: StoreApi<S>;
    if (def.persist) {
      const versioned = (): S & { v: number } => {
        const state = def.initial();
        if (!hasVersion(state)) {
          throw new Error(`persisted store "${def.name}" needs a numeric "v" in its state`);
        }
        return state;
      };
      versioned(); // fail at definition time, not on the first reset
      const spec: PersistedStoreOptions<S & { v: number }> = {
        ...def.persist,
        defaults: versioned,
      };
      api = persistedStore(spec);
    } else {
      api = createStore<S>(() => def.initial());
    }

    const rawSetState: StoreApi<S>['setState'] = api.setState.bind(api);
    if (guardWrites) {
      api.setState = ((...args: Parameters<StoreApi<S>['setState']>) => {
        if (readOnlyDepth > 0) {
          throw new ObserverWriteError(def.name);
        }
        rawSetState(...args);
      }) as StoreApi<S>['setState'];
    }
    entries.set(def.name, {
      def,
      api,
      rawSetState,
    });
    return api;
  };

  const resetScope = (scope: ResettableScope): void => {
    const scopes = IMPLIED[scope];
    for (const { def, api, rawSetState } of entries.values()) {
      if (def.scope === 'never') continue;
      if (scopes.includes(def.scope)) {
        const initial = def.initial();
        const current = api.getState();
        // the counter of a read-model key stays monotonic (persisted stores carry `v`, not a counter)
        if (!def.persist && hasCounter(current) && hasCounter(initial)) {
          rawSetState({ ...initial, version: current.version + 1 }, true);
        } else {
          rawSetState(initial, true);
        }
      } else if (def.resetMap) {
        // a store of a wider scope that holds per-map fields: every reset implies `map`
        rawSetState(def.resetMap(api.getState()));
      }
    }
  };

  return {
    defineStore,
    resetScope,
    get: (name) => entries.get(name)?.api,
    names: () => [...entries.keys()],
    info: () =>
      [...entries.values()].map(({ def }) => ({
        name: def.name,
        scope: def.scope,
        owner: def.owner,
        fixtures: Object.keys(def.fixtures ?? {}),
      })),
    snapshot: () => {
      const out: Record<string, unknown> = {};
      for (const [name, e] of entries) out[name] = e.api.getState();
      return out;
    },
    loadFixture: (id) => {
      const dot = id.indexOf('.');
      const storeName = dot < 0 ? id : id.slice(0, dot);
      const fixtureName = dot < 0 ? '' : id.slice(dot + 1);
      const entry = entries.get(storeName);
      if (!entry) throw new Error(`unknown store "${storeName}" in fixture id "${id}"`);
      const fixture = entry.def.fixtures?.[fixtureName];
      if (fixture === undefined) {
        throw new Error(`store "${storeName}" has no fixture "${fixtureName}"`);
      }
      entry.rawSetState({ ...(entry.def.initial() as object), ...(fixture as object) }, true);
    },
    runReadOnly: <T>(fn: () => T): T => {
      readOnlyDepth += 1;
      try {
        return fn();
      } finally {
        readOnlyDepth -= 1;
      }
    },
    get readOnly() {
      return readOnlyDepth > 0;
    },
  };
}
