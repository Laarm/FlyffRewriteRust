// The experience-scroll families of the buff strips (`II_SYS_SYS_SCR_AMPES{A,B,C,D,E,S}`): scrolls of one family stack into ONE icon
// whose tooltip lists each scroll's time; the E scroll does not stack.

import {
  II_SYS_SYS_SCR_AMPESA,
  II_SYS_SYS_SCR_AMPESA1,
  II_SYS_SYS_SCR_AMPESA2,
  II_SYS_SYS_SCR_AMPESB,
  II_SYS_SYS_SCR_AMPESB1,
  II_SYS_SYS_SCR_AMPESB2,
  II_SYS_SYS_SCR_AMPESC,
  II_SYS_SYS_SCR_AMPESC1,
  II_SYS_SYS_SCR_AMPESC2,
  II_SYS_SYS_SCR_AMPESD,
  II_SYS_SYS_SCR_AMPESD1,
  II_SYS_SYS_SCR_AMPESD2,
  II_SYS_SYS_SCR_AMPESE,
  II_SYS_SYS_SCR_AMPESS,
  II_SYS_SYS_SCR_AMPESS1,
  II_SYS_SYS_SCR_AMPESS2,
} from '../../gen/defs/item';

export interface ExpFamily {
  /** `nExpkind` 0..5 (A, B, C, D, E, S). */
  readonly kind: number;
  /** The item shown for the family. */
  readonly base: number;
  readonly ids: readonly number[];
  /** `IsOverlap`: the scrolls stack (all but E). */
  readonly overlap: boolean;
  /** `bSpecial`: the S scroll. */
  readonly special: boolean;
}

export const EXP_FAMILIES: readonly ExpFamily[] = [
  {
    kind: 0,
    base: II_SYS_SYS_SCR_AMPESA,
    ids: [II_SYS_SYS_SCR_AMPESA, II_SYS_SYS_SCR_AMPESA1, II_SYS_SYS_SCR_AMPESA2],
    overlap: true,
    special: false,
  },
  {
    kind: 1,
    base: II_SYS_SYS_SCR_AMPESB,
    ids: [II_SYS_SYS_SCR_AMPESB, II_SYS_SYS_SCR_AMPESB1, II_SYS_SYS_SCR_AMPESB2],
    overlap: true,
    special: false,
  },
  {
    kind: 2,
    base: II_SYS_SYS_SCR_AMPESC,
    ids: [II_SYS_SYS_SCR_AMPESC, II_SYS_SYS_SCR_AMPESC1, II_SYS_SYS_SCR_AMPESC2],
    overlap: true,
    special: false,
  },
  {
    kind: 3,
    base: II_SYS_SYS_SCR_AMPESD,
    ids: [II_SYS_SYS_SCR_AMPESD, II_SYS_SYS_SCR_AMPESD1, II_SYS_SYS_SCR_AMPESD2],
    overlap: true,
    special: false,
  },
  {
    kind: 4,
    base: II_SYS_SYS_SCR_AMPESE,
    ids: [II_SYS_SYS_SCR_AMPESE],
    overlap: false,
    special: false,
  },
  {
    kind: 5,
    base: II_SYS_SYS_SCR_AMPESS,
    ids: [II_SYS_SYS_SCR_AMPESS, II_SYS_SYS_SCR_AMPESS1, II_SYS_SYS_SCR_AMPESS2],
    overlap: true,
    special: true,
  },
];

/** The family of an item id, `undefined` for any other item. */
export function expFamilyOf(itemId: number): ExpFamily | undefined {
  return EXP_FAMILIES.find((f) => f.ids.includes(itemId));
}
