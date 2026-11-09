// The three text colour sets of the item tooltip (Plan/45 §10.4; `CWndMgr::InitSetItemTextColor`,
// `src:_Interface/WndManager.cpp:547-640`). The option "tooltip text colours" (`g_Option.m_nToolTipText`, 0/1/2) picks the set:
// FIRST is the base table, SECOND overrides a few entries, THIRD is a copy of FIRST. Every colour is `D3DCOLOR_XRGB` (opaque).
// The values are interface facts transcribed by hand (Open question 5 of doc 45); `colors.test.ts` pins them against the table.

import { argb } from '../../ui/text/markup';

/** `ToolTipItemTextColor` (`WndManager.h`): one colour per kind of tooltip line. */
export interface ItemTextColors {
  readonly name0: number;
  readonly name1: number;
  readonly name2: number;
  readonly name3: number;
  readonly name4: number;
  readonly name5: number;
  readonly general: number;
  readonly plusOption: number;
  readonly piercing: number;
  readonly resist: number;
  readonly resistFire: number;
  readonly resistWater: number;
  readonly resistEarth: number;
  readonly resistElectricity: number;
  readonly resistWind: number;
  /** An element stone applied by a cash item (weapon). */
  readonly resistSM: number;
  /** An element stone applied by a cash item (armour); also the pet-dead line. */
  readonly resistSM1: number;
  readonly time: number;
  readonly effective0: number;
  readonly effective1: number;
  readonly effective2: number;
  readonly effective3: number;
  readonly randomOption: number;
  readonly enchantOption: number;
  readonly setName: number;
  readonly setItem0: number;
  readonly setItem1: number;
  readonly setEffect: number;
  readonly gold: number;
  readonly command: number;
  readonly notUse: number;
  readonly addedOpt: readonly [
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  readonly awakening: number;
  readonly blessing: number;
  readonly blessingWarning: number;
}

const rgb = (r: number, g: number, b: number): number => argb(255, r, g, b);

/** `dwItemColor[FIRST_TC]`. */
export const FIRST_COLORS: ItemTextColors = {
  name0: rgb(46, 112, 169),
  name1: rgb(0, 93, 0),
  name2: rgb(128, 64, 0),
  name3: rgb(127, 0, 255),
  name4: rgb(0, 0, 255),
  name5: rgb(0, 0, 255),
  general: rgb(0, 0, 0),
  plusOption: rgb(0, 0, 0),
  piercing: rgb(178, 0, 255),
  resist: rgb(0, 0, 0),
  resistFire: rgb(0, 0, 0),
  resistWater: rgb(0, 0, 0),
  resistEarth: rgb(0, 0, 0),
  resistElectricity: rgb(0, 0, 0),
  resistWind: rgb(0, 0, 0),
  resistSM: rgb(0, 158, 0),
  resistSM1: rgb(255, 0, 0),
  time: rgb(0, 200, 0),
  effective0: rgb(0, 0, 0),
  effective1: rgb(0, 255, 0),
  effective2: rgb(255, 0, 0),
  effective3: rgb(128, 128, 128),
  randomOption: rgb(0, 0, 0),
  enchantOption: rgb(0, 0, 0),
  setName: rgb(0, 0, 0),
  setItem0: rgb(178, 178, 178),
  setItem1: rgb(0, 0, 204),
  setEffect: rgb(255, 157, 0),
  gold: rgb(0, 0, 0),
  command: rgb(0, 0, 0),
  notUse: rgb(255, 66, 0),
  addedOpt: [
    rgb(127, 255, 0),
    rgb(0, 255, 255),
    rgb(140, 23, 23),
    rgb(255, 127, 0),
    rgb(255, 110, 199),
    rgb(205, 127, 50),
    rgb(255, 0, 0),
    rgb(50, 50, 255),
    rgb(147, 112, 219),
  ],
  // `__EXT_PIERCING` (VER >= 12): blue; the older purple pair is the dead `#else`
  awakening: rgb(0, 0, 255),
  blessing: rgb(0, 0, 255),
  blessingWarning: rgb(255, 0, 0),
};

/** `dwItemColor[SECOND_TC]`: FIRST with these entries replaced. */
export const SECOND_COLORS: ItemTextColors = {
  ...FIRST_COLORS,
  name0: rgb(12, 144, 231),
  resistFire: rgb(230, 0, 0),
  resistWater: rgb(0, 0, 182),
  resistEarth: rgb(185, 118, 0),
  resistElectricity: rgb(21, 55, 152),
  resistWind: rgb(0, 217, 214),
  time: rgb(146, 0, 38),
  plusOption: rgb(199, 155, 0),
  piercing: rgb(77, 11, 145),
  randomOption: rgb(199, 155, 0),
  setName: rgb(50, 178, 0),
  setItem0: rgb(178, 178, 178),
  setItem1: rgb(114, 186, 85),
  setEffect: rgb(50, 178, 0),
  command: rgb(199, 155, 0),
};

/** `dwItemColor[THIRD_TC]`: a copy of FIRST. */
export const THIRD_COLORS: ItemTextColors = { ...FIRST_COLORS };

export const ITEM_COLOR_SETS: readonly [ItemTextColors, ItemTextColors, ItemTextColors] = [
  FIRST_COLORS,
  SECOND_COLORS,
  THIRD_COLORS,
];

/** The colours of the option value `0 | 1 | 2` (anything else is FIRST, like an out-of-range option would crash the original). */
export function colorsOf(set: number): ItemTextColors {
  return ITEM_COLOR_SETS[set === 1 ? 1 : set === 2 ? 2 : 0];
}

/** The colour of an element-stone line by the element (`SAI79::ePropType`): fire 1, water 2, electricity 3, wind 4, earth 5. */
export function elementColor(c: ItemTextColors, element: number): number {
  switch (element) {
    case 1:
      return c.resistFire;
    case 2:
      return c.resistWater;
    case 3:
      return c.resistElectricity;
    case 4:
      return c.resistWind;
    case 5:
      return c.resistEarth;
    default:
      return c.resist;
  }
}

/** The colour of the awakening line whose value is the table maximum (`__MAX_AWAKE_COLOR`). */
export const MAX_AWAKE_COLOR = 0xffed4a62;
/** The quantity line of `__ITEM_COUNT_TOOLTIP`. */
export const ITEM_COUNT_COLOR = 0xff3fabb9;
