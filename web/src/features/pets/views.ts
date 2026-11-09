// The read models of the pet windows (Plan/45 §18.7; WP-CFEAT-27): pure functions of the pet item, the pet tables and the texts. The shells never see a protobuf
// message or a table: they get `PetStatusView`, `BuffPetView` and the picks of the chance window.

import {
  DST_ADJDEF,
  DST_ATKPOWER,
  DST_DEX,
  DST_HP_MAX,
  DST_INT,
  DST_STA,
  DST_STR,
} from '../../gen/defs/attribute';
import {
  TID_GAME_PET_EGG_ABILITY,
  TID_GAME_PETGRADE_A,
  TID_GAME_PETGRADE_B,
  TID_GAME_PETGRADE_C,
  TID_GAME_PETGRADE_D,
  TID_GAME_PETGRADE_E,
  TID_GAME_PETGRADE_S,
  TID_TOOLTIP_ATKPOWER_VALUE,
  TID_TOOLTIP_DEFENCE,
  TID_TOOLTIP_DEX,
  TID_TOOLTIP_DST_HP_MAX,
  TID_TOOLTIP_HP,
  TID_TOOLTIP_INT,
  TID_TOOLTIP_STA,
  TID_TOOLTIP_STR,
} from '../../gen/defs/text';
import {
  petAvailDest,
  petAvailLevel,
  petAvailValueAt,
  petMaxEnergy,
  petMaxExp,
  PL_A,
  PL_B,
  PL_C,
  PL_D,
  PK_MAX,
  PL_EGG,
  PL_S,
  type PetLike,
  type PetTables,
} from '../../game/pets/petData';
import type { BuffPetView, PetStatusPick, PetStatusView } from '../../ui/windows/pets/ports';
import { NeedVis, validTable, type VisNeeds } from './vis';

/** `TID_GAME_PETGRADE_x` of a level (`DrawPetInformation`: the egg is E; the ids are named by the grade, not by the level order). */
export function gradeTid(level: number): number {
  switch (level) {
    case PL_D:
      return TID_GAME_PETGRADE_D;
    case PL_C:
      return TID_GAME_PETGRADE_C;
    case PL_B:
      return TID_GAME_PETGRADE_B;
    case PL_A:
      return TID_GAME_PETGRADE_A;
    case PL_S:
      return TID_GAME_PETGRADE_S;
    default:
      return TID_GAME_PETGRADE_E;
  }
}

/** The stat name of a `DST_*` in the information block (`dwTooltip` of `DrawPetInformation`), `0` for a stat the original does not name. */
export function statTid(dst: number, forTooltip = false): number {
  switch (dst) {
    case DST_STR:
      return TID_TOOLTIP_STR;
    case DST_DEX:
      return TID_TOOLTIP_DEX;
    case DST_INT:
      return TID_TOOLTIP_INT;
    case DST_STA:
      return TID_TOOLTIP_STA;
    case DST_ATKPOWER:
      return TID_TOOLTIP_ATKPOWER_VALUE;
    case DST_ADJDEF:
      return TID_TOOLTIP_DEFENCE;
    case DST_HP_MAX:
      // the information block says "HP", the tooltip of the strip "Max HP"
      return forTooltip ? TID_TOOLTIP_DST_HP_MAX : TID_TOOLTIP_HP;
    default:
      return 0;
  }
}

export interface StatusEnv {
  readonly tables: PetTables | undefined;
  readonly text: (tid: number) => string;
  /** The name of the item (`GetProp()->szName`) and of a mover (`MoverProp.szName`). */
  readonly itemName: (itemId: number) => string;
  readonly moverName: (moverIndex: number) => string;
  /** `features.no_pet_life`. */
  readonly noPetLife: boolean;
  /** `MoverProp` id of the pet's model, `null` for an unknown kind. */
  readonly moverIndex: (pet: PetLike) => number | null;
}

/** The title: the pet's own name, else the mover name of a hatched pet, else the item name (`PaintFrame`). */
export function petTitle(itemId: number, pet: PetLike, env: StatusEnv): string {
  if (pet.name !== undefined && pet.name !== '') return pet.name;
  if (pet.level > PL_EGG) {
    const idx = env.moverIndex(pet);
    const n = idx === null ? '' : env.moverName(idx);
    if (n !== '') return n;
  }
  return env.itemName(itemId);
}

/** `"<stat> +<n>"` of the summed bonus, the egg text for a pet without one. */
export function abilityText(pet: PetLike, env: StatusEnv): string {
  const { dst, value } = petAvailDest(pet, env.tables);
  const tid = statTid(dst);
  if (dst !== 0 && value !== 0 && tid !== 0) return `${env.text(tid)} +${String(value)}`;
  return env.text(TID_GAME_PET_EGG_ABILITY);
}

/** The levels D..current that have a pick, in level order. */
export function picksOf(pet: PetLike): PetStatusPick[] {
  const out: PetStatusPick[] = [];
  for (let l = PL_D; l <= pet.level; l += 1) {
    const pick = petAvailLevel(pet, l);
    if (pick !== 0) out.push({ level: l, pick });
  }
  return out;
}

/** The tooltip of the pick of a level (`"<stat> +<value>"`). */
export function pickTip(pet: PetLike, level: number, env: StatusEnv): string {
  const dst = env.tables?.avail[pet.kind]?.dst ?? 0;
  const tid = statTid(dst, true);
  return `${tid === 0 ? '' : env.text(tid)} +${String(petAvailValueAt(pet, level, env.tables))}`;
}

/** `CWndPetStatus` view of the summoned pet. */
export function petStatusView(
  itemId: number,
  pet: PetLike,
  subject: unknown,
  env: StatusEnv,
): PetStatusView {
  return {
    title: petTitle(itemId, pet, env),
    grade: env.text(gradeTid(pet.level)),
    ability: abilityText(pet, env),
    life: env.noPetLife ? null : pet.level === PL_EGG ? 0 : (pet.life ?? 0),
    energy: pet.energy ?? 0,
    energyMax: petMaxEnergy(pet.level, env.tables),
    exp: pet.exp ?? 0,
    expMax: petMaxExp(pet.level),
    picks: picksOf(pet),
    subject,
    camera: pet.level === PL_EGG ? null : Math.max(0, Math.min(PK_MAX - 1, pet.kind)),
  };
}

/** The picks (1..9) of the last two levels of a B / A / S pet for the chance window; `null` for any other pet. */
export function miraclePicks(pet: PetLike | undefined): readonly [number, number] | null {
  if (!pet) return null;
  if (pet.level !== PL_B && pet.level !== PL_A && pet.level !== PL_S) return null;
  return [petAvailLevel(pet, pet.level - 1), petAvailLevel(pet, pet.level)];
}

export interface BuffEnv {
  readonly needs: VisNeeds;
  /** The icon file of a charm item (`item/itm_x.dds`), `null` = none. */
  readonly icon: (itemId: number) => string | null;
}

/** `CWndBuffPetStatus`: the sockets of the vis pet item (`cards` are the charm ids, `0` = empty). */
export function buffPetView(
  cards: readonly number[],
  title: string,
  subject: unknown,
  env: BuffEnv,
): BuffPetView {
  const valid = validTable(cards, env.needs);
  return {
    title,
    size: cards.length,
    slots: cards.map((vis, i) => ({
      vis,
      icon: vis === 0 ? null : env.icon(vis),
      valid: valid[i] === NeedVis.Success,
    })),
    subject,
  };
}
