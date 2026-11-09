// Sections 1-6 of the item tooltip (Plan/45 §10.2; `CWndMgr::PutItemName`, `PutItemAbilityPiercing`, `PutPetKind`, the equip-bind
// state, `PutWeapon`, `PutSex`; `src:_Interface/WndManager.cpp:4627-4720, 6108-6122, 6275-6296, 7734-7757`).

import { HD_ONE, HD_TWO } from '../../../gen/defs/attribute';
import {
  II_ARM_S_CLO_CLO_DRAGON1,
  II_ARM_S_CLO_CLO_DRAGON2,
  II_ARM_S_CLO_CLO_DRAGON3,
  II_ARM_S_CLO_CLO_DRAGON4,
  II_GEN_MAT_MOONSTONE_1,
  II_GEN_MAT_ORICHALCUM01_1,
  II_SYS_SYS_SCR_SEALCHARACTER,
} from '../../../gen/defs/item';
import {
  ARMOR_SET,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK3_CLOAK,
  IK3_EGG,
  IK3_PET,
  IK3_SHIELD,
  PET_VIS,
  WEAPON_GENERAL,
  WEAPON_ULTIMATE,
  WEAPON_UNIQUE,
} from '../../../gen/defs/itemkind';
import {
  TID_GAME_CAGE_STRING,
  TID_GAME_TOOLTIP_ITEM_BINDS,
  TID_GAME_TOOLTIP_ONEHANDWEAPON,
  TID_GAME_TOOLTIP_SEXFEMALE,
  TID_GAME_TOOLTIP_SEXMALE,
  TID_GAME_TOOLTIP_TWOHANDWEAPON,
  TID_GUILD_CLOAK,
  TID_TOOLTIP_EQUIPBIND_AFTER,
  TID_TOOLTIP_EQUIPBIND_BEFORE,
  TID_TOOLTIP_PET_BUFF,
  TID_TOOLTIP_PET_PICKUP,
  TID_TOOLTIP_PET_REAR,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR } from '../../../ui/text/markup';
import { IP_FLAG, NULL_ID, partsOf } from '../../inventory/info';
import { ITEM_FLAG } from '../../inventory/items';
import { sprintf } from '../printf';
import { ESSTY } from '../rich';
import type { ComposeCx } from '../model';
import { MI_PET_EGG } from '../../../gen/defs/obj';

/** `CItemElem::IsEatPet()`. */
export const isEatPet = (cx: Pick<ComposeCx, 'prop'>): boolean => cx.prop.item_kind3 === IK3_PET;
/** `ItemProp::IsVisPet()` or a transformed pet. */
export const isVisPet = (cx: Pick<ComposeCx, 'prop' | 'item'>): boolean =>
  (cx.prop.item_kind3 === IK3_PET && cx.prop.refer_stat1 === PET_VIS) ||
  cx.item.transformVisPet === true;
/** `CItemElem::IsPet()` (an egg item). */
export const isPetItem = (cx: Pick<ComposeCx, 'prop'>): boolean => cx.prop.item_kind3 === IK3_EGG;
/** `CItemElem::IsEgg()`: an egg item without a pet or with an egg-level pet. */
export const isEgg = (cx: Pick<ComposeCx, 'prop' | 'item'>): boolean =>
  isPetItem(cx) && (!cx.item.pet || cx.item.pet.level === 0);
/** `ItemProp::IsUltimate()`. */
export const isUltimate = (cx: Pick<ComposeCx, 'prop'>): boolean =>
  (cx.prop.item_kind2 === IK2_WEAPON_DIRECT || cx.prop.item_kind2 === IK2_WEAPON_MAGIC) &&
  cx.prop.refer_stat1 === WEAPON_ULTIMATE;

const DRAGON_CLOAKS: readonly number[] = [
  II_ARM_S_CLO_CLO_DRAGON1,
  II_ARM_S_CLO_CLO_DRAGON2,
  II_ARM_S_CLO_CLO_DRAGON3,
  II_ARM_S_CLO_CLO_DRAGON4,
];

/** `CPet::GetIndex()`: the mover of the pet's shape. */
function petMoverIndex(cx: ComposeCx): number | null {
  const pet = cx.item.pet;
  if (!pet) return null;
  if (pet.level === 0) return MI_PET_EGG;
  const avail = cx.env.petTables()?.avail[pet.kind];
  if (!avail) return null;
  const shape = pet.level < 3 ? 0 : pet.level < 5 ? 1 : 2;
  return avail.movers[shape] ?? null;
}

/** `CItemElem::GetName()`: the item name; an egg with a pet is the pet's cage. */
export function itemName(cx: ComposeCx): string {
  const pet = cx.item.pet;
  if (cx.prop.item_kind3 === IK3_EGG && pet) {
    const index = petMoverIndex(cx);
    const moverName = index === null ? null : cx.env.moverName(index);
    if (moverName !== null) {
      return sprintf(cx.env.text(TID_GAME_CAGE_STRING), [pet.name !== '' ? pet.name : moverName]);
    }
  }
  return cx.prop.name;
}

/** `PutItemName`: the bold name line; returns its colour (the `+N` and the socket count follow in it). */
export function putItemName(cx: ComposeCx): number {
  const { prop, item, colors, env } = cx;
  let color = colors.name0;
  switch (prop.refer_stat1) {
    case WEAPON_GENERAL:
      color = colors.name0;
      break;
    case WEAPON_UNIQUE:
      color = colors.name1;
      break;
    case WEAPON_ULTIMATE:
      color = colors.name3;
      break;
    case ARMOR_SET:
      color = colors.name1;
      break;
    default:
      break;
  }
  let prefix = '';
  // `pItemElem->GetProp()->dwFlag != IP_FLAG_EQUIP_BIND` is an equality, not a mask test (KEEP)
  if (
    (item.flag & ITEM_FLAG.BINDS) !== 0 &&
    item.itemId !== II_GEN_MAT_ORICHALCUM01_1 &&
    item.itemId !== II_GEN_MAT_MOONSTONE_1 &&
    prop.flag !== IP_FLAG.EQUIP_BIND
  ) {
    prefix = `${env.text(TID_GAME_TOOLTIP_ITEM_BINDS)} `;
  }
  // the stat-bonus prefix ("of the Tiger") replaces the bound prefix (the second `strTemp.Format` overwrites the first)
  if (partsOf(prop) !== -1) {
    const stat = cx.numbers?.randomStat;
    if (stat) prefix = `${stat.name} `;
  }
  let name: string;
  if (prop.item_kind3 === IK3_CLOAK && item.idGuild !== 0) {
    const guild = env.guildName(item.idGuild);
    name =
      guild !== null && !DRAGON_CLOAKS.includes(item.itemId)
        ? sprintf(env.text(TID_GUILD_CLOAK), [guild])
        : prop.name;
  } else {
    name = itemName(cx);
  }
  // an ultimate weapon's 13 leading spaces are the host's (`ultimateRuns`): it shifts the name when the band is present
  cx.b.add(prefix + name, color, ESSTY.BOLD);
  return color;
}

/** `PutItemAbilityPiercing`: ` +N` (bold, the name colour) and `    (filled/size)` in the piercing colour. */
export function putItemAbilityPiercing(cx: ComposeCx, nameColor: number): void {
  const { item, b, colors } = cx;
  if (item.abilityOption !== 0) {
    b.add(
      ` ${item.abilityOption < 0 ? '-' : '+'}${String(Math.abs(item.abilityOption))}`,
      nameColor,
      ESSTY.BOLD,
    );
  }
  let filled = 0;
  for (const card of item.cards) if (card !== 0) filled += 1;
  if (item.cards.length > 0 && item.itemId !== II_SYS_SYS_SCR_SEALCHARACTER) {
    b.add(`    (${String(filled)}/${String(item.cards.length)})`, colors.piercing);
  }
}

/** `PutPetKind` (default colour, the commented-out colours of the source): buff / pickup / rear pet. */
export function putPetKind(cx: ComposeCx): void {
  const { env, b } = cx;
  let line = '';
  if (isEatPet(cx)) {
    line = isVisPet(cx)
      ? `\n${env.text(TID_TOOLTIP_PET_BUFF)}`
      : `\n${env.text(TID_TOOLTIP_PET_PICKUP)}`;
  }
  if (isEgg(cx)) line = `\n${env.text(TID_TOOLTIP_PET_REAR)}`;
  b.add(line, DEFAULT_TEXT_COLOR);
}

/** The equip-bind state line (`IP_FLAG_EQUIP_BIND`), in the colour of its text. */
export function putEquipBindState(cx: ComposeCx): void {
  if ((cx.prop.flag & IP_FLAG.EQUIP_BIND) === 0) return;
  const { env, b } = cx;
  const tid =
    (cx.item.flag & ITEM_FLAG.BINDS) !== 0
      ? TID_TOOLTIP_EQUIPBIND_AFTER
      : TID_TOOLTIP_EQUIPBIND_BEFORE;
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(env.text(tid), env.textColor(tid));
}

/** `PutWeapon`: one- or two-handed (not for shields). The newline is added even for a handedness the text does not cover. */
export function putWeapon(cx: ComposeCx): void {
  const { prop, env, b, colors } = cx;
  if (prop.item_kind3 === IK3_SHIELD || prop.handed === NULL_ID) return;
  let line = '';
  if (prop.handed === HD_ONE) line = env.text(TID_GAME_TOOLTIP_ONEHANDWEAPON);
  else if (prop.handed === HD_TWO) line = env.text(TID_GAME_TOOLTIP_TWOHANDWEAPON);
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(line, colors.general);
}

/** `PutSex`: the sex restriction, in the not-usable colour for the other sex. */
export function putSex(cx: ComposeCx): void {
  const { prop, env, b, colors, owner } = cx;
  if (prop.item_sex === NULL_ID) return;
  const line = env.text(
    prop.item_sex === 0 ? TID_GAME_TOOLTIP_SEXMALE : TID_GAME_TOOLTIP_SEXFEMALE,
  );
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(line, prop.item_sex === owner.sex ? colors.general : colors.notUse);
}
