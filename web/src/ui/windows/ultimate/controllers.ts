// The ultimate-weapon windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-32, WP-UI-33): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { ULTIMATE_LAZY } from './lazy';

export const ULTIMATE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_SMELT_MIXJEWEL: () => ULTIMATE_LAZY.WndMixJewel.load(),
  APP_SMELT_MIXJEWEL_CONFIRM: () => ULTIMATE_LAZY.WndMixJewelConfirm.load(),
  APP_SMELT_EXTRACTION: () => ULTIMATE_LAZY.WndExtraction.load(),
  APP_SMELT_JEWEL: () => ULTIMATE_LAZY.WndSmeltJewel.load(),
  APP_SMELT_CHANGEWEAPON: () => ULTIMATE_LAZY.WndChangeWeapon.load(),
  APP_SMELT_REMOVE_JEWEL: () => ULTIMATE_LAZY.WndRemoveJewel.load(),
  APP_SMELT_REMOVE_CONFIRM: () => ULTIMATE_LAZY.WndRemoveJewelConfirm.load(),
};
