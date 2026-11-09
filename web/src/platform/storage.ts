// The only module that touches localStorage/sessionStorage (Plan/02 section 11.7, Plan/40 section 11.2).
// Every access is wrapped in try/catch with an in-memory fallback, so the page works in private
// windows, with blocked site data and in previews.

export interface SafeStorage {
  /** The stored value; `null` when missing or unreadable. */
  get(key: string): string | null;
  /** `true` when the value reached persistent storage; the value is kept in memory either way. */
  set(key: string, value: string): boolean;
  remove(key: string): void;
  /** `false` once the backing storage proved unavailable (private mode, blocked storage, quota). */
  readonly persistent: boolean;
}

/**
 * Builds a safe wrapper around a storage getter. `getBacking` may throw (the `localStorage` accessor
 * itself throws when site data is blocked) and may return a storage whose methods throw.
 */
export function createSafeStorage(getBacking: () => Storage): SafeStorage {
  const memory = new Map<string, string>();
  let persistent = true;

  const backing = (): Storage | null => {
    try {
      return getBacking();
    } catch {
      persistent = false;
      return null;
    }
  };

  return {
    get(key) {
      const storage = backing();
      if (storage) {
        try {
          const value = storage.getItem(key);
          if (value !== null) return value;
        } catch {
          persistent = false;
        }
      }
      return memory.get(key) ?? null;
    },
    set(key, value) {
      memory.set(key, value);
      const storage = backing();
      if (!storage) return false;
      try {
        storage.setItem(key, value);
        return true;
      } catch {
        persistent = false;
        return false;
      }
    },
    remove(key) {
      memory.delete(key);
      const storage = backing();
      if (!storage) return;
      try {
        storage.removeItem(key);
      } catch {
        persistent = false;
      }
    },
    get persistent() {
      return persistent;
    },
  };
}

let local: SafeStorage | undefined;
let session: SafeStorage | undefined;

/** `safeStorage.local` / `safeStorage.session` (created lazily so importing never touches storage). */
export const safeStorage: { readonly local: SafeStorage; readonly session: SafeStorage } = {
  get local() {
    return (local ??= createSafeStorage(() => globalThis.localStorage));
  },
  get session() {
    return (session ??= createSafeStorage(() => globalThis.sessionStorage));
  },
};
