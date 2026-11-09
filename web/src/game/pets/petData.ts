// The pet tables of the client (`CPetProperty`, `CPet`; `src:_Common/pet.cpp`, `pet.h`; Plan/19 §13; WP-CLIENT-31, WP-CFEAT-27): pure functions over the `pet` section of
// the `item-ext` bundle. Pet levels are `PL_EGG 0, D 1, C 2, B 3, A 4, S 5`; a pet's stat bonus per level is the table value picked by its `avail_level`
// (1..9) of that level.

import type { PetTablesDef } from '../../gen/bundles';
import { II_PET_EGG } from '../../gen/defs/item';
import { MI_PET_EGG } from '../../gen/defs/obj';

/** `PETLEVEL`. */
export const PL_EGG = 0;
export const PL_D = 1;
export const PL_C = 2;
export const PL_B = 3;
export const PL_A = 4;
export const PL_S = 5;
/** `PF_*`: why the server sent the whole pet (`Pet.flag`). */
export const PF_PET = 0;
export const PF_PET_LEVEL_UP = 1;
export const PF_PET_LEVEL_DOWN = 2;
export const PF_PET_GET_AVAIL = 3;
/** `MAX_PET_EGG_EXP` / `MAX_PET_EXP`. */
export const MAX_PET_EGG_EXP = 50_000;
export const MAX_PET_EXP = 100_000;
/** `PK_MAX`: the number of pet kinds (`0..6`). */
export const PK_MAX = 7;
/** `MAX_PET_AVAIL_LEVEL`.
export const MAX_PET_AVAIL_LEVEL = 9;

/** The data of a pet that the tables read (the fields of the wire `PetData`). */
export interface PetLike {
  readonly kind: number;
  readonly level: number;
  readonly exp?: number;
  readonly energy?: number;
  readonly life?: number;
  readonly availLevel?: readonly number[];
  readonly name?: string;
}

export type PetTables = Pick<PetTablesDef, 'avail' | 'max_energy'>;

/** `MoverProp` id of the model of a pet (`CPet::GetIndex`): the egg model, else one of the three shapes of its kind; `null` for a kind without a table row. */
export function petMoverIndex(
  kind: number,
  level: number,
  tables: PetTables | undefined,
): number | null {
  if (level === PL_EGG) return MI_PET_EGG;
  const row = tables?.avail[kind];
  if (!row) return null;
  const shape = level < PL_B ? 0 : level < PL_S ? 1 : 2;
  return row.movers[shape] ?? null;
}

/** `CPet::GetItemId`: the egg item, else the pet item of its kind (`null` for an unknown kind). */
export function petItemId(
  kind: number,
  level: number,
  tables: PetTables | undefined,
): number | null {
  if (level === PL_EGG) return II_PET_EGG;
  return tables?.avail[kind]?.item ?? null;
}

/** `CPet::GetMaxEnergy` (`CPetProperty::GetMaxEnergy`): `MaxEnergy` of the level D..S, `1` for an egg (no division by zero). */
export function petMaxEnergy(level: number, tables: PetTables | undefined): number {
  if (level >= PL_D && level <= PL_S) return Math.max(1, tables?.max_energy[level - 1] ?? 1);
  return 1;
}

/** `CPet::GetMaxExp`. */
export function petMaxExp(level: number): number {
  return level === PL_EGG ? MAX_PET_EGG_EXP : MAX_PET_EXP;
}

/** `CPet::GetAvailLevel(level)`: the 1..9 pick of a level (`0` = none yet). */
export function petAvailLevel(pet: PetLike, level: number): number {
  if (level < PL_D || level > PL_S) return 0;
  return pet.availLevel?.[level - 1] ?? 0;
}

/** The stat bonus of ONE level: the table value of its pick, `0` without a pick or a table row. */
export function petAvailValueAt(
  pet: PetLike,
  level: number,
  tables: PetTables | undefined,
): number {
  const pick = petAvailLevel(pet, level);
  if (pick < 1) return 0;
  return tables?.avail[pet.kind]?.values[pick - 1] ?? 0;
}

/** `CPet::GetAvailDestParam`: the `DST_*` of the kind and the sum of the bonuses of the levels D..level (`dst = 0` for an unknown kind). */
export function petAvailDest(
  pet: PetLike,
  tables: PetTables | undefined,
): { dst: number; value: number } {
  const row = tables?.avail[pet.kind];
  if (!row) return { dst: 0, value: 0 };
  let value = 0;
  for (let l = PL_D; l <= pet.level; l += 1) value += petAvailValueAt(pet, l, tables);
  return { dst: row.dst, value };
}

/**
 * The icon tier of a pet level (`CItemBase::SetTexture`, Item.cpp:80-117): `PL_D` / `PL_C` -> 0 (`_00`), `PL_B` / `PL_A` -> 1
 * (`_01`), `PL_S` -> 2 (`_02`); an egg, an unknown level or no pet record -> `null` (the base icon).
 */
export function petIconTier(level: number | undefined): 0 | 1 | 2 | null {
  if (level === PL_D || level === PL_C) return 0;
  if (level === PL_B || level === PL_A) return 1;
  if (level === PL_S) return 2;
  return null;
}

/**
 * The icon file of an `IK3_EGG` item instance (`CItemBase::SetTexture`, Item.cpp:80-117): `_0<tier>` goes in before the LAST `.` of
 * `icon` (`Itm_PetNineFox01.dds` -> `Itm_PetNineFox01_01.dds`), the tier from the pet's `level`. Only `II_PET_EGG` has a base icon
 * file; the seven hatched kinds ship `_00`, `_01` and `_02` only. Without a tier (level 0, no pet record) `II_PET_EGG` keeps its base
 * icon and any other `IK3_EGG` item takes tier 0 (`WndFTool.cpp:170-191`, `dwID != II_PET_EGG`): a small, intended deviation, the
 * original would show the missing base file (nothing) for a GM-created hatched-kind item without a pet. A name without a dot is
 * returned unchanged.
 */
export function petItemIconFile(icon: string, itemId: number, level: number | undefined): string {
  const tier = petIconTier(level) ?? (itemId === II_PET_EGG ? null : 0);
  if (tier === null) return icon;
  const dot = icon.lastIndexOf('.');
  if (dot < 0) return icon;
  return `${icon.slice(0, dot)}_0${String(tier)}${icon.slice(dot)}`;
}
