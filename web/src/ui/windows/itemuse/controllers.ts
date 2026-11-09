// The item-use dialogs and the buff status window of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-28): the lazy classes of `lazy.ts`;
// the two applet windows (the duration list, the buff status window) are built by the applet registry synchronously, so their classes are plain imports.
import type { WndCtor } from '../../catalog';
import { ITEMUSE_LAZY } from './lazy';
import { WndBuffStatus } from './WndBuffStatus';
import { WndCommItem } from './WndCommItem';

export const ITEMUSE_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_EQUIP_BIND_CONFIRM: () => ITEMUSE_LAZY.WndEquipBindConfirm.load(),
  APP_QUESTITEM_INFO: () => ITEMUSE_LAZY.WndQuestItemInfo.load(),
  APP_COMM_SELECTVIL: () => ITEMUSE_LAZY.WndSelectVillage.load(),
  APP_COMM_ITEM: () => Promise.resolve(WndCommItem),
  APP_SUMMON_ANGEL: () => ITEMUSE_LAZY.WndSummonAngel.load(),
  APP_RANDOMSCROLL_CONFIRM: () => ITEMUSE_LAZY.WndRandomScrollConfirm.load(),
  APP_SMELT_SAFETY_CONFIRM: () => ITEMUSE_LAZY.WndSmeltSafetyConfirm.load(),
  APP_BUFF_STATUS: () => Promise.resolve(WndBuffStatus),
  APP_REMOVE_ELEM: () => ITEMUSE_LAZY.WndRemoveElem.load(),
  APP_RESTATE_CONFIRM: () => ITEMUSE_LAZY.WndRestateConfirm.load(),
  APP_FUNNYCOIN_CONFIRM: () => ITEMUSE_LAZY.WndFunnyCoinConfirm.load(),
  APP_CONFIRM_ENTER: () => ITEMUSE_LAZY.WndConfirmEnter.load(),
  APP_SELECT_CHANNEL: () => ITEMUSE_LAZY.WndSelectChannel.load(),
  APP_CHANGENAME: () => ITEMUSE_LAZY.WndChangeName.load(),
  APP_CHANGE_ATTRIBUTE: () => ITEMUSE_LAZY.WndChangeAttribute.load(),
  APP_RANDOMOPTIONRETRY: () => ITEMUSE_LAZY.WndRandomOptionRetry.load(),
};
