// The item tooltip composer (Plan/45 §10.1-§10.2, WP-CFEAT-16/17; `CWndMgr::MakeToolTipText`, `src:_Interface/WndManager.cpp:6684-6880`):
// `composeItemTooltip(view, ctx, env)` appends the sections in the original's order and returns the rich runs (plus the ultimate
// band) the tooltip host (`ui/tooltip`) lays out. It is a pure function of its inputs: every number comes from `ctx.numbers`
// (the one batched `item_tooltip_numbers` call of Plan/42 §9.6), every text from `env`.
//
//   1 name  2 +N and sockets  3 pet kind  4 equip-bind state  5 handedness  6 sex
//   7 by kind: gear (ability range, speed, element, resists, base options, random options, set-enchant bonus), medicine, jewelry,
//     system (vis, seal scroll), disguise
//   8 eat pet skill, vis pet totals  9 cooldown  11 keep time  12 job  13 level  14 description  15 price  16 named set
//   17 pet card / feed pocket  18 socketed cards  19 awakening / blessing  20 ultimate options
// (row 10, the endurance, is compiled out of the original.)

import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_BLINKWING,
  IK2_CLOTH,
  IK2_CLOTHETC,
  IK2_FOOD,
  IK2_GMTEXT,
  IK2_JEWELRY,
  IK2_POTION,
  IK2_REFRESHER,
  IK2_SYSTEM,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK1_ARMOR,
  IK1_WEAPON,
  IK3_EGG,
  IK3_ELECARD,
  IK3_TEXT_DISGUISE,
  IK3_VIS,
} from '../../gen/defs/itemkind';
import { II_SYS_SYS_SCR_PET_FEED_POCKET, II_SYS_SYS_SCR_SEALCHARACTER } from '../../gen/defs/item';
import type {
  ItemTooltipCtx,
  ItemView,
  TooltipContent,
  UltimateBand,
} from '../../ui/tooltip/types';
import { colorsOf } from './colors';
import { jobTypeByLayout } from './jobs';
import {
  isTooltipItemView,
  type ComposeCx,
  type ComposeEnv,
  type ItemTooltipExtra,
  type TooltipOwnerInfo,
} from './model';
import { RichBuilder } from './rich';
import { putAddedOpt, putAwakeningBlessing, putPiercingOpt } from './sections/awakening';
import {
  putBaseItemOpt,
  putBaseResist,
  putEnchantOpt,
  putItemMinMax,
  putItemResist,
  putItemSpeed,
  putMedicine,
  putRandomOpt,
} from './sections/ability';
import {
  isEatPet,
  isUltimate,
  isVisPet,
  putEquipBindState,
  putItemAbilityPiercing,
  putItemName,
  putPetKind,
  putSex,
  putWeapon,
} from './sections/head';
import { putPetFeedPocket, putPetInfo } from './sections/pet';
import { putItemGold } from './sections/price';
import { putCommand, putJob, putLevel } from './sections/requirements';
import { putSetItemOpt } from './sections/set';
import {
  putActiveSkillDsts,
  putDisguise,
  putNeededVis,
  putSealChar,
  putVisPetInfo,
} from './sections/system';
import { putCoolTime, putKeepTime } from './sections/time';
import type { ItemTooltipNumbers } from '../../rules/types';

/** The default viewer when the hover context names none. */
const NO_OWNER: TooltipOwnerInfo = { sex: 0, job: 0, level: 1, jobType: 0 };

/** The icon of the ultimate band (`Icon_Ultimate.dds`, the French build used the theme folder). */
export const ULTIMATE_ICON = 'Icon/Icon_Ultimate.dds';

function numbersOf(ctx: ItemTooltipCtx): ItemTooltipNumbers | null {
  // the provider returns the host's reduced shape with the full rules answer in `detail`
  const n = ctx.numbers as
    (ItemTooltipCtx['numbers'] & { detail?: ItemTooltipNumbers }) | null | undefined;
  return n?.detail ?? null;
}

/** The tooltip lines of one item (`MakeToolTipText`) without the equipped-header or the host box. */
export function composeItem(
  view: ItemView,
  ctx: ItemTooltipCtx,
  env: ComposeEnv,
  into?: RichBuilder,
): { runs: RichBuilder; volatile: boolean; ultimate: UltimateBand | undefined } | null {
  if (!isTooltipItemView(view)) return null;
  const item = view.item;
  const prop = env.prop(item.itemId);
  if (!prop) return null;
  const extra = (ctx.extra ?? {}) as ItemTooltipExtra;
  const owner = extra.owner ?? NO_OWNER;
  const cx: ComposeCx = {
    view,
    item,
    prop,
    flag: ctx.flag,
    now: ctx.now,
    numbers: numbersOf(ctx),
    extra,
    owner: { ...owner, jobType: owner.jobType || (jobTypeByLayout(owner.job) ?? 0) },
    env,
    colors: colorsOf(env.colorSet()),
    b: into ?? new RichBuilder(),
    volatile: false,
  };
  const nameColor = putItemName(cx);
  putItemAbilityPiercing(cx, nameColor);
  putPetKind(cx);
  putEquipBindState(cx);
  putWeapon(cx);
  putSex(cx);
  switch (prop.item_kind2) {
    case IK2_WEAPON_DIRECT:
    case IK2_WEAPON_MAGIC:
    case IK2_ARMORETC:
    case IK2_CLOTHETC:
    case IK2_ARMOR:
    case IK2_CLOTH:
    case IK2_BLINKWING:
      putItemMinMax(cx);
      putItemSpeed(cx);
      if (prop.item_kind3 === IK3_ELECARD) putItemResist(cx);
      else if (
        item.resistAbilityOption !== 0 &&
        (prop.item_kind1 === IK1_WEAPON || prop.item_kind1 === IK1_ARMOR)
      ) {
        putItemResist(cx);
      }
      putBaseResist(cx);
      putBaseItemOpt(cx);
      putRandomOpt(cx);
      putEnchantOpt(cx);
      break;
    case IK2_REFRESHER:
    case IK2_FOOD:
    case IK2_POTION:
      putMedicine(cx, prop.dest_param[0], prop.adj_param_val[0]);
      putMedicine(cx, prop.dest_param[1], prop.adj_param_val[1]);
      break;
    case IK2_JEWELRY:
      putBaseItemOpt(cx);
      break;
    case IK2_SYSTEM:
      if (prop.item_kind3 === IK3_VIS) {
        putNeededVis(cx);
        putBaseItemOpt(cx);
      }
      if (item.itemId === II_SYS_SYS_SCR_SEALCHARACTER) putSealChar(cx);
      break;
    case IK2_GMTEXT:
      if (prop.item_kind3 === IK3_TEXT_DISGUISE) putDisguise(cx);
      break;
    default:
      break;
  }
  if (isEatPet(cx)) putActiveSkillDsts(cx);
  if (isVisPet(cx)) putVisPetInfo(cx);
  putCoolTime(cx);
  putKeepTime(cx);
  putJob(cx);
  putLevel(cx);
  putCommand(cx);
  putItemGold(cx);
  putSetItemOpt(cx);
  if (prop.item_kind3 === IK3_EGG && item.pet) putPetInfo(cx);
  if (prop.id === II_SYS_SYS_SCR_PET_FEED_POCKET) putPetFeedPocket(cx);
  putPiercingOpt(cx);
  putAwakeningBlessing(cx);
  if (isUltimate(cx)) putAddedOpt(cx);
  let ultimate: UltimateBand | undefined;
  if (isUltimate(cx)) {
    ultimate = {
      icon: ULTIMATE_ICON,
      slots: item.gems.length,
      gems: item.gems.map((g) => {
        const p = g === 0 ? undefined : env.prop(g);
        return p && p.icon !== '' ? `item/${p.icon}` : null;
      }),
    };
  }
  return { runs: cx.b, volatile: cx.volatile, ultimate };
}

/** The item composer registered with the tooltip host (`registerComposers({ item })`). */
export function createItemComposer(
  env: ComposeEnv,
  fallback: (item: ItemView, ctx: ItemTooltipCtx) => TooltipContent,
): (item: ItemView, ctx: ItemTooltipCtx) => TooltipContent {
  return (view, ctx) => {
    const out = composeItem(view, ctx, env);
    if (!out) return fallback(view, ctx);
    const content: TooltipContent = { runs: out.runs.runs };
    if (out.ultimate) content.ultimate = out.ultimate;
    if (out.volatile) content.volatile = true;
    return content;
  };
}
