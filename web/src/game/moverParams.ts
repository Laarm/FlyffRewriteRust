// `SimMoverParams` (Plan/31 section 5.4, Plan/40 section 7.4): the inputs of the simulation that the owner writes and the
// sim never derives. This file holds the TypeScript shape, the deterministic defaults of `SimMoverParams::default_for`
// and the builder of the bundle-derived fields; the binary layout is `PARAMS_LAYOUT` of the generated `gen/sim/wire.ts`
// (`SimCmdWriter.setParams` encodes it). No gameplay formula is evaluated here (rule B1): DST-derived fields keep their
// defaults until WP-CLIENT-30 supplies them from `RulesContext`, and the `bounds` box is computed inside wasm from the
// `FLMB` entry indices this module chooses (no box arithmetic in TS, Plan/40 section 7.4 "bounds").

import { OT_MOVER } from '../gen/defs/define';
import { IK3_HAND, IK3_VIRTUAL } from '../gen/defs/itemkind';
import { MI_MALE } from '../gen/defs/obj';
import { AII_MONSTER, AII_MOVER, AII_NONE, AII_PET } from '../gen/defs/neuz';
import { RANK_NORMAL } from '../gen/defs/attribute';
import { KIND, MAX_BOUNDS_ENTRIES } from '../gen/sim/wire';

/** `MoverKind` of `ADD_MOVER` / `SET_PARAMS` (`wire.KIND`). */
export type SimMoverKind = (typeof KIND)[keyof typeof KIND];

/** The weapon stance of the active hand for the motion offsets of `SetMotion` (Plan/31 section 12.3). */
export interface WeaponStanceParams {
  /** `ItemProp.dwItemKind3` (`IK3_*`). */
  readonly kind3: number;
  /** `dwHanded == HD_TWO`. */
  readonly twoHanded: boolean;
  readonly dual: boolean;
  readonly masterModel: boolean;
  readonly psyJob: boolean;
}

/** The ridden item of a flying player (`PARTS_RIDE`). */
export interface RideParams {
  readonly item: number;
  readonly flightSpeed: number;
  readonly lrAngle: number;
  readonly tbAngle: number;
  readonly useMotion: number;
  readonly fuelReMax: number;
}

/** What the sim needs to know about one mover (the fields of `PARAMS_LAYOUT`, camelCase). */
export interface SimMoverParams {
  kind: SimMoverKind;
  moverIndex: number;
  ai: number;
  flyingNpc: boolean;
  rank: number;
  killable: boolean;
  baseSpeed: number;
  speedFactor: number;
  speedAdjPct: number;
  speedChgSet: boolean;
  jumping: number;
  chrState: number;
  mode: number;
  baseMotionMode: boolean;
  attackSpeed: number;
  castingAniSpeed: number;
  frameFactor: number;
  hpPositive: boolean;
  level: number;
  stance: WeaponStanceParams | null;
  ride: RideParams | null;
  modelOt: number;
  modelIndex: number;
  /** `FLMB` entry indices (1..=16); the sim unions the boxes. */
  boundsEntries: readonly number[];
  scale: number;
  hawkeyeRate: number;
  disguised: boolean;
  propFlying: number;
  isPet: boolean;
  petOwner: number;
}

/** `NO_OBJ` of the sim (`0xFFFF_FFFF`). */
export const NO_OBJ = 0xffff_ffff;

/** `IK3_HAND` stance of an unarmed player (`II_WEA_HAN_HAND`). */
export const BARE_HANDS: WeaponStanceParams = {
  kind3: IK3_HAND,
  twoHanded: false,
  dual: false,
  masterModel: false,
  psyJob: false,
};

/** `IK3_VIRTUAL`: the virtual item every monster and NPC holds. */
export const VIRTUAL_ITEM: WeaponStanceParams = {
  kind3: IK3_VIRTUAL,
  twoHanded: false,
  dual: false,
  masterModel: false,
  psyJob: false,
};

/** `[ai, base speed, mover index]` of `SimMoverParams::default_for` per kind. */
const PLAYER_DEFAULTS: readonly [number, number, number] = [AII_NONE, 0.1, MI_MALE];
const DEFAULTS: Readonly<Record<number, readonly [number, number, number]>> = {
  [KIND.PLAYER]: PLAYER_DEFAULTS,
  [KIND.MONSTER]: [AII_MONSTER, 0.075, 0],
  [KIND.NPC]: [AII_MOVER, 0.075, 0],
  [KIND.PET]: [AII_PET, 0.1, 0],
};

/** `SimMoverParams::default_for(kind)` (crates/flyff-sim/src/mover.rs); the bounds entry defaults to the first file. */
export function defaultParams(kind: SimMoverKind): SimMoverParams {
  const [ai, baseSpeed, moverIndex] = DEFAULTS[kind] ?? PLAYER_DEFAULTS;
  return {
    kind,
    moverIndex,
    ai,
    flyingNpc: false,
    rank: RANK_NORMAL,
    killable: true,
    baseSpeed,
    speedFactor: 1,
    speedAdjPct: 0,
    speedChgSet: false,
    jumping: 0,
    chrState: 0,
    mode: 0,
    baseMotionMode: false,
    attackSpeed: 1,
    castingAniSpeed: 1,
    frameFactor: 1,
    hpPositive: true,
    level: 1,
    stance: kind === KIND.PLAYER ? BARE_HANDS : VIRTUAL_ITEM,
    ride: null,
    modelOt: OT_MOVER,
    modelIndex: moverIndex,
    boundsEntries: [0],
    scale: 1,
    hawkeyeRate: 0,
    disguised: false,
    propFlying: 0,
    isPet: kind === KIND.PET,
    petOwner: NO_OBJ,
  };
}

/** `true` when `entries` is a valid `bounds_entries` set: 1..=16 non-negative integers. */
export function validBoundsEntries(entries: readonly number[]): boolean {
  return (
    entries.length >= 1 &&
    entries.length <= MAX_BOUNDS_ENTRIES &&
    entries.every((e) => Number.isInteger(e) && e >= 0)
  );
}

/** The `FLMB` entry index of a model file (`model-bounds` bundle row order), `null` when the file has no entry. */
export interface BoundsIndex {
  /** Number of files in the table (0 when the bundle is not loaded). */
  readonly size: number;
  /** Row index of a lower-case VFS path (`model/mvr_aibatt.o3d`). */
  entryOf(file: string): number | null;
}

/** An empty index (the `model-bounds` bundle is not loaded yet). */
export const NO_BOUNDS: BoundsIndex = { size: 0, entryOf: () => null };

/** Builds the path to row index map of the `model-bounds` bundle. */
export function boundsIndexFromFiles(files: readonly string[]): BoundsIndex {
  const map = new Map<string, number>();
  files.forEach((f, i) => {
    const key = f.toLowerCase();
    if (!map.has(key)) map.set(key, i);
  });
  return {
    size: files.length,
    entryOf: (file) => map.get(file.toLowerCase()) ?? null,
  };
}

/**
 * The bounds entries of a list of model files: unknown files are skipped, at most 16 are kept and the result is never
 * empty (a mover without any known file uses entry 0, which exists in every non-empty bundle). `null` while the
 * bundle is empty: the caller must not send `SET_PARAMS` then (an unknown entry is a `JsError`, Plan/31 17.4).
 */
export function boundsEntriesFor(index: BoundsIndex, files: readonly string[]): number[] | null {
  if (index.size === 0) return null;
  const out: number[] = [];
  for (const f of files) {
    const e = index.entryOf(f);
    if (e !== null && !out.includes(e)) out.push(e);
    if (out.length === MAX_BOUNDS_ENTRIES) break;
  }
  return out.length === 0 ? [0] : out;
}
