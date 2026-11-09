// The pet windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-33, WP-UI-34): the lazy classes of `lazy.ts`; the pet filter is an applet and a plain import.
// (`APP_PET_AWAK_CANCEL` belongs to the upgrade family, `APP_CHANGENAME` and `APP_CONFIRM_ENTER` to the item-use family.)
import type { WndCtor } from '../../catalog';
import { PETS_LAZY } from './lazy';
import { WndPetFilter } from './WndPetFilter';

export const PET_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_PET_STATUS: () => PETS_LAZY.WndPetStatus.load(),
  APP_PET_ITEM: () => PETS_LAZY.WndFoodConfirm.load(),
  APP_PET_FOODMILL: () => PETS_LAZY.WndPetFoodMill.load(),
  APP_PET_MIRACLE: () => PETS_LAZY.WndPetMiracle.load(),
  APP_PET_TRANS_EGGS: () => PETS_LAZY.WndPetTransEggs.load(),
  APP_PET_RES: () => PETS_LAZY.WndPetRes.load(),
  APP_BUFFPET_STATUS: () => PETS_LAZY.WndBuffPetStatus.load(),
  APP_PETFILTER: () => Promise.resolve(WndPetFilter),
};
