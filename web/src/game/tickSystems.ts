// Per-60-Hz-step systems (Plan/40 section 10.2 step 6): UI window fades, countdowns, camera smoothing,
// damage numbers. A system is called once per loop iteration with `due`, the number of 60 Hz frames that
// iteration stepped (all of them advance `due` steps, never per rendered frame); it loops internally.
// Exceptions are caught per system (rule L3): the other systems and the rest of the iteration still run.

import type { Unsubscribe } from '../platform/visibility';

/** `due` = frames stepped by this iteration (>= 1; the registry is not run for 0). */
export type TickSystemFn = (due: number) => void;

export interface TickSystems {
  /** Registers a system; systems run in registration order. Names are unique. */
  register(name: string, fn: TickSystemFn): Unsubscribe;
  /** Runs every system with `due`; `onError(name, error)` is called for each throwing system. */
  run(due: number, onError?: (name: string, error: unknown) => void): void;
  readonly names: readonly string[];
}

export function createTickSystems(): TickSystems {
  const systems = new Map<string, TickSystemFn>();
  return {
    register(name, fn) {
      if (systems.has(name)) throw new Error(`tick system "${name}" is already registered`);
      systems.set(name, fn);
      return () => {
        if (systems.get(name) === fn) systems.delete(name);
      };
    },
    run(due, onError) {
      for (const [name, fn] of systems) {
        try {
          fn(due);
        } catch (error) {
          onError?.(name, error);
        }
      }
    },
    get names() {
      return [...systems.keys()];
    },
  };
}
