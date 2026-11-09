// Window classes in chunks of their own (WP-UI-35 follow-up, "the lazy chunks really lazy"): `LazyWnd` is the one place that imports a window
// shell dynamically (`() => import('./WndShop')`), so that nothing else in the application graph imports that module statically and the
// bundler can really split it off. The title screens never load the in-world dialogs; `UiApi.preloadWindows()` brings them in once the UI is
// up, long before a player can click an NPC.
//
//   * `lazy.ctor` is the class once its chunk is in (`null` before): the services build windows with it (`openLazy`);
//   * `openLazy(wm, app, lazy, build)` opens the window at once when the class is loaded, else it loads the chunk, opens the window then, and
//     answers `null` now (the same `T | null` the `open*` methods always had);
//   * in tests the class is there synchronously (`lazyWndSync` in `web/vite.config.ts`, a transform of vitest only, gives every `lazy.ts` the static imports of its
//     shells), so the shell and feature tests keep opening windows right away; `lazy.provide(ctor)` is the manual way in (the gallery).
//
// Windows that are APPLETS (`AppletRegistry` factories: the inventory, the status window, the party window ...) stay static: the world-entry
// sequence opens them synchronously.

import type { WindowManager } from '../wm/WindowManager';
import type { WndNeuz } from './WndNeuz';

/** A window shell class with constructor parameters `A`. */
export type WndClass<T extends WndNeuz = WndNeuz> = abstract new (...args: never[]) => T;

/** Every lazy class that was declared (the preload and the test provider walk it). */
const ALL: LazyWnd<WndClass>[] = [];

let warn: (message: string) => void = () => undefined;
/** Where a chunk that cannot be loaded is reported (the game wires its logger in; the default is silent). */
export function setLazyWindowLog(fn: ((message: string) => void) | null): void {
  warn = fn ?? (() => undefined);
}

export class LazyWnd<C extends WndClass> {
  readonly #loader: () => Promise<C>;
  readonly #sync: (() => C) | null;
  #ctor: C | null = null;
  #loading: Promise<C> | null = null;

  /**
   * `name` is the exported class name of the module. `sync` is the static route to the same class, added by the vitest transform only (tests have
   * no chunks to wait for); the application never passes it.
   */
  constructor(
    readonly name: string,
    loader: () => Promise<C>,
    sync?: () => C,
  ) {
    this.#loader = loader;
    this.#sync = sync ?? null;
    ALL.push(this);
  }

  /** The class once its chunk is in, else `null`. */
  get ctor(): C | null {
    if (this.#ctor === null && this.#sync !== null) this.#ctor = this.#sync();
    return this.#ctor;
  }

  /** Loads the chunk (once; a failure is not remembered, the next call tries again). */
  load(): Promise<C> {
    if (this.ctor) return Promise.resolve(this.ctor);
    this.#loading ??= this.#loader().then(
      (c) => {
        this.#ctor = c;
        return c;
      },
      (e: unknown) => {
        this.#loading = null;
        throw e;
      },
    );
    return this.#loading;
  }

  /** Hands over the class without a chunk (tests, the gallery). */
  provide(ctor: C): void {
    this.#ctor = ctor;
  }
}

/** Every declared lazy class. */
export function allLazyWindows(): readonly LazyWnd<WndClass>[] {
  return ALL;
}

let preloadSettled = false;

/** Loads every declared chunk (in parallel); one that fails does not stop the others. Resolves with the names that failed. */
export async function preloadLazyWindows(): Promise<string[]> {
  const failed: string[] = [];
  await Promise.all(
    ALL.map((l) =>
      l.load().then(
        () => undefined,
        () => {
          failed.push(l.name);
        },
      ),
    ),
  );
  preloadSettled = true;
  return failed;
}

/**
 * `true` once every declared class is in, or once a preload has finished (a chunk that failed to load does not hold the game back: its window then
 * opens late, by itself). The loading gate of the world waits for it, so that a window that is opened with `ui.npc.openDialog(...)` and the like is
 * there at once.
 */
export function lazyWindowsReady(): boolean {
  return preloadSettled || ALL.every((l) => l.ctor !== null);
}

/**
 * Opens the window of `lazy` for `app`. With the class loaded the window is built and opened now (the old window of the app is replaced by
 * `replace`) and returned; without it the chunk is loaded first and `null` is returned for now.
 */
export function openLazy<C extends WndClass>(
  wm: WindowManager,
  app: number,
  lazy: LazyWnd<C>,
  build: (ctor: C) => InstanceType<C>,
  replace: (app: number) => void,
): InstanceType<C> | null {
  const open = (ctor: C): InstanceType<C> | null => {
    replace(app);
    return wm.open(app, () => build(ctor));
  };
  const ctor = lazy.ctor;
  if (ctor) return open(ctor);
  lazy.load().then(
    (c) => {
      open(c);
    },
    (e: unknown) => {
      warn(`the window ${lazy.name} could not be loaded: ${String(e)}`);
    },
  );
  return null;
}
