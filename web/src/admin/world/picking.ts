// Picking on the map (Plan/25 section 3.9): the entity under a pointer and the entities inside a dragged box, found through the 64 m
// spatial hash of the store (never a scan of all entities). Hit radii are in CSS pixels so that a click means the same at every zoom.
import type { Ent, EntityStore } from './entities';
import { BUCKET_M } from './entities';
import { isBossRank, Kind, Rank } from './types';
import type { Rect } from './view';

/** Hit radius in CSS pixels per kind (a boss marker is larger). */
export function hitRadiusPx(e: Pick<Ent, 'kind' | 'sub'>): number {
  switch (e.kind) {
    case Kind.PLAYER:
      return 11;
    case Kind.MONSTER:
      return isBossRank(e.sub) ? 16 : e.sub === Rank.CAPTAIN ? 10 : 8;
    case Kind.NPC:
      return 9;
    case Kind.PET:
      return 7;
    case Kind.ITEM:
      return 7;
    default:
      return 8;
  }
}

/** Stacking order for a tie: the entity drawn on top wins (players over monsters over NPCs over the rest). */
export function kindPriority(kind: number): number {
  switch (kind) {
    case Kind.PLAYER:
      return 0;
    case Kind.MONSTER:
      return 1;
    case Kind.NPC:
      return 2;
    case Kind.PET:
      return 3;
    case Kind.CTRL:
      return 4;
    default:
      return 5;
  }
}

export interface PickOptions {
  /** World units per CSS pixel. */
  readonly upp: number;
  /** The client clock: displayed positions are placed at this time. */
  readonly now: number;
  /** Hidden kinds, filtered-out entities: `false` skips the entity. */
  readonly visible?: (e: Ent) => boolean;
}

/** The entity nearest to the world point (x, z) within its hit radius (the score is the distance in units of the radius), or `null`. */
export function pickAt(store: EntityStore, x: number, z: number, o: PickOptions): Ent | null {
  // the largest hit radius is 16 px; the hash is queried with that reach
  const reach = 16 * o.upp + BUCKET_M;
  let best: Ent | null = null;
  let bestScore = Infinity;
  store.queryBuckets([x - reach, z - reach, x + reach, z + reach], (e) => {
    if (o.visible && !o.visible(e)) return;
    store.place(e, o.now);
    const dx = (e.x - x) / o.upp;
    const dz = (e.z - z) / o.upp;
    const r = hitRadiusPx(e);
    const d2 = dx * dx + dz * dz;
    if (d2 > r * r) return;
    // a small bias keeps the higher kind on top when two markers overlap exactly
    const score = Math.sqrt(d2) / r + kindPriority(e.kind) * 1e-3;
    if (score < bestScore) {
      best = e;
      bestScore = score;
    }
  });
  return best;
}

/** The entities whose displayed position is inside `rect`, players first, then by id. */
export function pickBox(store: EntityStore, rect: Rect, o: PickOptions): Ent[] {
  const out: Ent[] = [];
  store.queryBuckets(rect, (e) => {
    if (o.visible && !o.visible(e)) return;
    store.place(e, o.now);
    if (e.x >= rect[0] && e.x <= rect[2] && e.z >= rect[1] && e.z <= rect[3]) out.push(e);
  });
  out.sort((a, b) => kindPriority(a.kind) - kindPriority(b.kind) || a.id - b.id);
  return out;
}

/** The normalised rectangle of a drag from (ax, az) to (bx, bz). */
export function dragRect(ax: number, az: number, bx: number, bz: number): Rect {
  return [Math.min(ax, bx), Math.min(az, bz), Math.max(ax, bx), Math.max(az, bz)];
}
