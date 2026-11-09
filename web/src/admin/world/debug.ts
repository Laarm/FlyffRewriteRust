// The debug hooks of the map (`window.__adminMapDebug`, next to `window.__adminMapStats` of Plan/25 section 3.7): the counters are the plan's; these
// add what the Playwright checks need and an operator can use in the console of the browser: the entities held, the view, a SYNTHETIC population
// (the 60 fps probe with 5,000 entities needs no server load) and a frame-time probe. They only touch the viewer's own client state.
import type { Ent } from './entities';
import type { MapEngine, MapStats } from './engine';
import type { MapSession } from './session';
import { Flag, Kind, TierId } from './types';
import { viewRect } from './view';

export interface ProbeResult {
  readonly frames: number;
  /** Mean and 95th percentile of the time between frames in ms. */
  readonly meanMs: number;
  readonly p95Ms: number;
  readonly fps: number;
  /** The CPU cost of a frame of the engine in ms (mean of the frames). */
  readonly engineMs: number;
}

export interface MapDebug {
  stats(): MapStats;
  /** `{cx, cz, upp, w, h}`. */
  view(): { cx: number; cz: number; upp: number; w: number; h: number };
  setView(cx: number, cz: number, upp: number): void;
  /** The entities held, with their current (target) positions. */
  entities(): { id: number; kind: number; sub: number; x: number; z: number }[];
  /** Stops the live feed and fills the store with `n` entities spread over the viewport; `moving` makes a third of them walk. */
  inject(n: number, options?: { moving?: boolean }): void;
  clearSynthetic(): void;
  /** Pans the view `frames` frames and reports the frame times. */
  probe(frames: number): Promise<ProbeResult>;
  /** The colour at a CSS pixel of the GL canvas (`[r, g, b, a]`). */
  pixel(x: number, y: number): number[] | null;
  /** `true` when the GPU takes BC1 blocks as they are (else the decoder converts the tiles to RGBA8). */
  s3tc(): boolean;
  /** Tiles on the GPU. */
  tileCount(): number;
  /** What the tile cache did: requested, loaded, missing, failed, aborted, evicted. */
  tileStats(): {
    requested: number;
    loaded: number;
    missing: number;
    failed: number;
    aborted: number;
    evicted: number;
  };
}

/** A small deterministic generator (the probe must be repeatable). */
function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

export function debugHooks(engine: MapEngine, session: MapSession): MapDebug {
  let walker: ReturnType<typeof setInterval> | null = null;
  const stop = (): void => {
    if (walker !== null) clearInterval(walker);
    walker = null;
  };
  return {
    stats: () => engine.stats,
    view: () => {
      const v = engine.view;
      return { cx: v.cx, cz: v.cz, upp: v.upp, w: v.w, h: v.h };
    },
    setView: (cx, cz, upp) => {
      engine.setView({ cx, cz, upp });
    },
    entities: () =>
      [...engine.store.byId.values()].map((e: Ent) => ({
        id: e.id,
        kind: e.kind,
        sub: e.sub,
        x: e.x1,
        z: e.z1,
      })),
    inject(n, options = {}) {
      stop();
      session.pause();
      const rnd = lcg(12345);
      const r = viewRect(engine.view, 0);
      const spawns = [];
      for (let i = 0; i < n; i += 1) {
        const t = rnd();
        const kind =
          t < 0.05 ? Kind.PLAYER : t < 0.15 ? Kind.NPC : t < 0.2 ? Kind.ITEM : Kind.MONSTER;
        spawns.push({
          id: 1_000_000 + i,
          kind,
          sub: kind === Kind.MONSTER ? (rnd() < 0.05 ? 1 : 0) : 0,
          prop: 100 + Math.floor(rnd() * 50),
          level: 1 + Math.floor(rnd() * 60),
          x: r[0] + rnd() * (r[2] - r[0]),
          z: r[1] + rnd() * (r[3] - r[1]),
          yaw: Math.floor(rnd() * 256),
          hpPct: 100,
          flags: rnd() < 0.3 ? Flag.ASLEEP : 0,
          target: 0,
        });
      }
      engine.store.apply(
        {
          keyframe: true,
          tick: 1,
          tier: TierId.A,
          degraded: false,
          binM: 128,
          spawns,
          removes: [],
          bins: [],
          names: [],
        },
        performance.now(),
      );
      engine.markDirty();
      if (options.moving === true) {
        const movers = spawns
          .filter((s) => s.kind === Kind.PLAYER || s.kind === Kind.MONSTER)
          .filter((_, i) => i % 3 === 0);
        let tick = 1;
        walker = setInterval(() => {
          tick += 1;
          const ids: number[] = [];
          const x: number[] = [];
          const z: number[] = [];
          const yaw: number[] = [];
          const hp: number[] = [];
          const flags: number[] = [];
          for (const m of movers) {
            m.x += (rnd() - 0.5) * 3;
            m.z += (rnd() - 0.5) * 3;
            ids.push(m.id);
            x.push(m.x);
            z.push(m.z);
            yaw.push(Math.floor(rnd() * 256));
            hp.push(100);
            flags.push(m.flags);
          }
          engine.store.apply(
            {
              keyframe: false,
              tick,
              tier: TierId.A,
              degraded: false,
              binM: 0,
              spawns: [],
              moves: { ids, x, z, yaw, hpPct: hp, flags, target: [] },
              removes: [],
              bins: [],
              names: [],
            },
            performance.now(),
          );
          engine.markDirty();
        }, 200);
      }
    },
    clearSynthetic() {
      stop();
      engine.store.clear();
      engine.markDirty();
      session.resume();
    },
    async probe(frames) {
      const times: number[] = [];
      const v0 = engine.view;
      let last = performance.now();
      for (let i = 0; i < frames; i += 1) {
        // a pan of a few pixels each frame, back and forth: the view changes every frame
        const dx = Math.sin(i / 12) * 6;
        engine.setView({
          cx: v0.cx + dx * v0.upp * 4,
          cz: v0.cz + Math.cos(i / 17) * 3 * v0.upp * 4,
        });
        await new Promise<void>((resolve) => {
          requestAnimationFrame(() => {
            resolve();
          });
        });
        const now = performance.now();
        times.push(now - last);
        last = now;
      }
      engine.setView({ cx: v0.cx, cz: v0.cz });
      const sorted = [...times].sort((a, b) => a - b);
      const mean = times.reduce((a, b) => a + b, 0) / Math.max(1, times.length);
      return {
        frames,
        meanMs: mean,
        p95Ms: sorted[Math.floor(sorted.length * 0.95)] ?? 0,
        fps: mean > 0 ? 1000 / mean : 0,
        engineMs: engine.stats.frameMs,
      };
    },
    pixel: (x, y) => engine.gl.readPixel(x, y),
    s3tc: () => engine.gl.s3tc,
    tileCount: () => engine.tileCache?.size ?? 0,
    tileStats: () => ({
      ...(engine.tileCache?.stats ?? {
        requested: 0,
        loaded: 0,
        missing: 0,
        failed: 0,
        aborted: 0,
        evicted: 0,
      }),
    }),
  };
}
