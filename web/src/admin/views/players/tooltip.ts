// The item tooltip of the Items tab (Plan/25 section 4.4; the 0.5 ad spike of WP-ADMIN-18): the EXACT composer of the game
// (`features/tooltip/compose.ts`, a pure function of the item instance, the viewer, the texts and one batched `item_tooltip_numbers` answer of
// the wasm rules) runs here without an in-world player. The viewer is THIS CHARACTER: its level, job and sex colour the requirement lines
// as that player sees them, its worn items give the set bonus and the weapon of the numbers. Nothing here reads a game store.
import { composeItem } from '../../../features/tooltip/compose';
import { itemFromWire } from '../../../features/inventory/items';
import { partsOf } from '../../../features/inventory/info';
import { jobTypeByLayout } from '../../../features/tooltip/jobs';
import type {
  ComposeEnv,
  ItemTooltipExtra,
  TooltipItemRecord,
  TooltipItemView,
  TooltipOwnerInfo,
} from '../../../features/tooltip/model';
import type { SetItemAvail } from '../../../gen/bundles';
import { APP_INVENTORY } from '../../../gen/defs/resdata';
import {
  PARTS_CAP,
  PARTS_FOOT,
  PARTS_HAND,
  PARTS_RWEAPON,
  PARTS_UPPER_BODY,
} from '../../../gen/defs/neuz';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import {
  splitRandomOptionWord,
  type ItemTooltipInput,
  type ItemTooltipNumbers,
  type RulesFacade,
} from '../../../rules';
import type { TooltipWornJs } from '../../../rules/types';
import type { RichRun } from '../../../ui/text/markup';
import type { ItemTooltipCtx, UltimateBand } from '../../../ui/tooltip/types';
import type { ContainerModel } from './containers';
import { isExpired } from './containers';

/** The composed lines of a tooltip and its ultimate band. */
export interface TipContent {
  readonly runs: readonly RichRun[];
  readonly ultimate: UltimateBand | undefined;
}

/** What the viewer of a tooltip is: the character the console is looking at. */
export interface TipViewer {
  /** 0 male, 1 female. */
  readonly sex: number;
  readonly job: number;
  readonly level: number;
}

export function ownerOf(viewer: TipViewer): TooltipOwnerInfo {
  return {
    sex: viewer.sex === 1 ? 1 : 0,
    job: viewer.job,
    level: viewer.level,
    jobType: jobTypeByLayout(viewer.job) ?? 0,
  };
}

/** The worn items of a character (its equipment container), for the set bonus and the weapon of the numbers. */
export interface TipEquipment {
  readonly worn: readonly TooltipWornJs[];
  /** The item id in the right-hand weapon slot, 0 = none. */
  readonly rweaponItem: number;
  /** The equipment cells by part. */
  readonly parts: ReadonlyMap<number, ItemElem>;
}

export const NO_EQUIPMENT: TipEquipment = { worn: [], rweaponItem: 0, parts: new Map() };

export function equipmentOf(equipment: ContainerModel | undefined): TipEquipment {
  if (!equipment) return NO_EQUIPMENT;
  const worn: TooltipWornJs[] = [];
  for (const [part, e] of equipment.cells) {
    worn.push({ part, itemId: e.itemId, expired: isExpired(e) });
  }
  worn.sort((a, b) => a.part - b.part);
  return {
    worn,
    rweaponItem: equipment.cells.get(PARTS_RWEAPON)?.itemId ?? 0,
    parts: equipment.cells,
  };
}

/** The armour +N set bonus row of a WORN set piece (`PutEnchantOpt`): only while at least +1 of every piece is worn. */
export function armorSetBonus(
  equipment: TipEquipment,
  elem: ItemElem,
  parts: number,
  table: readonly SetItemAvail[] | undefined,
): SetItemAvail | null {
  const pieces = [PARTS_UPPER_BODY, PARTS_HAND, PARTS_FOOT, PARTS_CAP];
  if (!pieces.includes(parts)) return null;
  if (equipment.parts.get(parts)?.serial !== elem.serial) return null; // not the worn piece
  let level = 10;
  for (const p of pieces) {
    const worn = equipment.parts.get(p);
    const v = worn ? worn.abilityOption : 0;
    if (v === 0) return null;
    level = Math.min(level, v);
  }
  if (level <= 0) return null;
  return table?.[level - 1] ?? null;
}

/** The input of the numbers call for one item and its viewer. */
export function numbersInput(
  record: TooltipItemRecord,
  viewer: TooltipOwnerInfo,
  equipment: TipEquipment,
): ItemTooltipInput {
  return {
    itemId: record.itemId,
    abilityOption: record.abilityOption,
    expired: (record.flag & 1) !== 0,
    element: record.itemResist,
    elementLevel: record.resistAbilityOption,
    ...splitRandomOptionWord(record.randomOpt),
    eggLevelC: (record.pet?.level ?? 0) >= 2,
    piercing: record.cards,
    ultimateGems: record.gems,
    owner: {
      sex: viewer.sex,
      job: viewer.job,
      level: viewer.level,
      rweaponItem: equipment.rweaponItem,
    },
    worn: equipment.worn,
  };
}

export interface ComposeArgs {
  readonly env: ComposeEnv;
  /** The wasm rules once built; without them the lines that need a number are left out and the rest is the same. */
  readonly rules: Pick<RulesFacade, 'itemTooltipNumbers'> | null;
  /** `tables.exp.set_item_avail` (the armour set bonus), when the `tables` bundle is loaded. */
  readonly setItemAvail: readonly SetItemAvail[] | undefined;
  readonly elem: ItemElem;
  readonly viewer: TipViewer;
  readonly equipment: TipEquipment;
  /** The cell is in the equipment container (a worn piece). */
  readonly worn: boolean;
  /** The clock of the keep-time and cooldown lines, in ms. */
  readonly nowMs: number;
}

/** The tooltip of one cell; `null` for an item the data does not know. */
export function composeTip(a: ComposeArgs): TipContent | null {
  const prop = a.env.prop(a.elem.itemId);
  if (!prop) return null;
  const record: TooltipItemRecord = itemFromWire(a.elem);
  const owner = ownerOf(a.viewer);
  const view: TooltipItemView = {
    name: prop.name,
    kind2: prop.item_kind2,
    parts: prop.parts,
    serial: Number(a.elem.serial),
    item: { ...record, objId: a.elem.objId },
  };
  const answer: ItemTooltipNumbers | null = a.rules
    ? a.rules.itemTooltipNumbers(numbersInput(record, owner, a.equipment))
    : null;
  const bonus = a.worn ? armorSetBonus(a.equipment, a.elem, partsOf(prop), a.setItemAvail) : null;
  const extra: ItemTooltipExtra = { owner, shop: null, armorSetBonus: bonus };
  const ctx: ItemTooltipCtx = {
    flag: APP_INVENTORY,
    now: a.nowMs,
    numbers: answer
      ? {
          multiplier: answer.multiplier,
          ability: answer.ability,
          equipDenied: answer.equipDenied,
          detail: answer,
        }
      : null,
    extra,
  };
  const out = composeItem(view, ctx, a.env);
  if (!out) return null;
  return { runs: out.runs.runs, ultimate: out.ultimate };
}

/** The plain text of a composed tooltip, one string per line (tests and the "copy" of an admin read this). */
export function tipLines(content: TipContent): string[] {
  const lines: string[] = [];
  let current = '';
  for (const run of content.runs) {
    run.text.split('\n').forEach((part, i) => {
      if (i > 0) {
        lines.push(current);
        current = '';
      }
      current += part;
    });
  }
  lines.push(current);
  return lines.filter((l) => l !== '');
}
