// `NpcWindowsService` (`ui.npc`, Plan/42 §19.4 E; WP-UI-24): the NPC dialog, the shop and their confirmation windows.
//
//   ui.npc.openDialog({view, actions})        APP_DIALOG_EX (modal): the conversation with an NPC
//   ui.npc.openShop({view, actions})          APP_SHOPEX: the four tabs of a vendor, sell by dropping an item
//   ui.npc.openConfirmBuy(request)            APP_CONFIRM_BUYEX: the quantity and the price of a purchase
//   ui.npc.openConfirmSell(request)           APP_CONFIRM_SELL: the quantity and the price of a sale
//   ui.npc.openWarning(request)               APP_WARNING: the premium-item warning before a sale
//   ui.npc.openQuickSell({view, actions})     APP_QUICK_SELL: the items a quick sale takes and the total
//   ui.npc.openHairShop({view, actions})      APP_BEAUTY_SHOP_EX: the hair style and colour
//   ui.npc.openFaceShop({view, actions})      APP_BEAUTY_SHOP_SKIN: the face
//   ui.npc.openBeautyConfirm(request)         APP_BEAUTY_SHOP_EX_CONFIRM: "change your hair / appearance?"
//   ui.npc.moverMenu                          the popup of the menu actions of a player or an NPC
//
// Each `open*` replaces the window of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`).

import {
  APP_BEAUTY_SHOP_EX,
  APP_BEAUTY_SHOP_EX_CONFIRM,
  APP_BEAUTY_SHOP_SKIN,
  APP_CONFIRM_BUYEX,
  APP_CONFIRM_SELL,
  APP_DIALOG_EX,
  APP_QUICK_SELL,
  APP_SHOPEX,
  APP_WARNING,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { NPC_LAZY } from './lazy';
import { MoverMenu } from './MoverMenu';
import type {
  BeautyConfirmRequest,
  BeautyPreviews,
  ConfirmBuyRequest,
  ConfirmSellRequest,
  DialogActions,
  DialogView,
  FacePreviewSubjects,
  FaceShopActions,
  FaceShopView,
  HairPreviewSubjects,
  HairShopActions,
  HairShopView,
  QuickSellActions,
  QuickSellView,
  ShopActions,
  ShopView,
  ViewSource,
  WarningRequest,
} from './ports';
import type { WndBeautyConfirm } from './WndBeautyConfirm';
import type { WndBeautyShop } from './WndBeautyShop';
import type { WndConfirmBuy } from './WndConfirmBuy';
import type { WndConfirmSell } from './WndConfirmSell';
import type { WndDialogEx } from './WndDialogEx';
import type { WndFaceShop } from './WndFaceShop';
import type { WndQuickSell } from './WndQuickSell';
import type { WndShop } from './WndShop';
import type { WndWarning } from './WndWarning';

export * from './ports';
export { MoverMenu, type MoverMenuItem } from './MoverMenu';
export { BEAUTY_WARNING_COLOR } from './beautyColors';
export type { WndBeautyConfirm } from './WndBeautyConfirm';
export type { WndBeautyShop } from './WndBeautyShop';
export { BeautyCard } from './BeautyCard';
export { placeBesideInventory, turntableDegrees } from './shopLayout';
export { ChannelSlider } from './ChannelSlider';
export type { WndFaceShop } from './WndFaceShop';
export type { WndConfirmBuy } from './WndConfirmBuy';
export type { WndConfirmSell } from './WndConfirmSell';
export type { WndDialogEx } from './WndDialogEx';
export type { WndQuickSell } from './WndQuickSell';
export type { WndShop } from './WndShop';
export type { WndWarning } from './WndWarning';

export interface NpcWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class NpcWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;
  #menu: MoverMenu | null = null;

  constructor(deps: NpcWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
  }

  /** The popup of the mover menus (`m_wndMenuMover`), created on first use. */
  get moverMenu(): MoverMenu {
    if (!this.#menu || this.#menu.destroyed) {
      const m = new MoverMenu();
      m.attach(this.#wm.root);
      this.#menu = m;
    }
    return this.#menu;
  }

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  /** Opens the window of a lazy class (`null` while its chunk is still loading: the window then opens by itself). */
  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  openDialog(req: {
    readonly view: ViewSource<DialogView>;
    readonly actions: DialogActions;
  }): WndDialogEx | null {
    return this.#open(
      APP_DIALOG_EX,
      NPC_LAZY.WndDialogEx,
      (C) => new C({ ...req, tooltip: this.#tooltip, getText: this.#getText }),
    );
  }

  openShop(req: {
    readonly view: ViewSource<ShopView>;
    readonly actions: ShopActions;
  }): WndShop | null {
    return this.#open(
      APP_SHOPEX,
      NPC_LAZY.WndShop,
      (C) => new C({ ...req, tooltip: this.#tooltip, getText: this.#getText }),
    );
  }

  openConfirmBuy(request: ConfirmBuyRequest): WndConfirmBuy | null {
    return this.#open(APP_CONFIRM_BUYEX, NPC_LAZY.WndConfirmBuy, (C) => new C(request));
  }

  openConfirmSell(request: ConfirmSellRequest): WndConfirmSell | null {
    return this.#open(APP_CONFIRM_SELL, NPC_LAZY.WndConfirmSell, (C) => new C(request));
  }

  openWarning(request: WarningRequest): WndWarning | null {
    return this.#open(APP_WARNING, NPC_LAZY.WndWarning, (C) => new C(request));
  }

  openHairShop(req: {
    readonly view: ViewSource<HairShopView>;
    readonly actions: HairShopActions;
    readonly previews?: BeautyPreviews<HairPreviewSubjects> | null;
  }): WndBeautyShop | null {
    return this.#open(
      APP_BEAUTY_SHOP_EX,
      NPC_LAZY.WndBeautyShop,
      (C) => new C({ ...req, getText: this.#getText }),
    );
  }

  openFaceShop(req: {
    readonly view: ViewSource<FaceShopView>;
    readonly actions: FaceShopActions;
    readonly previews?: BeautyPreviews<FacePreviewSubjects> | null;
  }): WndFaceShop | null {
    return this.#open(
      APP_BEAUTY_SHOP_SKIN,
      NPC_LAZY.WndFaceShop,
      (C) => new C({ ...req, getText: this.#getText }),
    );
  }

  openBeautyConfirm(request: BeautyConfirmRequest): WndBeautyConfirm | null {
    return this.#open(APP_BEAUTY_SHOP_EX_CONFIRM, NPC_LAZY.WndBeautyConfirm, (C) => new C(request));
  }

  openQuickSell(req: {
    readonly view: ViewSource<QuickSellView>;
    readonly actions: QuickSellActions;
  }): WndQuickSell | null {
    return this.#open(
      APP_QUICK_SELL,
      NPC_LAZY.WndQuickSell,
      (C) => new C({ ...req, tooltip: this.#tooltip, getText: this.#getText }),
    );
  }
}
