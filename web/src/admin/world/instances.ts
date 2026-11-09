// The instance buffers of a frame (Plan/25 section 3.5, 3.7 "client-side culling"): the visible 64 m buckets of the spatial hash are walked, the
// entities inside the canvas plus a 64 px margin are placed (interpolated), styled and written as sprite instances; nothing outside the view is
// touched, interpolated or labelled. Pure: no GL, no DOM, so `instances` of `__adminMapStats` is a number the tests can predict.
import type { Ent, EntityStore } from './entities';
import { cellUv } from './spriteAtlas';
import {
  Sprite,
  binSize,
  pulseFrame,
  sizeScale,
  styleOf,
  type Rgb,
  type Style,
  type StyleContext,
  COLOR,
} from './style';
import { headingDegrees } from './interp';
import { Kind, TierId } from './types';
import { DRAW_MARGIN_PX, viewRect, type Rect, type View } from './view';
import type { Counts } from './state';
import type { InstanceBuffers } from './gl';

/** Instances may need up to this many per entity (a ring under the sprite). */
const PER_ENTITY = 2;

export interface InstanceInput {
  readonly store: EntityStore;
  readonly view: View;
  readonly now: number;
  /** `false` hides the entity (a kind switched off, a rank or state filtered out). */
  readonly visible: (e: Ent) => boolean;
  /** `true` dims the entity (the text filter does not match). `null` = no filter. */
  readonly dim: ((e: Ent) => boolean) | null;
  readonly style: StyleContext;
  readonly selected: ReadonlySet<number>;
  readonly followId: number | null;
  readonly hoverId: number | null;
  /** Draw the tier-C bins (monsters aggregated in 128 m cells). */
  readonly showBins: boolean;
  /** Dim the bins (a text filter is active: bins cannot be tested against it). */
  readonly dimBins: boolean;
}

export interface InstanceResult {
  /** Instances written. */
  readonly count: number;
  /** Entities placed inside the canvas plus the margin (what `__adminMapStats.instances` counts besides bins and decorations). */
  readonly drawn: number;
  /** Entities inside the canvas itself, by kind (the status bar). */
  readonly counts: Counts;
  /** Bins drawn. */
  readonly bins: number;
  /** The entities inside the canvas, in a stable order (players first): the candidates for labels and lists. */
  readonly inCanvas: readonly Ent[];
  /** Monsters represented by the bins inside the canvas (tier C). */
  readonly binMonsters: number;
}

const UV: readonly (readonly [number, number, number, number])[] = Array.from(
  { length: 16 },
  (_, i) => cellUv(i),
);

function uvOf(id: number): readonly [number, number, number, number] {
  return UV[id] ?? UV[0] ?? [0, 0, 1, 1];
}

/** Writes one instance. */
function put(
  b: InstanceBuffers,
  n: number,
  x: number,
  z: number,
  size: number,
  rot: number,
  sprite: number,
  c: Rgb,
  alpha: number,
): void {
  const i = n * 4;
  b.pos[i] = x;
  b.pos[i + 1] = z;
  b.pos[i + 2] = size;
  b.pos[i + 3] = rot;
  const uv = uvOf(sprite);
  b.uv[i] = uv[0];
  b.uv[i + 1] = uv[1];
  b.uv[i + 2] = uv[2];
  b.uv[i + 3] = uv[3];
  b.color[i] = c[0];
  b.color[i + 1] = c[1];
  b.color[i + 2] = c[2];
  b.color[i + 3] = alpha;
}

const DEG = Math.PI / 180;
/** Draw order of the kinds (first = underneath). */
const ORDER = [Kind.ITEM, Kind.CTRL, Kind.PET, Kind.NPC, Kind.MONSTER, Kind.PLAYER] as const;

const styleCache = new Map<number, Style>();

function cachedStyle(e: Ent, ctx: StyleContext): Style {
  // a player's colour depends on who is followed; everything else only on its kind, rank and flags
  if (e.kind === Kind.PLAYER) return styleOf(e, ctx);
  const key = e.kind | (e.sub << 3) | (e.flags << 11) | ((ctx.dimAsleep === false ? 1 : 0) << 25);
  let s = styleCache.get(key);
  if (!s) {
    s = styleOf(e, ctx);
    styleCache.set(key, s);
  }
  return s;
}

/** Fills the buffers for one frame. `ensure(n)` grows them (the arrays may be replaced: the returned buffers are the ones written). */
export function buildInstances(
  input: InstanceInput,
  ensure: (instances: number) => InstanceBuffers,
): { result: InstanceResult; buffers: InstanceBuffers } {
  const { store, view, now } = input;
  const draw: Rect = viewRect(view, DRAW_MARGIN_PX);
  const inner: Rect = viewRect(view, 0);
  const lists: Ent[][] = [[], [], [], [], [], [], []];
  const counts = { players: 0, monsters: 0, npcs: 0, pets: 0, items: 0, ctrls: 0 };
  const inCanvas: Ent[] = [];
  let drawn = 0;
  store.queryBuckets(draw, (e) => {
    if (!input.visible(e)) return;
    store.place(e, now);
    if (e.x < draw[0] || e.x > draw[2] || e.z < draw[1] || e.z > draw[3]) return;
    lists[e.kind]?.push(e);
    drawn += 1;
    if (e.x >= inner[0] && e.x <= inner[2] && e.z >= inner[1] && e.z <= inner[3]) {
      inCanvas.push(e);
      switch (e.kind) {
        case Kind.PLAYER:
          counts.players += 1;
          break;
        case Kind.MONSTER:
          counts.monsters += 1;
          break;
        case Kind.NPC:
          counts.npcs += 1;
          break;
        case Kind.PET:
          counts.pets += 1;
          break;
        case Kind.ITEM:
          counts.items += 1;
          break;
        default:
          counts.ctrls += 1;
      }
    }
  });
  const bins = input.showBins && store.tier === TierId.C ? store.bins : [];
  const need = drawn * PER_ENTITY + bins.length * 2 + input.selected.size + 8;
  const b = ensure(need);
  const scale = sizeScale(view.upp);
  let n = 0;

  // the selection pulses and the follow ring lie under everything
  const frame = Sprite.POS0 + pulseFrame(now);
  for (const id of input.selected) {
    const e = store.byId.get(id);
    if (!e) continue;
    store.place(e, now);
    put(b, n, e.x, e.z, 34, 0, frame, COLOR.selected, 0.95);
    n += 1;
  }
  if (input.followId !== null) {
    const e = store.byId.get(input.followId);
    if (e) {
      store.place(e, now);
      put(b, n, e.x, e.z, 30, 0, Sprite.RING, COLOR.followed, 1);
      n += 1;
    }
  }
  if (input.hoverId !== null && !input.selected.has(input.hoverId)) {
    const e = store.byId.get(input.hoverId);
    if (e) {
      put(b, n, e.x, e.z, 26, 0, Sprite.RING, COLOR.selected, 0.9);
      n += 1;
    }
  }

  // tier C: the monsters of a 128 m cell as one blob whose size grows with the count, the asleep ones as a small grey dot
  let binMonsters = 0;
  for (const bin of bins) {
    const x = (bin.bx + 0.5) * store.binM;
    const z = (bin.bz + 0.5) * store.binM;
    if (x < draw[0] || x > draw[2] || z < draw[1] || z > draw[3]) continue;
    const total = bin.normal + bin.captain;
    if (x >= inner[0] && x <= inner[2] && z >= inner[1] && z <= inner[3]) binMonsters += total;
    const alpha = input.dimBins ? 0.18 : 0.55;
    put(
      b,
      n,
      x,
      z,
      binSize(total) * scale,
      0,
      Sprite.BLOB,
      bin.captain > 0 ? COLOR.captain : COLOR.monster,
      alpha,
    );
    n += 1;
    if (bin.asleep > 0) {
      put(b, n, x, z, binSize(bin.asleep) * 0.5 * scale, 0, Sprite.DOT, COLOR.dead, alpha + 0.2);
      n += 1;
    }
  }

  for (const kind of ORDER) {
    const list = lists[kind];
    if (!list) continue;
    // bosses are drawn after the other monsters so that their star is on top
    if (kind === Kind.MONSTER) list.sort((a, c) => (isTop(a) ? 1 : 0) - (isTop(c) ? 1 : 0));
    for (const e of list) {
      const s = cachedStyle(e, input.style);
      const dim = input.dim?.(e) === true ? 0.18 : 1;
      const size = s.size * (kind === Kind.PLAYER ? Math.max(0.8, scale) : scale);
      if (s.ring !== null) {
        const pulse = s.pulse ? 0.7 + 0.3 * Math.sin(now / 160) : 1;
        put(b, n, e.x, e.z, (size + 8) * pulse, 0, s.ring, s.ringColor, s.alpha * dim);
        n += 1;
      }
      const rot = s.rotates ? headingDegrees(e.yaw) * DEG : 0;
      put(b, n, e.x, e.z, size, rot, s.sprite, s.color, s.alpha * dim);
      n += 1;
    }
  }
  return {
    buffers: b,
    result: { count: n, drawn, counts, bins: bins.length, inCanvas, binMonsters },
  };
}

function isTop(e: Ent): boolean {
  return e.sub >= 2 && e.sub <= 4;
}
