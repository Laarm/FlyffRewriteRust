// `RegionTracker` (Plan/44 §11.4, WP-AUDIO-04): the region loop of `CWndWorld::Process` (`src:_Interface/WndWorld.cpp:8775-8890`).
// Every tick the truncated player position is tested against the client region list of the world (`m_aRegion`: the `worlds`
// bundle's `regions` in `.rgn` file order); a region the player just entered yields its music (and, for the captions of
// doc 45, its title and description); a region the position left clears its `inside` flag.
//
//   PtInRect: left <= x < right && top <= z < bottom (CRect), on `(int)x`, `(int)z` (truncation toward zero)
//   __VER >= 9: the scan stops after the FIRST newly entered region, so the flags of the later regions keep their value
//   RA_INN regions count as entered only when the player stands on the ground (`(int)y == (int)landHeight`): the music is
//   applied either way, the caption (`captionOk`) only when it is on the ground; an inn region above the ground therefore
//   "enters" again on every tick (KEEP-QUIRK).
//
// The `RA_INN` check asks the sim for the land height (`SimBridge.landHeight`, the only sim query of the audio layer). It may
// answer `undefined` (the per-iteration query budget is used up): the entry is then deferred by one tick with the same early
// stop. The shipped data has no `RA_INN` region, so the shipped game makes no such call at all.

import type { ClientRegion } from '../gen/bundles';
import { RA_INN } from '../gen/defs/define';

export interface RegionDeps {
  /** The client region list of the current world, `undefined` while it is not known; a new array = a new world. */
  readonly regions: () => readonly ClientRegion[] | undefined;
  /** `SimBridge.landHeight` (cap Q): `undefined` = not executed this iteration. */
  readonly landHeight: (x: number, z: number) => number | undefined;
  /** Incremented when an `RA_INN` entry had to wait for the height query. */
  readonly onDeferred?: () => void;
}

/** What doc 45 subscribes to: a region was entered; `captionOk` is false for an `RA_INN` region above the ground. */
export type RegionEntered = (region: ClientRegion, captionOk: boolean) => void;

export class RegionTracker {
  readonly #deps: RegionDeps;
  #list: readonly ClientRegion[] | undefined;
  #inside = new Uint8Array(0);
  readonly #listeners = new Set<RegionEntered>();
  /** The (at most one element) result of `update`, reused so a tick allocates nothing. */
  readonly #result: ClientRegion[] = [];

  constructor(deps: RegionDeps) {
    this.#deps = deps;
  }

  /** Doc 45's caption / navigator subscription. */
  onEntered(fn: RegionEntered): () => void {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  }

  /** The indices of the regions the player is inside (for the probe). */
  get insideRegions(): number[] {
    const out: number[] = [];
    this.#inside.forEach((v, i) => {
      if (v !== 0) out.push(i);
    });
    return out;
  }

  /** The `inside` flags are cleared when the world is (re)opened: enter, every `Replace`, a re-attach into another world. */
  reset(): void {
    this.#inside.fill(0);
  }

  /**
   * Tests the position; returns the region entered this tick (0 or 1 element; the array is reused: copy it to keep it). `y`
   * is only used for `RA_INN` regions.
   */
  update(x: number, y: number, z: number): readonly ClientRegion[] {
    const result = this.#result;
    result.length = 0;
    const list = this.#deps.regions();
    if (list === undefined) return result;
    if (list !== this.#list || this.#inside.length !== list.length) {
      this.#list = list;
      this.#inside = new Uint8Array(list.length);
    }
    const px = Math.trunc(x);
    const pz = Math.trunc(z);
    for (let i = 0; i < list.length; i += 1) {
      const r = list[i];
      if (r === undefined) continue;
      const [left, top, right, bottom] = r.rect;
      if (left <= px && px < right && top <= pz && pz < bottom) {
        if (this.#inside[i] === 0) {
          let ok = true;
          if ((r.attribute & RA_INN) !== 0) {
            const h = this.#deps.landHeight(x, z);
            if (h === undefined) {
              this.#deps.onDeferred?.();
              return result; // inside[i] stays false: re-evaluated on the next tick
            }
            ok = Math.trunc(y) === Math.trunc(h);
          }
          this.#inside[i] = ok ? 1 : 0;
          result.push(r);
          for (const fn of [...this.#listeners]) {
            try {
              fn(r, ok);
            } catch {
              // a caption subscriber must not break the music
            }
          }
          return result; // __VER >= 9: stop scanning after the first entry
        }
      } else {
        this.#inside[i] = 0;
      }
    }
    return result;
  }
}
