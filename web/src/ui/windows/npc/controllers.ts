// The NPC dialog and shop windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-24): the lazy classes of `lazy.ts`.
import type { WndCtor } from '../../catalog';
import { NPC_LAZY } from './lazy';

export const NPC_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_DIALOG_EX: () => NPC_LAZY.WndDialogEx.load(),
  APP_SHOPEX: () => NPC_LAZY.WndShop.load(),
  APP_CONFIRM_BUYEX: () => NPC_LAZY.WndConfirmBuy.load(),
  APP_CONFIRM_SELL: () => NPC_LAZY.WndConfirmSell.load(),
  APP_WARNING: () => NPC_LAZY.WndWarning.load(),
  APP_QUICK_SELL: () => NPC_LAZY.WndQuickSell.load(),
  APP_BEAUTY_SHOP_EX: () => NPC_LAZY.WndBeautyShop.load(),
  APP_BEAUTY_SHOP_SKIN: () => NPC_LAZY.WndFaceShop.load(),
  APP_BEAUTY_SHOP_EX_CONFIRM: () => NPC_LAZY.WndBeautyConfirm.load(),
};
