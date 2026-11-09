// The upgrade, element, awakening and piercing windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-29, WP-UI-30, WP-UI-31): the lazy classes of
// `lazy.ts`. `APP_SMELT_SAFETY_CONFIRM`, `APP_RANDOMSCROLL_CONFIRM`, `APP_RANDOMOPTIONRETRY`, `APP_CHANGE_ATTRIBUTE` and `APP_REMOVE_ELEM` are the item-use windows'
// (`itemuse`).
import type { WndCtor } from '../../catalog';
import { UPGRADE_LAZY } from './lazy';

export const UPGRADE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_TEST: () => UPGRADE_LAZY.WndUpgradeBase.load(),
  APP_SMELT_SAFETY: () => UPGRADE_LAZY.WndSmeltSafety.load(),
  APP_REMOVE_ATTRIBUTE: () => UPGRADE_LAZY.WndRemoveAttribute.load(),
  APP_REMOVE_ATTRIBUTE_CONFIRM: () => UPGRADE_LAZY.WndRemoveAttributeConfirm.load(),
  APP_COMMERCIAL_ELEM: () => UPGRADE_LAZY.WndCommercialElem.load(),
  APP_AWAKENING: () => UPGRADE_LAZY.WndAwakening.load(),
  APP_AWAKE_SELECTCASE: () => UPGRADE_LAZY.WndSelectAwakeCase.load(),
  APP_CANCEL_BLESSING: () => UPGRADE_LAZY.WndCancelBlessing.load(),
  APP_LVREQDOWN: () => UPGRADE_LAZY.WndLvReqDown.load(),
  APP_PET_AWAK_CANCEL: () => UPGRADE_LAZY.WndPetAwakCancel.load(),
  APP_PIERCING: () => UPGRADE_LAZY.WndPiercing.load(),
  APP_SMELT_REMOVE_PIERCING: () => UPGRADE_LAZY.WndRemovePiercing.load(),
  APP_SMELT_REMOVE_PIERCING_EX: () => UPGRADE_LAZY.WndRemovePiercing.load(),
};
