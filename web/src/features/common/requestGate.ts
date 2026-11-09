// `requestGate(key)` (Plan/45 §2.4): at most one outstanding request per key (lobby operations, a motion, the skill-tree
// finish). The window that sends takes the gate, the answer (or a failure) releases it.

export interface RequestGate {
  /** `true` when the key was free and is now held; `false` when a request with this key is outstanding. */
  tryAcquire(key: string): boolean;
  release(key: string): void;
  isHeld(key: string): boolean;
  /** Releases every key (a new session, a lost connection). */
  clear(): void;
}

export function createRequestGate(): RequestGate {
  const held = new Set<string>();
  return {
    tryAcquire(key) {
      if (held.has(key)) return false;
      held.add(key);
      return true;
    },
    release(key) {
      held.delete(key);
    },
    isHeld: (key) => held.has(key),
    clear() {
      held.clear();
    },
  };
}
