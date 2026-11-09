// Persisted-store helper (Plan/40 section 11.2, WP-CLIENT-14, PL-128): a zustand vanilla store whose
// state is saved as versioned JSON under one key of `safeStorage.local`. Used by 45's `optionsStore`
// (`flyff.options.v1`) and available to 42 and 43. Every storage access goes through `SafeStorage`
// (try/catch with an in-memory fallback), so the store works without storage.

import { createStore, type StoreApi } from 'zustand/vanilla';
import { safeStorage, type SafeStorage } from './storage';
import { onVisibility } from './visibility';

export interface PersistedStoreOptions<S extends { v: number }> {
  /** Storage key, e.g. `flyff.options.v1` (key table: Plan/40 section 11.3). */
  readonly key: string;
  /** Current schema version; stored in the state as `v`. */
  readonly version: number;
  /** The state used when nothing (readable) is stored. Must contain `v: version`. */
  defaults(): S;
  /**
   * Converts an OLDER stored state (`fromVersion < version`). Return `null` (or throw) when the value
   * cannot be converted: `defaults()` is used and the unreadable value is kept under `<key>.bad`.
   */
  migrate?(old: unknown, fromVersion: number): S | null;
  /** Runs after every load (PL-128: options forced at load: `Interface = 1`, `bloom = 0`, ...). */
  forced?(state: S): S;
  /** Write debounce in ms (default 500). */
  readonly debounceMs?: number;
  /** Test seam: defaults to `safeStorage.local`. */
  readonly storage?: SafeStorage;
}

/** The store plus the persistence controls. */
export type PersistedStore<S extends { v: number }> = StoreApi<S> & {
  /** Writes a pending change immediately (also done on `pagehide` and when the tab is hidden). */
  flush(): void;
  /** Stops persisting (unsubscribes, drops a pending timer). The store itself stays usable. */
  dispose(): void;
};

const DEFAULT_DEBOUNCE_MS = 500;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** Keeps an unreadable stored value once under `<key>.bad` for diagnosis (never overwrites). */
function keepBad(storage: SafeStorage, key: string, raw: string): void {
  const badKey = `${key}.bad`;
  if (storage.get(badKey) === null) {
    storage.set(badKey, raw);
  }
}

interface Loaded<S> {
  readonly state: S;
  /** `true` when the stored value was migrated or replaced and should be written back. */
  readonly dirty: boolean;
}

function load<S extends { v: number }>(
  o: PersistedStoreOptions<S>,
  storage: SafeStorage,
): Loaded<S> {
  const withVersion = (s: S): S => ({ ...s, v: o.version });
  const finish = (s: S, dirty: boolean): Loaded<S> => ({
    state: withVersion(o.forced ? o.forced(s) : s),
    dirty,
  });

  const raw = storage.get(o.key);
  if (raw === null) return finish(o.defaults(), false);

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    keepBad(storage, o.key, raw);
    return finish(o.defaults(), true);
  }
  if (!isRecord(parsed) || typeof parsed['v'] !== 'number') {
    keepBad(storage, o.key, raw);
    return finish(o.defaults(), true);
  }

  const fromVersion = parsed['v'];
  if (fromVersion === o.version) {
    // a shallow merge with the defaults tolerates fields added without a version bump
    return finish({ ...o.defaults(), ...(parsed as Partial<S>) }, false);
  }
  if (fromVersion < o.version && o.migrate) {
    try {
      const migrated = o.migrate(parsed, fromVersion);
      if (migrated !== null) return finish(migrated, true);
    } catch {
      // fall through to the defaults
    }
  }
  keepBad(storage, o.key, raw);
  return finish(o.defaults(), true);
}

/**
 * Creates the persisted store. The state is loaded synchronously; changes are written debounced
 * (at most `debounceMs` after the first unwritten change) and flushed on `pagehide`/hidden.
 */
export function persistedStore<S extends { v: number }>(
  o: PersistedStoreOptions<S>,
): PersistedStore<S> {
  const storage = o.storage ?? safeStorage.local;
  const debounceMs = o.debounceMs ?? DEFAULT_DEBOUNCE_MS;
  const loaded = load(o, storage);
  const store = createStore<S>(() => loaded.state);

  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending = false;

  const write = (): void => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    if (!pending) return;
    pending = false;
    try {
      storage.set(o.key, JSON.stringify({ ...store.getState(), v: o.version }));
    } catch {
      // unserialisable state: keep working in memory
    }
  };

  const unsubscribeStore = store.subscribe(() => {
    pending = true;
    timer ??= setTimeout(write, debounceMs);
  });
  const unsubscribeVisibility = onVisibility((e) => {
    if (e === 'pagehide' || e === 'hidden') write();
  });

  if (loaded.dirty) {
    pending = true;
    timer = setTimeout(write, debounceMs);
  }

  return Object.assign(store, {
    flush: write,
    dispose: () => {
      unsubscribeStore();
      unsubscribeVisibility();
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      pending = false;
    },
  });
}
