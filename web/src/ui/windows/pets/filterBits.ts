// The twelve boxes of the pet filter and their bits (`Resource/defineFilter.h`, Plan/19 §6.8, 45 §18.7): a set bit means SKIP. In a module of its own so that the pet feature
// reads the table without importing the (lazy) window.

import {
  WIDC_PF_ARMOR,
  WIDC_PF_ELECARDS,
  WIDC_PF_EGG,
  WIDC_PF_FPFOOD,
  WIDC_PF_HPFOOD,
  WIDC_PF_JEWELRY,
  WIDC_PF_MOONSTONE,
  WIDC_PF_MPFOOD,
  WIDC_PF_QUESTITEM,
  WIDC_PF_SOCKETCARD,
  WIDC_PF_SUNSTONE,
  WIDC_PF_WEAPON,
} from '../../../gen/defs/resdata';

/** The check boxes and their bits (`defineFilter.h`: `FILTER_HPFOOD 1` ... `FILTER_SOCKETCARD 2048`). */
export const PET_FILTER_BOXES: readonly {
  readonly id: number;
  readonly bit: number;
  readonly name: string;
}[] = [
  { id: WIDC_PF_HPFOOD, bit: 1, name: 'HPFOOD' },
  { id: WIDC_PF_MPFOOD, bit: 2, name: 'MPFOOD' },
  { id: WIDC_PF_FPFOOD, bit: 4, name: 'FPFOOD' },
  { id: WIDC_PF_WEAPON, bit: 8, name: 'WEAPON' },
  { id: WIDC_PF_ARMOR, bit: 16, name: 'ARMOR' },
  { id: WIDC_PF_JEWELRY, bit: 32, name: 'JEWELRY' },
  { id: WIDC_PF_EGG, bit: 64, name: 'EGG' },
  { id: WIDC_PF_SUNSTONE, bit: 128, name: 'SUNSTONE' },
  { id: WIDC_PF_MOONSTONE, bit: 256, name: 'MOONSTONE' },
  { id: WIDC_PF_QUESTITEM, bit: 512, name: 'QUESTITEM' },
  { id: WIDC_PF_ELECARDS, bit: 1024, name: 'ELECARDS' },
  { id: WIDC_PF_SOCKETCARD, bit: 2048, name: 'SOCKETCARD' },
];
/** Every defined bit. */
export const PET_FILTER_ALL = 0xfff;
