// `features/npc` (WP-CFEAT-18, -19): the NPC click menus, the dialog window and the NPC shop. See `npcModule.ts`.
export { npcModule, beautyOf, dialogOf, menuOf, shopOf } from './npcModule';
export {
  EVENT,
  HIDDEN_NPC_MENUS,
  menuLabel,
  playerMenuIds,
  popupMenus,
  resolveNpcClick,
  visibleNpcMenus,
  type NpcClick,
  type NpcMenuEnv,
  type PlayerMenuEnv,
} from './menu';
export type { DialogFeature, QuestInfo } from './dialog/dialogModule';
export type { MenuAction, MenuActionArgs, MenuFeature } from './menuModule';
export type { ShopFeature } from './shop/shopModule';
export type { BeautyFeature } from './beauty/beautyModule';
export { busyText, BUSY_WINDOWS } from './busy';
export { maxBuyQuantity } from './shop/shopModule';
export {
  acceptsOps,
  applyDialogOps,
  beginText,
  clickAnswer,
  clickKey,
  clickLink,
  clickQuest,
  defineDialogStore,
  initialDialogState,
  mayClick,
  nextPage,
  type DialogEffect,
  type DialogEnv,
  type DialogState,
  type DialogStore,
} from './dialog/state';
export { parseDialogPage, sayMarkup } from '../../ui/windows/npc/dialogText';
export {
  applyOpenShop,
  applyUpdateVendor,
  applyVendor,
  defineShopStore,
  initialShopState,
  parseTab,
  type ShopState,
  type ShopStockEntry,
  type ShopStore,
} from './shop/state';
export { quickSellCandidates, quickSellTotal } from './shop/quickSell';
