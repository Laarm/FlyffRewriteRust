// The `SET_PARAMS` of a mover from what the client knows (Plan/40 section 7.4 "SimMoverParams"): the movers bundle row (kind,
// ai, rank, speed, frame factor), the spawn data (mode, level, HP, scale, speed factor), the equipment (weapon stance and
// ride through the items bundle) and the `FLMB` entry indices of the model files (the sim computes the bounds box).
// DST-derived fields (`speed_adj_pct`, `chr_state`, `hawkeye_rate`, attack speed, ...) keep their defaults until the
// rules context has answered for the mover (`setDerived`, WP-CLIENT-30: the rows of `drain_sync`, game/rules/sync.ts).
//
// A mover is marked dirty by the handlers and the records are written once per drain (`flush`), at most one per mover per
// iteration. Nothing is written while the `model-bounds` bundle or the sim's `FLMB` table is missing (the sim rejects an unknown
// bounds entry): the mover stays dirty and `flush` runs again when they are in.

import type { ClientDataIndex } from '../../data/clientData';
import type { DrainRow } from '../../rules';
import { HD_TWO } from '../../gen/defs/attribute';
import { OT_MOVER } from '../../gen/defs/define';
import { PARTS_RIDE, PARTS_RWEAPON } from '../../gen/defs/neuz';
import {
  JOB_PSYCHIKEEPER,
  JOB_PSYCHIKEEPER_HERO,
  JOB_PSYCHIKEEPER_MASTER,
} from '../../gen/defs/job';
import { MI_CROWNBUBBLE, MI_CROWNIBLIS, MI_CROWNSHADE, MI_DWARPETMAS } from '../../gen/defs/obj';
import { AII_EGG, AII_MONSTER, AII_PET } from '../../gen/defs/neuz';
import { KIND } from '../../gen/sim/wire';
import {
  EntityKind,
  type EntityRecord,
  type EntityTable,
  type MoverVisualState,
} from '../entities';
import {
  BARE_HANDS,
  boundsEntriesFor,
  boundsIndexFromFiles,
  defaultParams,
  NO_BOUNDS,
  type BoundsIndex,
  type SimMoverParams,
  type SimMoverKind,
  type WeaponStanceParams,
} from '../moverParams';
import type { SimBridge } from '../ports';

/** The data the builder reads (a `ClientDataIndex` satisfies it). */
export type ParamsData = Pick<ClientDataIndex, 'mover' | 'item' | 'model' | 'bundle'>;

/** Which `.o3d` files a player's current look loads (the renderer's part assembly, Plan/41 12.3). */
export interface PartFilesSource {
  filesOf(visual: MoverVisualState): readonly string[];
}

/** Fallback while the renderer's assembly is not available: the default body and the hair and head of the sex. */
export const DEFAULT_PART_FILES: PartFilesSource = {
  filesOf(v) {
    const g = v.sex === 0 ? 'male' : 'female';
    const pad = (n: number): string => String(n + 1).padStart(2, '0');
    return [
      `model/part_${g}upper.o3d`,
      `model/part_${g}lower.o3d`,
      `model/part_${g}hand.o3d`,
      `model/part_${g}foot.o3d`,
      `model/part_${g}hair${pad(v.hairMesh)}.o3d`,
      `model/part_${g}head${pad(v.headMesh)}.o3d`,
    ];
  },
};

const MASTER_MODELS: ReadonlySet<number> = new Set([
  MI_CROWNIBLIS,
  MI_CROWNSHADE,
  MI_CROWNBUBBLE,
  MI_DWARPETMAS,
]);
const PSY_JOBS: ReadonlySet<number> = new Set([
  JOB_PSYCHIKEEPER,
  JOB_PSYCHIKEEPER_MASTER,
  JOB_PSYCHIKEEPER_HERO,
]);

/** `KIND` of a mover record (`PlayerOther`, a monster, an NPC with a `character_key`, a pet). */
export function simKindOf(rec: EntityRecord, data: ParamsData | null): SimMoverKind {
  const v = rec.visual;
  if (!v || v.kind === 'player') return KIND.PLAYER;
  if (v.kind === 'pet') return KIND.PET;
  if (v.kind === 'npc') return KIND.NPC;
  const ai = data?.mover(rec.propId)?.ai;
  return ai === AII_PET || ai === AII_EGG ? KIND.PET : KIND.MONSTER;
}

/** The job of a player record (`PlayerPublic.job`), `-1` for movers without one. */
function jobOf(rec: EntityRecord): number {
  const s = rec.spawn;
  return 'player' in s && s.player ? s.player.job : -1;
}

/** The weapon stance of a player from the right-hand item (`GetActiveHandItemProp`, Plan/31 12.3). */
function stanceOf(
  rec: EntityRecord,
  visual: MoverVisualState,
  data: ParamsData | null,
): WeaponStanceParams {
  const weapon = visual.equip.get(PARTS_RWEAPON);
  const prop = weapon ? data?.item(weapon.itemId) : undefined;
  const job = jobOf(rec);
  const flags = {
    masterModel: MASTER_MODELS.has(visual.moverIndex),
    psyJob: PSY_JOBS.has(job),
  };
  if (!prop) return { ...BARE_HANDS, ...flags };
  return {
    kind3: prop.item_kind3,
    twoHanded: prop.handed === HD_TWO,
    dual: false,
    ...flags,
  };
}

/** The ridden item of a flying player (`PARTS_RIDE`). */
function rideOf(visual: MoverVisualState, data: ParamsData | null): SimMoverParams['ride'] {
  const ride = visual.equip.get(PARTS_RIDE);
  if (!ride) return null;
  const prop = data?.item(ride.itemId);
  if (!prop) return null;
  return {
    item: ride.itemId,
    flightSpeed: prop.flight_speed,
    lrAngle: prop.flight_lr_angle,
    tbAngle: prop.flight_tb_angle,
    useMotion: prop.use_motion,
    fuelReMax: prop.f_fuel_re_max,
  };
}

/** Writes the DST-derived row of the rules context into the parameters (`jumping` is the own character's only). */
export function applyDerived(p: SimMoverParams, row: DrainRow, own: boolean): void {
  p.speedAdjPct = row.speedAdjPct;
  p.speedChgSet = row.speedChgSet;
  p.chrState = row.chrState;
  p.hawkeyeRate = row.hawkeyeRate;
  if (own) p.jumping = row.jumping;
  p.attackSpeed = row.attackSpeed;
  p.castingAniSpeed = row.castingAniSpeed;
}

/**
 * The parameters of a mover record, `boundsEntries` left at the default (the manager fills them). Fields without a
 * source keep `defaultParams(kind)`; `derived` is the row the rules context returned for the mover (Plan/40 section 7.10).
 */
export function buildMoverParams(
  rec: EntityRecord,
  data: ParamsData | null,
  derived?: DrainRow,
): SimMoverParams {
  const kind = simKindOf(rec, data);
  const p = defaultParams(kind);
  const visual = rec.visual;
  const prop = data?.mover(rec.propId);
  p.moverIndex = rec.propId;
  p.modelOt = OT_MOVER;
  p.modelIndex = rec.propId;
  p.hpPositive = rec.hp > 0;
  if (prop) {
    p.ai = prop.ai;
    p.flyingNpc = kind !== KIND.PLAYER && prop.flying === 1;
    p.rank = prop.class;
    p.killable = prop.killable !== 0;
    p.baseSpeed = prop.ai === AII_MONSTER ? prop.speed / 2 : prop.speed;
    p.frameFactor = prop.frame < 0 ? 1 : prop.frame;
    p.level = Math.max(1, prop.level);
    p.propFlying = prop.flying;
  }
  if (visual) {
    p.mode = visual.mode;
    p.scale = visual.scale;
    if (kind === KIND.PLAYER) {
      const s = rec.spawn;
      if ('player' in s && s.player) p.level = Math.max(1, s.player.level);
      p.stance = stanceOf(rec, visual, data);
      p.ride = rideOf(visual, data);
    }
  }
  const spawn = rec.spawn;
  if ('npc' in spawn && spawn.npc && spawn.npc.speedFactor > 0)
    p.speedFactor = spawn.npc.speedFactor;
  p.isPet = kind === KIND.PET;
  // a local system pet follows its owner inside the sim (`flyff_sim::client::local_pet`, WP-CLIENT-31)
  if (rec.kind === EntityKind.LocalPet && rec.petOwner !== undefined) p.petOwner = rec.petOwner;
  if (derived) applyDerived(p, derived, rec.isSelf);
  return p;
}

/** The model files whose boxes the sim unions for this mover (`FLMB` rows, Plan/24 section 10.6). */
export function boundsFilesOf(
  rec: EntityRecord,
  data: ParamsData | null,
  parts: PartFilesSource,
): readonly string[] {
  const v = rec.visual;
  if (v && (v.kind === 'player' || (v.kind === 'npc' && data?.mover(rec.propId)?.if_parts === 1))) {
    return parts.filesOf(v);
  }
  const file = data?.model(OT_MOVER, rec.propId)?.file;
  return file ? [file] : [];
}

/** The path to row index map of the `model-bounds` bundle (rebuilt when the bundle object changes). */
export function boundsIndexOf(
  data: ParamsData | null,
  cache: { for: unknown; index: BoundsIndex },
): BoundsIndex {
  const bundle = data?.bundle('model-bounds');
  if (!bundle) return NO_BOUNDS;
  if (cache.for !== bundle) {
    const files = bundle.files;
    const at = files.columns.indexOf('file');
    cache.index = boundsIndexFromFiles(
      files.rows.map((row) => {
        const f = row[at];
        return typeof f === 'string' ? f : '';
      }),
    );
    cache.for = bundle;
  }
  return cache.index;
}

export interface MoverParamsManagerDeps {
  readonly entities: Pick<EntityTable, 'get'>;
  readonly sim: Pick<SimBridge, 'cmd' | 'modelBoundsLoaded'>;
  readonly data: () => ParamsData | null;
  /** The renderer's part assembly (default: the body of the sex). */
  readonly parts?: () => PartFilesSource;
}

/** Collects the movers whose parameters changed and writes one `SET_PARAMS` each. */
export class MoverParamsManager {
  private readonly dirty = new Set<number>();
  /** The last row of the rules context per mover (the DST-derived parameters). */
  private readonly derived = new Map<number, DrainRow>();
  private readonly boundsCache: { for: unknown; index: BoundsIndex } = {
    for: null,
    index: NO_BOUNDS,
  };
  /** `SET_PARAMS` records written so far (tests, probe). */
  written = 0;

  constructor(private readonly deps: MoverParamsManagerDeps) {}

  mark(id: number): void {
    this.dirty.add(id);
  }

  /** The rules context answered for `id`: its DST-derived parameters; the record is written by the next `flush`. */
  setDerived(id: number, row: DrainRow): void {
    this.derived.set(id, row);
    this.dirty.add(id);
  }

  /** The derived row of `id`, `undefined` before the rules answered. */
  derivedOf(id: number): DrainRow | undefined {
    return this.derived.get(id);
  }

  forget(id: number): void {
    this.dirty.delete(id);
    this.derived.delete(id);
  }

  clear(): void {
    this.dirty.clear();
    this.derived.clear();
  }

  get pending(): number {
    return this.dirty.size;
  }

  /** Writes the records of every dirty mover that can be written now; returns how many. */
  flush(): number {
    return this.write(null);
  }

  /**
   * Writes the record of ONE mover now when it is dirty (the combat handlers need a changed attack speed in the command buffer
   * BEFORE the `ACT` that starts the swing); returns whether a record was written.
   */
  flushOne(id: number): boolean {
    return this.write(id) > 0;
  }

  private write(only: number | null): number {
    if (this.dirty.size === 0 || !this.deps.sim.modelBoundsLoaded) return 0;
    const data = this.deps.data();
    const index = boundsIndexOf(data, this.boundsCache);
    if (index.size === 0) return 0;
    const parts = this.deps.parts?.() ?? DEFAULT_PART_FILES;
    let n = 0;
    for (const id of only === null ? [...this.dirty] : this.dirty.has(only) ? [only] : []) {
      const rec = this.deps.entities.get(id);
      if (!rec || rec.removed || !rec.simHandle) {
        this.dirty.delete(id);
        continue;
      }
      const params = buildMoverParams(rec, data, this.derived.get(id));
      const entries = boundsEntriesFor(index, boundsFilesOf(rec, data, parts));
      if (!entries) continue;
      params.boundsEntries = entries;
      this.deps.sim.cmd.setParams(id, params);
      this.dirty.delete(id);
      n += 1;
    }
    this.written += n;
    return n;
  }
}
