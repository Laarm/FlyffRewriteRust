// The water effects of the movers (Plan/41 section 17.3 "Water", ref/20 section 7.6; `CMover::ProcessWaterCircle`, Mover.cpp:3241-3262).
//
// Every 60 Hz tick, for every mover that is not invisible (`TRANSPARENT_MODE`), from its LOGICAL positions (the tick before and now)
// over a patch of the land that is WATER (`tex & 3 == WTYPE_WATER`; clouds are not water) with the surface height `h`:
//
//   the mover was above the surface, is at or under it now, and jumps   `XI_GEN_WATERCROWN01` at `(x, h, z)`
//   it moved, is under the surface and less than 1.5 below it, and the 8 ticks since the last circle have passed
//                                                                        `XI_GEN_WATERCIRCLE01` at `(x, h + 0.01, z)`
//
// The counter of a mover counts every tick (also out of the water), starts at 0 and is reset by a circle: the first circle needs 8
// ticks of life. Positions of a tick batch interpolate from the previous batch (a slow frame runs several ticks on one view).

import { XI_GEN_WATERCIRCLE01, XI_GEN_WATERCROWN01 } from '../../gen/defs/obj';
import type { SfxOptions } from '../fx';
import { WTYPE } from '../terrain/landData';

/** `OBJSTA_JUMP_ALL`: the jump bits of the state word (`IsActJump`). */
export const OBJSTA_JUMP_ALL = 0x0000_f000;
export const MOVER_VISIBLE = 1;
/** Ticks the counter of a mover must have passed before the next circle (`m_nWaterCircleCount > 7`). */
export const CIRCLE_PERIOD = 8;
export const WATER_BAND = 1.5;
/** `TRANSPARENT_MODE` of the mode word. */
export const TRANSPARENT_MODE = 0x2;

type V3 = [number, number, number];

export interface WaterMovers {
  /** Calls `fn(index, id, flags)` for every mover of the view. */
  forEachMover(fn: (index: number, id: number, flags: number) => void): void;
  /** The logical position (LH); `false` for an unknown id. */
  posOf(id: number, out: V3): boolean;
  /** The `OBJSTA_*` word. */
  stateOf(id: number): number;
}

export interface WaterField {
  /** The water record under `(x, z)`: `height` is the surface, `type` the `WTYPE_*`; `null` where no land is resident. */
  waterAt(x: number, z: number): { readonly height: number; readonly type: number } | null;
}

export interface WaterFxDeps {
  readonly sfx: (xi: number, o: SfxOptions) => void;
  /** The mode word of a mover (`visual.mode`), `0` when unknown. */
  readonly modeOf: (id: number) => number;
}

interface Track {
  readonly prev: V3;
  hasPrev: boolean;
  count: number;
  seen: number;
}

export class WaterFx {
  private readonly tracks = new Map<number, Track>();
  private readonly cur: V3 = [0, 0, 0];
  private readonly step: V3 = [0, 0, 0];
  private readonly last: V3 = [0, 0, 0];
  private generation = 0;
  /** Effects made (probe, tests). */
  crowns = 0;
  circles = 0;

  constructor(private readonly deps: WaterFxDeps) {}

  /** `steps` ticks over the movers of `view` (`null`: nothing to do) and the water of `field`. */
  tick(steps: number, view: WaterMovers | null, field: WaterField | null): void {
    if (!view || !field || steps <= 0) return;
    const gen = ++this.generation;
    view.forEachMover((_i, id, flags) => {
      if ((flags & MOVER_VISIBLE) === 0) return;
      const track = this.track(id);
      track.seen = gen;
      if (!view.posOf(id, this.cur)) return;
      if ((this.deps.modeOf(id) & TRANSPARENT_MODE) !== 0) {
        // the counter belongs to the processing of a visible mover only: `m_nWaterCircleCount++` is inside the check
        track.prev[0] = this.cur[0];
        track.prev[1] = this.cur[1];
        track.prev[2] = this.cur[2];
        track.hasPrev = true;
        return;
      }
      if (!track.hasPrev) {
        track.prev[0] = this.cur[0];
        track.prev[1] = this.cur[1];
        track.prev[2] = this.cur[2];
        track.hasPrev = true;
      }
      const jumping = (view.stateOf(id) & OBJSTA_JUMP_ALL) !== 0;
      const a = this.last;
      a[0] = track.prev[0];
      a[1] = track.prev[1];
      a[2] = track.prev[2];
      for (let k = 1; k <= steps; k++) {
        const t = k / steps;
        const s = this.step;
        s[0] = track.prev[0] + (this.cur[0] - track.prev[0]) * t;
        s[1] = track.prev[1] + (this.cur[1] - track.prev[1]) * t;
        s[2] = track.prev[2] + (this.cur[2] - track.prev[2]) * t;
        this.one(track, field, a, s, jumping);
        a[0] = s[0];
        a[1] = s[1];
        a[2] = s[2];
      }
      track.prev[0] = this.cur[0];
      track.prev[1] = this.cur[1];
      track.prev[2] = this.cur[2];
    });
    // forget the movers that left the view
    if (this.tracks.size > 0) {
      for (const [id, t] of this.tracks) if (t.seen !== gen) this.tracks.delete(id);
    }
  }

  private track(id: number): Track {
    let t = this.tracks.get(id);
    if (!t) {
      t = { prev: [0, 0, 0], hasPrev: false, count: 0, seen: 0 };
      this.tracks.set(id, t);
    }
    return t;
  }

  /** One tick: `from` is `vPosTemp`, `to` is `vPos`. */
  private one(track: Track, field: WaterField, from: V3, to: V3, jumping: boolean): void {
    const a = from;
    const w = field.waterAt(to[0], to[2]);
    if (w?.type === WTYPE.water) {
      const h = w.height;
      if (a[1] > h && to[1] <= h && jumping) {
        this.crowns++;
        this.deps.sfx(XI_GEN_WATERCROWN01, { pos: [to[0], h, to[2]] });
      }
      const moved = to[0] !== a[0] || to[1] !== a[1] || to[2] !== a[2];
      if (moved && to[1] < h && to[1] > h - WATER_BAND && track.count > CIRCLE_PERIOD - 1) {
        this.circles++;
        this.deps.sfx(XI_GEN_WATERCIRCLE01, { pos: [to[0], h + 0.01, to[2]] });
        track.count = 0;
      }
    }
    track.count++;
  }

  clear(): void {
    this.tracks.clear();
  }

  get tracked(): number {
    return this.tracks.size;
  }
}
