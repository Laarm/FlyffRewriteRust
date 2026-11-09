// The geometry and decisions of the inventory's equipment area as pure functions (WP-UI-21; `CWndInventory`, WndField.cpp:
// 1127-1250 tooltips, 1249-1420 draw, 1624-1700 `m_InvenRect`, 2881-2945 mouse): which placeholder holds which `PARTS_*`
// slot, what is drawn in it, which part a point hits and which tooltip text an empty slot shows.

import {
  PARTS_BOOTS,
  PARTS_BULLET,
  PARTS_CAP,
  PARTS_CAP2,
  PARTS_CLOAK,
  PARTS_CLOTH,
  PARTS_EARRING1,
  PARTS_EARRING2,
  PARTS_FOOT,
  PARTS_FOOT2,
  PARTS_GLOVE,
  PARTS_HAND,
  PARTS_HAND2,
  PARTS_HAT,
  PARTS_LWEAPON,
  PARTS_MASK,
  PARTS_NECKLACE1,
  PARTS_RIDE,
  PARTS_RING1,
  PARTS_RING2,
  PARTS_RWEAPON,
  PARTS_SHIELD,
  PARTS_UPPER2,
  PARTS_UPPER_BODY,
  MAX_HUMAN_PARTS,
} from '../../../gen/defs/neuz';
import {
  TID_TOOLTIP_INVENTORY_BOOTS,
  TID_TOOLTIP_INVENTORY_BULLET,
  TID_TOOLTIP_INVENTORY_CLO,
  TID_TOOLTIP_INVENTORY_CLOTHES_01,
  TID_TOOLTIP_INVENTORY_CLOTHES_02,
  TID_TOOLTIP_INVENTORY_CLOTHES_03,
  TID_TOOLTIP_INVENTORY_CLOTHES_04,
  TID_TOOLTIP_INVENTORY_EARRING,
  TID_TOOLTIP_INVENTORY_GAUNTLET,
  TID_TOOLTIP_INVENTORY_GUARD,
  TID_TOOLTIP_INVENTORY_HELMET,
  TID_TOOLTIP_INVENTORY_MASK,
  TID_TOOLTIP_INVENTORY_NECKLACE,
  TID_TOOLTIP_INVENTORY_RID,
  TID_TOOLTIP_INVENTORY_RING,
  TID_TOOLTIP_INVENTORY_SUIT,
  TID_TOOLTIP_INVENTORY_WEAPON,
} from '../../../gen/defs/text';
import { type Rect, rectContains } from '../../layout/geometry';
import { FONT, lineHeight } from '../../text/fonts';
import type { PaperLayer } from './PaperDoll';
import type { InventoryItem } from './ports';

/** The first part drawn by the loop of the original (`for( int i=2; i<MAX_HUMAN_PARTS; i++ )`). */
export const FIRST_DRAWN_PART = 2;

/**
 * `m_InvenRect[part] = GetWndCtrl(WIDC_CUSTOMn)->rect` (`OnInitialUpdate`): the placeholder of each part. Several parts share
 * a placeholder (the overcoat pieces of V8-V12 and the shield / left weapon pair).
 */
export const PART_PLACEHOLDERS: readonly {
  readonly custom: string;
  readonly parts: readonly number[];
}[] = [
  { custom: 'WIDC_CUSTOM1', parts: [PARTS_CAP, PARTS_CAP2] },
  { custom: 'WIDC_CUSTOM2', parts: [PARTS_UPPER_BODY, PARTS_UPPER2] },
  { custom: 'WIDC_CUSTOM3', parts: [PARTS_HAND, PARTS_HAND2] },
  { custom: 'WIDC_CUSTOM4', parts: [PARTS_FOOT, PARTS_FOOT2] },
  { custom: 'WIDC_CUSTOM5', parts: [PARTS_RWEAPON] },
  { custom: 'WIDC_CUSTOM6', parts: [PARTS_LWEAPON, PARTS_SHIELD] },
  { custom: 'WIDC_CUSTOM7', parts: [PARTS_BULLET] },
  { custom: 'WIDC_CUSTOM8', parts: [PARTS_CLOAK] },
  { custom: 'WIDC_CUSTOM9', parts: [PARTS_MASK] },
  { custom: 'WIDC_CUSTOM10', parts: [PARTS_RIDE] },
  { custom: 'WIDC_CUSTOM11', parts: [PARTS_RING1] },
  { custom: 'WIDC_CUSTOM12', parts: [PARTS_EARRING1] },
  { custom: 'WIDC_CUSTOM13', parts: [PARTS_NECKLACE1] },
  { custom: 'WIDC_CUSTOM14', parts: [PARTS_EARRING2] },
  { custom: 'WIDC_CUSTOM15', parts: [PARTS_RING2] },
  { custom: 'WIDC_CUSTOM16', parts: [PARTS_HAT] },
  { custom: 'WIDC_CUSTOM17', parts: [PARTS_CLOTH] },
  { custom: 'WIDC_CUSTOM18', parts: [PARTS_GLOVE] },
  { custom: 'WIDC_CUSTOM19', parts: [PARTS_BOOTS] },
];

/** The fashion pieces that have a hide box, in the order of `m_abHideCoat` (HAT, CLOTH, GLOVE, BOOTS, then the cloak). */
export const HIDE_PARTS: readonly number[] = [
  PARTS_HAT,
  PARTS_CLOTH,
  PARTS_GLOVE,
  PARTS_BOOTS,
  PARTS_CLOAK,
];
/** The id of the first hide box (`100000 + i`). */
export const HIDE_BOX_BASE_ID = 100000;

/** `part -> rect` from the placeholder rects (`m_InvenRect`); parts without a placeholder have no entry. */
export function partRects(rectOf: (custom: string) => Rect | null): ReadonlyMap<number, Rect> {
  const m = new Map<number, Rect>();
  for (const p of PART_PLACEHOLDERS) {
    const r = rectOf(p.custom);
    if (!r) continue;
    for (const part of p.parts) m.set(part, r);
  }
  return m;
}

/** The icon scale and offset of a part (`sx`, `cpAdd` of `OnDraw`): accessories 0.8 at (0, 0), fashion pieces 0.9 at (0, 0), else 1 at (6, 6). */
export function partIconGeometry(part: number): { scale: number; add: number } {
  if (part >= PARTS_NECKLACE1 && part <= PARTS_EARRING2) return { scale: 0.8, add: 0 };
  if (part >= PARTS_HAT && part <= PARTS_BOOTS) return { scale: 0.9, add: 0 };
  return { scale: 1, add: 6 };
}

/** `alpha` of ghosted icons (`dwAlpha = 100`). */
export const GHOST_ALPHA = 100 / 255;

/** The stack count of an equipped stackable item (arrows in the bullet slot): right edge at 42, bottom at 42 from the rect. */
export function equipCountPos(rect: Rect, newFontColor: boolean): { right: number; top: number } {
  const font = newFontColor ? FONT.title : FONT.text;
  return { right: rect.l + 42, top: rect.t + 42 - lineHeight(font) };
}

/**
 * What is drawn for each part, in the draw order of the original (parts 2..30 ascending; a later part overwrites an earlier one
 * that shares its placeholder). A two-handed right weapon is repeated ghosted in the left-hand slot; a hidden fashion piece is
 * ghosted; an expired item is red.
 */
export function buildPaperLayers(o: {
  readonly equip: readonly (InventoryItem | null)[];
  readonly rects: ReadonlyMap<number, Rect>;
  readonly hideCoat: readonly boolean[];
  readonly newFontColor: boolean;
}): PaperLayer[] {
  const layers: PaperLayer[] = [];
  for (let part = FIRST_DRAWN_PART; part < MAX_HUMAN_PARTS; part += 1) {
    let item = o.equip[part] ?? null;
    let alpha = 1;
    if (part === PARTS_LWEAPON) {
      const right = o.equip[PARTS_RWEAPON] ?? null;
      if (right?.twoHanded === true) {
        item = right;
        alpha = GHOST_ALPHA;
      }
    }
    const rect = o.rects.get(part);
    if (!item || !rect || item.icon === null) continue;
    if (part >= PARTS_HAT && part <= PARTS_BOOTS && o.hideCoat[part - PARTS_HAT] === true) {
      alpha = GHOST_ALPHA;
    }
    const g = partIconGeometry(part);
    const layer: PaperLayer = {
      key: `part:${String(part)}`,
      x: rect.l + g.add,
      y: rect.t + g.add,
      size: Math.round(32 * g.scale),
      icon: item.icon,
      alpha,
      tint: item.expired,
      ...(item.stackable
        ? { count: { text: String(item.count), ...equipCountPos(rect, o.newFontColor) } }
        : {}),
    };
    layers.push(layer);
  }
  return layers;
}

/**
 * The part whose placeholder holds the point AND that has an item (`OnLButtonDown`, `OnLButtonDblClk`: the first part in
 * ascending order with a worn item; the shield behind a left weapon placeholder is found after the left weapon).
 */
export function wornPartAt(
  pt: { x: number; y: number },
  rects: ReadonlyMap<number, Rect>,
  equip: readonly (InventoryItem | null)[],
): number | null {
  for (let part = FIRST_DRAWN_PART; part < MAX_HUMAN_PARTS; part += 1) {
    const r = rects.get(part);
    if (r && rectContains(r, pt) && equip[part]) return part;
  }
  return null;
}

/** The first part (ascending) whose placeholder holds the point, worn or not (`OnMouseWndSurface`). */
export function slotPartAt(
  pt: { x: number; y: number },
  rects: ReadonlyMap<number, Rect>,
): number | null {
  for (let part = FIRST_DRAWN_PART; part < MAX_HUMAN_PARTS; part += 1) {
    const r = rects.get(part);
    if (r && rectContains(r, pt)) return part;
  }
  return null;
}

/**
 * What a tooltip over a placeholder shows (`OnMouseWndSurface`): the worn item - for the left-weapon placeholder the shield,
 * else the left weapon, else the right weapon - or the name of the slot when it is empty.
 */
export function slotTooltip(
  part: number,
  equip: readonly (InventoryItem | null)[],
): { readonly item: InventoryItem; readonly part: number } | { readonly tid: number } {
  if (part === PARTS_LWEAPON) {
    const shield = equip[PARTS_SHIELD] ?? null;
    if (shield) return { item: shield, part: PARTS_SHIELD };
    const left = equip[PARTS_LWEAPON] ?? null;
    if (left) return { item: left, part: PARTS_LWEAPON };
    const right = equip[PARTS_RWEAPON] ?? null;
    if (right) return { item: right, part: PARTS_RWEAPON };
    return { tid: TID_TOOLTIP_INVENTORY_WEAPON };
  }
  const item = equip[part] ?? null;
  if (item) return { item, part };
  return { tid: emptySlotTid(part) };
}

/** `TID_TOOLTIP_INVENTORY_*` of an empty slot (`0` for a part the switch does not name). */
export function emptySlotTid(part: number): number {
  switch (part) {
    case PARTS_UPPER_BODY:
      return TID_TOOLTIP_INVENTORY_SUIT;
    case PARTS_HAND:
      return TID_TOOLTIP_INVENTORY_GAUNTLET;
    case PARTS_FOOT:
      return TID_TOOLTIP_INVENTORY_BOOTS;
    case PARTS_CAP:
      return TID_TOOLTIP_INVENTORY_HELMET;
    case PARTS_CLOAK:
      return TID_TOOLTIP_INVENTORY_CLO;
    case PARTS_LWEAPON:
    case PARTS_RWEAPON:
      return TID_TOOLTIP_INVENTORY_WEAPON;
    case PARTS_SHIELD:
      return TID_TOOLTIP_INVENTORY_GUARD;
    case PARTS_MASK:
      return TID_TOOLTIP_INVENTORY_MASK;
    case PARTS_RIDE:
      return TID_TOOLTIP_INVENTORY_RID;
    case PARTS_NECKLACE1:
      return TID_TOOLTIP_INVENTORY_NECKLACE;
    case PARTS_RING1:
    case PARTS_RING2:
      return TID_TOOLTIP_INVENTORY_RING;
    case PARTS_EARRING1:
    case PARTS_EARRING2:
      return TID_TOOLTIP_INVENTORY_EARRING;
    case PARTS_BULLET:
      return TID_TOOLTIP_INVENTORY_BULLET;
    case PARTS_HAT:
      return TID_TOOLTIP_INVENTORY_CLOTHES_01;
    case PARTS_CLOTH:
      return TID_TOOLTIP_INVENTORY_CLOTHES_02;
    case PARTS_GLOVE:
      return TID_TOOLTIP_INVENTORY_CLOTHES_03;
    case PARTS_BOOTS:
      return TID_TOOLTIP_INVENTORY_CLOTHES_04;
    default:
      return 0;
  }
}

/**
 * The area where a bag item dropped from the bag equips (`OnDropIcon`): the union of the placeholders `WIDC_CUSTOM5` (top-left
 * corner of the equipment) to `WIDC_CUSTOM10` (bottom-right) - "items dropped on the border of the window are not worn".
 */
export function equipDropArea(custom5: Rect, custom10: Rect): Rect {
  return { l: custom5.l, t: custom5.t, r: custom10.r, b: custom10.b };
}
