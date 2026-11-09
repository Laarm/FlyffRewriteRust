// The secret-room windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-56, WP-UI-57): the lazy classes of `lazy.ts`. 12 layouts become `built`: 700, 704, 702, 672, 747, 710,
// 754, 760, 715, 716 and the two shells 751 / 752 (V15 comments them out). 702 and 672 share ONE class (`WndLineUpSelection`; the kind follows the app id the window is opened for).
import type { WndCtor } from '../../catalog';
import { SECRETROOM_LAZY } from './lazy';

export const SECRETROOM_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_SECRETROOM_OFFER: () => SECRETROOM_LAZY.WndSecretRoomOffer.load(),
  APP_SECRETROOM_OFFERSTATE: () => SECRETROOM_LAZY.WndSecretRoomOfferState.load(),
  APP_SECRETROOM_SELECTION: () => SECRETROOM_LAZY.WndLineUpSelection.load(),
  APP_GUILDCOMBAT_1TO1_SELECTION: () => SECRETROOM_LAZY.WndLineUpSelection.load(),
  APP_SECRETROOM_BOARD: () => SECRETROOM_LAZY.WndSecretRoomBoard.load(),
  APP_SECRETROOM_MSG: () => SECRETROOM_LAZY.WndSecretRoomInfoBox.load(),
  APP_SECRETROOM_QUICK: () => SECRETROOM_LAZY.WndSecretRoomQuick.load(),
  APP_SECRETROOM_CANCEL_CONFIRM: () => SECRETROOM_LAZY.WndSecretRoomCancelConfirm.load(),
  APP_SECRETROOM_TAXRATE_CHANGE: () => SECRETROOM_LAZY.WndSecretRoomChangeTaxRate.load(),
  APP_SECRETROOM_TAXRATE_CHECK: () => SECRETROOM_LAZY.WndSecretRoomCheckTaxRate.load(),
  APP_SECRETROOM_GUILDMEM_MNG: () => SECRETROOM_LAZY.WndSecretRoomGuildMemMng.load(),
  APP_SECRETROOM_GUILDMEMBER: () => SECRETROOM_LAZY.WndSecretRoomGuildMember.load(),
};
