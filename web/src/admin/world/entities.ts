// The entities of the live map (Plan/25 section 3.5, 3.7, 3.16): what the keyframes and deltas of `flyff.admin.v1` say, held per object id with
// the previous and the current position (for the one-interval slide of `interp.ts`), the names of the players, the tier-C bins, and a
// uniform 64 m spatial hash that the renderer, the picking and the lists query. The store knows no DOM, no GL and no protobuf runtime: the
// frame types below are satisfied structurally by the messages `gen/proto/admin` decodes.
import { isSnap, lerp, lerpYaw, slideFraction, smoothInterval } from './interp';
import { Kind, TierId, nominalIntervalMs } from './types';
import type { Rect } from './view';

/** The side of a bucket of the spatial hash in metres (Plan/25 section 3.7). */
export const BUCKET_M = 64;

/** A `MapEntitySpawn`. */
export interface SpawnRec {
  readonly id: number;
  readonly kind: number;
  readonly sub: number;
  readonly prop: number;
  readonly level: number;
  readonly x: number;
  readonly z: number;
  readonly yaw: number;
  readonly hpPct: number;
  readonly flags: number;
  readonly target: number;
}

/** A `MapMoves` (packed arrays; `target` is empty below tier A). */
export interface MovesRec {
  readonly ids: readonly number[];
  readonly x: readonly number[];
  readonly z: readonly number[];
  readonly yaw: readonly number[];
  readonly hpPct: readonly number[];
  readonly flags: readonly number[];
  readonly target: readonly number[];
}

/** A `MapBin`: a 128 m cell of the overview with the counts of the monsters in it. */
export interface BinRec {
  readonly bx: number;
  readonly bz: number;
  readonly normal: number;
  readonly captain: number;
  readonly asleep: number;
}

/** The part of a `MapKeyframe` / `MapDelta` the store reads. */
export interface FrameData {
  readonly keyframe: boolean;
  readonly tick: number;
  readonly tier: number;
  readonly degraded: boolean;
  /** The bin size in metres of a keyframe (0: keep). */
  readonly binM: number;
  readonly spawns: readonly SpawnRec[];
  readonly moves?: MovesRec | undefined;
  readonly removes: readonly number[];
  readonly bins: readonly BinRec[];
  readonly names: readonly { readonly id: number; readonly name: string }[];
}

/** One object on the map. */
export class Ent {
  id: number;
  kind: number;
  sub: number;
  prop: number;
  level: number;
  flags = 0;
  hp = 0;
  target = 0;
  /** The previous and the current position (and yaw byte): the slide goes from 0 to 1. */
  x0 = 0;
  z0 = 0;
  yaw0 = 0;
  x1 = 0;
  z1 = 0;
  yaw1 = 0;
  /** When the current position arrived (ms of the client clock) and how long the slide takes. */
  t = 0;
  dur = 0;
  /** The bucket of the current position and the index in it. */
  bucket = 0;
  slot = 0;
  /** The displayed position, written by {@link EntityStore.place}. */
  x = 0;
  z = 0;
  yaw = 0;
  /** Set by a keyframe to find the entities it did not mention. */
  mark = 0;

  constructor(id: number, kind: number, sub: number, prop: number, level: number) {
    this.id = id;
    this.kind = kind;
    this.sub = sub;
    this.prop = prop;
    this.level = level;
  }
}

function bucketKey(x: number, z: number): number {
  const bx = Math.max(-4000, Math.min(4000, Math.floor(x / BUCKET_M)));
  const bz = Math.max(-4000, Math.min(4000, Math.floor(z / BUCKET_M)));
  return (bx + 4096) * 8192 + (bz + 4096);
}

export class EntityStore {
  readonly byId = new Map<number, Ent>();
  readonly names = new Map<number, string>();
  readonly #buckets = new Map<number, Ent[]>();
  readonly #kindCount = [0, 0, 0, 0, 0, 0, 0];
  #bins: readonly BinRec[] = [];
  /** Bin size in metres (128 by default). */
  binM = 128;
  tier: number = TierId.A;
  degraded = false;
  /** Increments on every change (the renderer redraws, the lists refresh). */
  version = 0;
  /** The smoothed gap between frames in ms (the slide length). */
  interval = 0;
  /** The client time of the last frame, `0` before the first. */
  lastFrameAt = 0;
  /** The server tick of the last frame. */
  tick = 0;
  /** The slide of the latest frame ends at this client time. */
  slideUntil = 0;
  #mark = 0;
  #hasFrame = false;

  get size(): number {
    return this.byId.size;
  }

  /** Entities of `kind` held. */
  count(kind: number): number {
    return this.#kindCount[kind] ?? 0;
  }

  get bins(): readonly BinRec[] {
    return this.#bins;
  }

  /** `true` while some entity is still sliding at `now`. */
  sliding(now: number): boolean {
    return now < this.slideUntil;
  }

  clear(): void {
    this.byId.clear();
    this.names.clear();
    this.#buckets.clear();
    this.#kindCount.fill(0);
    this.#bins = [];
    this.lastFrameAt = 0;
    this.#hasFrame = false;
    this.interval = 0;
    this.slideUntil = 0;
    this.version += 1;
  }

  /** Applies a keyframe or a delta received at `now` (ms of the client clock). */
  apply(f: FrameData, now: number): void {
    const gap = this.#hasFrame ? now - this.lastFrameAt : 0;
    this.#hasFrame = true;
    const nominal = nominalIntervalMs(f.tier);
    this.interval = gap > 0 ? smoothInterval(this.interval, gap, nominal) : nominal;
    this.lastFrameAt = now;
    this.tick = f.tick;
    if (f.tier !== this.tier) {
      this.tier = f.tier;
    }
    this.degraded = f.degraded;
    if (f.binM > 0) this.binM = f.binM;
    if (f.keyframe) this.#keyframe(f, now);
    else this.#delta(f, now);
    this.version += 1;
  }

  #keyframe(f: FrameData, now: number): void {
    this.#mark += 1;
    const mark = this.#mark;
    for (const s of f.spawns) {
      const e = this.#upsert(s, now);
      e.mark = mark;
    }
    for (const e of [...this.byId.values()]) if (e.mark !== mark) this.#remove(e);
    this.#bins = f.bins;
    this.names.clear();
    for (const n of f.names) this.names.set(n.id, n.name);
  }

  #delta(f: FrameData, now: number): void {
    for (const id of f.removes) {
      const e = this.byId.get(id);
      if (e) this.#remove(e);
    }
    for (const s of f.spawns) this.#upsert(s, now);
    const m = f.moves;
    if (m) {
      for (let i = 0; i < m.ids.length; i += 1) {
        const e = this.byId.get(m.ids[i] ?? -1);
        if (!e) continue;
        this.#move(e, m.x[i] ?? e.x1, m.z[i] ?? e.z1, m.yaw[i] ?? e.yaw1, now);
        e.hp = m.hpPct[i] ?? e.hp;
        e.flags = m.flags[i] ?? e.flags;
        if (m.target.length > i) e.target = m.target[i] ?? 0;
      }
    }
    if (f.tier === TierId.C || f.bins.length > 0) this.#bins = f.bins;
    else if (this.tier !== TierId.C && this.#bins.length > 0) this.#bins = [];
    for (const n of f.names) this.names.set(n.id, n.name);
  }

  #upsert(s: SpawnRec, now: number): Ent {
    let e = this.byId.get(s.id);
    if (!e) {
      e = new Ent(s.id, s.kind, s.sub, s.prop, s.level);
      e.x0 = e.x1 = s.x;
      e.z0 = e.z1 = s.z;
      e.yaw0 = e.yaw1 = s.yaw;
      e.t = now;
      this.byId.set(s.id, e);
      this.#kindCount[s.kind] = (this.#kindCount[s.kind] ?? 0) + 1;
      this.#insert(e);
    } else {
      if (e.kind !== s.kind) {
        this.#kindCount[e.kind] = (this.#kindCount[e.kind] ?? 1) - 1;
        this.#kindCount[s.kind] = (this.#kindCount[s.kind] ?? 0) + 1;
      }
      e.kind = s.kind;
      e.sub = s.sub;
      e.prop = s.prop;
      e.level = s.level;
      this.#move(e, s.x, s.z, s.yaw, now);
    }
    e.hp = s.hpPct;
    e.flags = s.flags;
    e.target = s.target;
    return e;
  }

  /** The current position changes: the slide restarts from the old current position unless it is a teleport. */
  #move(e: Ent, x: number, z: number, yaw: number, now: number): void {
    if (e.x1 === x && e.z1 === z && e.yaw1 === yaw) return;
    e.x0 = e.x1;
    e.z0 = e.z1;
    e.yaw0 = e.yaw1;
    if (isSnap(e.x0, e.z0, x, z)) {
      e.x0 = x;
      e.z0 = z;
      e.yaw0 = yaw;
    }
    e.x1 = x;
    e.z1 = z;
    e.yaw1 = yaw;
    e.t = now;
    e.dur = this.interval;
    if (now + e.dur > this.slideUntil) this.slideUntil = now + e.dur;
    const key = bucketKey(x, z);
    if (key !== e.bucket) {
      this.#detach(e);
      this.#insert(e);
    }
  }

  #insert(e: Ent): void {
    const key = bucketKey(e.x1, e.z1);
    let b = this.#buckets.get(key);
    if (!b) {
      b = [];
      this.#buckets.set(key, b);
    }
    e.bucket = key;
    e.slot = b.length;
    b.push(e);
  }

  #detach(e: Ent): void {
    const b = this.#buckets.get(e.bucket);
    if (!b) return;
    const last = b.pop();
    if (last && last !== e) {
      b[e.slot] = last;
      last.slot = e.slot;
    }
    if (b.length === 0) this.#buckets.delete(e.bucket);
  }

  #remove(e: Ent): void {
    this.#detach(e);
    this.byId.delete(e.id);
    this.#kindCount[e.kind] = (this.#kindCount[e.kind] ?? 1) - 1;
    this.names.delete(e.id);
  }

  /** Writes the displayed position of `e` at `now` (`e.x`, `e.z`, `e.yaw`). */
  place(e: Ent, now: number): void {
    const f = slideFraction(now, e.t, e.dur);
    if (f >= 1) {
      e.x = e.x1;
      e.z = e.z1;
      e.yaw = e.yaw1;
    } else {
      e.x = lerp(e.x0, e.x1, f);
      e.z = lerp(e.z0, e.z1, f);
      e.yaw = lerpYaw(e.yaw0, e.yaw1, f);
    }
  }

  /** Calls `fn` for every entity whose bucket overlaps `rect` (the entity itself may lie up to 64 m outside it). */
  queryBuckets(rect: Rect, fn: (e: Ent) => void): void {
    const bx0 = Math.floor(rect[0] / BUCKET_M);
    const bx1 = Math.floor(rect[2] / BUCKET_M);
    const bz0 = Math.floor(rect[1] / BUCKET_M);
    const bz1 = Math.floor(rect[3] / BUCKET_M);
    // a rectangle with more buckets than the hash holds is cheaper to answer by walking the hash
    if ((bx1 - bx0 + 1) * (bz1 - bz0 + 1) > this.#buckets.size * 2) {
      for (const b of this.#buckets.values()) for (const e of b) fn(e);
      return;
    }
    for (let bx = bx0; bx <= bx1; bx += 1) {
      for (let bz = bz0; bz <= bz1; bz += 1) {
        const b = this.#buckets.get(bucketKey(bx * BUCKET_M, bz * BUCKET_M));
        if (b) for (const e of b) fn(e);
      }
    }
  }

  /** The entities whose CURRENT position lies inside `rect`. */
  inRect(rect: Rect): Ent[] {
    const out: Ent[] = [];
    this.queryBuckets(rect, (e) => {
      if (e.x1 >= rect[0] && e.x1 <= rect[2] && e.z1 >= rect[1] && e.z1 <= rect[3]) out.push(e);
    });
    return out;
  }

  /** The display name of a player (`#id` while its name has not arrived). */
  playerName(id: number): string {
    return this.names.get(id) ?? `#${String(id)}`;
  }

  /** All entities of one kind (a list, unordered). */
  ofKind(kind: number): Ent[] {
    const out: Ent[] = [];
    for (const e of this.byId.values()) if (e.kind === kind) out.push(e);
    return out;
  }
}

/** Converts the messages of `gen/proto/admin` (`bigint` ticks, `moves` optional) into the store's frame type. */
export function frameOf(
  keyframe: boolean,
  m: {
    tick: bigint;
    tier: number;
    degraded: boolean;
    binM?: number;
    entities?: readonly SpawnRec[];
    spawns?: readonly SpawnRec[];
    moves?: MovesRec | undefined;
    removes?: readonly number[];
    bins: readonly BinRec[];
    names: readonly { id: number; name: string }[];
  },
): FrameData {
  return {
    keyframe,
    tick: Number(m.tick),
    tier: m.tier,
    degraded: m.degraded,
    binM: m.binM ?? 0,
    spawns: (keyframe ? m.entities : m.spawns) ?? [],
    moves: m.moves,
    removes: m.removes ?? [],
    bins: m.bins,
    names: m.names,
  };
}

/** `true` for the kinds that move (players, monsters, NPCs, pets): the others are drawn without the slide. */
export function isMover(kind: number): boolean {
  return kind === Kind.PLAYER || kind === Kind.MONSTER || kind === Kind.NPC || kind === Kind.PET;
}
