// A tiny observable cell for the controller -> React path (Plan/42 §4.1 `ControlState`, §2.4 rule 1).
//
// Controllers mutate state through setters that COMPARE before writing: a write of an equal value emits nothing,
// so React never re-renders for it. React reads through `useSyncExternalStore(cell.subscribe, cell.get)`; `get`
// returns the same object until a change, as that hook requires.

export type Listener = () => void;

/** One piece of observable state. */
export class Observable<S> {
  #value: S;
  // a method-typed field: `Observable<Sub>` stays assignable to `Observable<Base>` (a control with extra state is a `Control`)
  readonly #equals: { compare(a: S, b: S): boolean }['compare'];
  readonly #listeners = new Set<Listener>();

  constructor(initial: S, equals: (a: S, b: S) => boolean = Object.is) {
    this.#value = initial;
    this.#equals = equals;
  }

  /** Arrow properties: they are passed to `useSyncExternalStore` unbound. */
  readonly get = (): S => this.#value;

  /** Replaces the value; returns `false` (and emits nothing) when it equals the current one. */
  set(next: S): boolean {
    if (this.#equals(this.#value, next)) return false;
    this.#value = next;
    this.#emit();
    return true;
  }

  readonly subscribe = (fn: Listener): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };

  /** Number of subscribers (tests, leak checks). */
  get listenerCount(): number {
    return this.#listeners.size;
  }

  #emit(): void {
    for (const fn of [...this.#listeners]) fn();
  }
}

/**
 * Observable record: `patch` merges fields and emits only when at least one `Object.is`-different value is
 * written; unchanged writes keep the previous object (so selectors and React see the same reference).
 */
export class ControlState<S extends object> extends Observable<S> {
  /** Merges `patch`; returns `true` when something changed. */
  patch(patch: Partial<S>): boolean {
    const cur = this.get();
    let changed = false;
    for (const k of Object.keys(patch) as (keyof S)[]) {
      if (!(k in patch)) continue;
      const v = patch[k];
      if (v !== undefined && !Object.is(cur[k], v)) {
        changed = true;
        break;
      }
    }
    if (!changed) return false;
    return this.set({ ...cur, ...patch });
  }
}
