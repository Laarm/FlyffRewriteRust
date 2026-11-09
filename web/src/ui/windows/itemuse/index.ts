// `ItemUseWindowsService` (`ui.itemUse`, Plan/42 §19.4 D, §19.5; WP-UI-28): the item-use dialogs and the buff windows.
//
//   ui.itemUse.openEquipBindConfirm(request)   APP_EQUIP_BIND_CONFIRM (modal, Enter = Yes): the soul-link confirm
//   ui.itemUse.openQuestItemInfo(request)      APP_QUESTITEM_INFO: picture, name, description of a quest item
//   ui.itemUse.openSelectVillage(request)      APP_COMM_SELECTVIL: the destination of a return scroll
//   ui.itemUse.openCommItem(request)           APP_COMM_ITEM: the item duration list
//   ui.itemUse.openSummonAngel(request)        APP_SUMMON_ANGEL: the angel summoning window
//   ui.itemUse.openRandomScrollConfirm(request) / openSmeltSafetyConfirm(request)   the text and Yes / No windows of the upgrade flows
//   ui.itemUse.openRemoveElem / openRestateConfirm / openFunnyCoinConfirm / openConfirmEnter(request)   APP_REMOVE_ELEM, APP_RESTATE_CONFIRM,
//                                              APP_FUNNYCOIN_CONFIRM, APP_CONFIRM_ENTER: a text and Yes / No (WP-CFEAT-15)
//   ui.itemUse.openSelectChannel(request)      APP_SELECT_CHANNEL: the ticket layer list
//   ui.itemUse.openChangeName(request)         APP_CHANGENAME: the pet naming / character rename edit
//   ui.itemUse.openChangeAttribute(request)    APP_CHANGE_ATTRIBUTE: the element-change scroll (target slot and five element icons)
//   ui.itemUse.openRandomOptionRetry(request)  APP_RANDOMOPTIONRETRY: the awakening augmentation scroll
//   ui.itemUse.setBuffs(actions)               installs the buff feature's port
//   ui.itemUse.showBuffStrip(on)               opens / closes the strip window under the windows (icons of the world view)
//   ui.itemUse.showBuffStatus(on)              opens / closes APP_BUFF_STATUS (the world does it by "any skill buff")
//
// Each `open*` replaces the dialog of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`). The
// requests carry the data and the callbacks; what the callbacks send is 45's (WP-CFEAT-13, -30).

import {
  APP_BUFF_STATUS,
  APP_CHANGE_ATTRIBUTE,
  APP_CHANGENAME,
  APP_COMM_ITEM,
  APP_COMM_SELECTVIL,
  APP_CONFIRM_ENTER,
  APP_EQUIP_BIND_CONFIRM,
  APP_FUNNYCOIN_CONFIRM,
  APP_QUESTITEM_INFO,
  APP_RANDOMOPTIONRETRY,
  APP_RANDOMSCROLL_CONFIRM,
  APP_REMOVE_ELEM,
  APP_RESTATE_CONFIRM,
  APP_SELECT_CHANNEL,
  APP_SMELT_SAFETY_CONFIRM,
  APP_SUMMON_ANGEL,
} from '../../../gen/defs/resdata';
import { constantView } from '../char/ports';
import type { ViewSource } from '../char/ports';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { ITEMUSE_LAZY } from './lazy';
import type {
  BuffActions,
  ChangeAttributeRequest,
  ChangeNameRequest,
  CommItemRequest,
  CommItemView,
  EquipBindConfirmRequest,
  QuestItemInfoRequest,
  RandomOptionRetryRequest,
  SelectChannelRequest,
  SelectVillageRequest,
  SummonAngelRequest,
  YesNoTextRequest,
} from './ports';
import { WndBuffStatus } from './WndBuffStatus';
import { APP_BUFF_STRIP, WndBuffStrip } from './WndBuffStrip';
import { WndCommItem } from './WndCommItem';
import type { WndEquipBindConfirm } from './WndEquipBindConfirm';
import type { WndQuestItemInfo } from './WndQuestItemInfo';
import type { WndSelectVillage } from './WndSelectVillage';
import type { WndSummonAngel } from './WndSummonAngel';
import type { WndChangeAttribute } from './WndChangeAttribute';
import type { WndChangeName } from './WndChangeName';
import type { WndRandomOptionRetry } from './WndRandomOptionRetry';
import type { WndSelectChannel } from './WndSelectChannel';
import type {
  WndConfirmEnter,
  WndFunnyCoinConfirm,
  WndRandomScrollConfirm,
  WndRemoveElem,
  WndRestateConfirm,
  WndSmeltSafetyConfirm,
} from './WndYesNoText';

export * from './ports';
export { WndBuffStatus, buffSlotPoint, MAX_SKILLBUFF_COUNT } from './WndBuffStatus';
export { WndBuffStrip, APP_BUFF_STRIP } from './WndBuffStrip';
export { WndCommItem, CommItemPage, commItemEntries } from './WndCommItem';
export type { WndEquipBindConfirm } from './WndEquipBindConfirm';
export type { WndQuestItemInfo } from './WndQuestItemInfo';
export type { WndSelectVillage } from './WndSelectVillage';
export { angelRates, MAX_MATDIE } from './angelRates';
export type { WndSummonAngel } from './WndSummonAngel';
export type { WndChangeAttribute } from './WndChangeAttribute';
export type { WndChangeName } from './WndChangeName';
export type { WndRandomOptionRetry } from './WndRandomOptionRetry';
export type { WndSelectChannel } from './WndSelectChannel';
export type {
  WndConfirmEnter,
  WndFunnyCoinConfirm,
  WndRandomScrollConfirm,
  WndRemoveElem,
  WndRestateConfirm,
  WndSmeltSafetyConfirm,
} from './WndYesNoText';
export { buffEntry, BUFF_ICON_SIZE, BUFF_TIP_SIZE, EVIL_TINT } from './buffEntries';

/** The buff port before the feature installs the real one: no buffs. */
export const NULL_BUFFS: BuffActions = {
  hud: () => ({ icons: [], cast: null }),
  status: constantView({ icons: [] }),
  tooltip: () => null,
  activate: () => undefined,
  mode: () => 0,
  setMode: () => undefined,
  subscribe: () => () => undefined,
};

export interface ItemUseWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class ItemUseWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;
  #buffs: BuffActions | null = null;
  #commView: ViewSource<CommItemView> = constantView({ rows: [] });

  constructor(deps: ItemUseWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
    // the applets of the start menu and the hotkeys (`APP_COMM_ITEM` 'J', `APP_BUFF_STATUS`) open with the installed ports
    const applets = deps.wm.applets;
    applets.register({
      app: APP_COMM_ITEM,
      factory: () =>
        new WndCommItem({
          request: { view: this.#commView },
          tooltip: this.#tooltip,
          getText: this.#getText,
        }),
    });
    applets.register({
      app: APP_BUFF_STATUS,
      factory: () =>
        new WndBuffStatus({ actions: this.#buffs ?? NULL_BUFFS, tooltip: this.#tooltip }),
    });
  }

  /** Installs the rows of the duration list (`APP_COMM_ITEM` opened from the start menu or its hotkey shows them). */
  setCommItemView(view: ViewSource<CommItemView>): void {
    this.#commView = view;
  }

  /** Closes the open dialog of an app so that a new one can take its place. */
  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  /** A window whose class is always loaded (the applets): built at once. */
  #openNow<T extends WndNeuz>(app: number, build: () => T): T | null {
    this.#replace(app);
    return this.#wm.open(app, build);
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

  openEquipBindConfirm(request: EquipBindConfirmRequest): WndEquipBindConfirm | null {
    return this.#open(
      APP_EQUIP_BIND_CONFIRM,
      ITEMUSE_LAZY.WndEquipBindConfirm,
      (C) => new C({ request, getText: this.#getText }),
    );
  }

  openQuestItemInfo(request: QuestItemInfoRequest): WndQuestItemInfo | null {
    return this.#open(APP_QUESTITEM_INFO, ITEMUSE_LAZY.WndQuestItemInfo, (C) => new C(request));
  }

  openSelectVillage(request: SelectVillageRequest): WndSelectVillage | null {
    return this.#open(APP_COMM_SELECTVIL, ITEMUSE_LAZY.WndSelectVillage, (C) => new C(request));
  }

  openCommItem(request: CommItemRequest): WndCommItem | null {
    return this.#openNow(
      APP_COMM_ITEM,
      () => new WndCommItem({ request, tooltip: this.#tooltip, getText: this.#getText }),
    );
  }

  openSummonAngel(request: SummonAngelRequest): WndSummonAngel | null {
    return this.#open(
      APP_SUMMON_ANGEL,
      ITEMUSE_LAZY.WndSummonAngel,
      (C) => new C(request, this.#getText),
    );
  }

  openRandomScrollConfirm(request: YesNoTextRequest): WndRandomScrollConfirm | null {
    return this.#open(
      APP_RANDOMSCROLL_CONFIRM,
      ITEMUSE_LAZY.WndRandomScrollConfirm,
      (C) => new C(request),
    );
  }

  openSmeltSafetyConfirm(request: YesNoTextRequest): WndSmeltSafetyConfirm | null {
    return this.#open(
      APP_SMELT_SAFETY_CONFIRM,
      ITEMUSE_LAZY.WndSmeltSafetyConfirm,
      (C) => new C(request),
    );
  }

  openRemoveElem(request: YesNoTextRequest): WndRemoveElem | null {
    return this.#open(APP_REMOVE_ELEM, ITEMUSE_LAZY.WndRemoveElem, (C) => new C(request));
  }

  openRestateConfirm(request: YesNoTextRequest): WndRestateConfirm | null {
    return this.#open(APP_RESTATE_CONFIRM, ITEMUSE_LAZY.WndRestateConfirm, (C) => new C(request));
  }

  openFunnyCoinConfirm(request: YesNoTextRequest): WndFunnyCoinConfirm | null {
    return this.#open(
      APP_FUNNYCOIN_CONFIRM,
      ITEMUSE_LAZY.WndFunnyCoinConfirm,
      (C) => new C(request),
    );
  }

  openConfirmEnter(request: YesNoTextRequest): WndConfirmEnter | null {
    return this.#open(APP_CONFIRM_ENTER, ITEMUSE_LAZY.WndConfirmEnter, (C) => new C(request));
  }

  openSelectChannel(request: SelectChannelRequest): WndSelectChannel | null {
    return this.#open(APP_SELECT_CHANNEL, ITEMUSE_LAZY.WndSelectChannel, (C) => new C(request));
  }

  openChangeName(request: ChangeNameRequest): WndChangeName | null {
    return this.#open(APP_CHANGENAME, ITEMUSE_LAZY.WndChangeName, (C) => new C(request));
  }

  openChangeAttribute(request: ChangeAttributeRequest): WndChangeAttribute | null {
    return this.#open(
      APP_CHANGE_ATTRIBUTE,
      ITEMUSE_LAZY.WndChangeAttribute,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openRandomOptionRetry(request: RandomOptionRetryRequest): WndRandomOptionRetry | null {
    return this.#open(
      APP_RANDOMOPTIONRETRY,
      ITEMUSE_LAZY.WndRandomOptionRetry,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  /** The open summon window (the inventory feeds it with a double click). */
  summonAngel(): WndSummonAngel | null {
    return this.#wm.get(APP_SUMMON_ANGEL) as WndSummonAngel | null;
  }

  // ---- buffs ---------------------------------------------------------------------------------------------------------------

  /** Installs (or removes) the buff port the windows read; without it both buff windows close. */
  setBuffs(actions: BuffActions | null): void {
    this.#buffs = actions;
    if (!actions) {
      this.#wm.close(APP_BUFF_STRIP);
      this.#wm.close(APP_BUFF_STATUS);
    }
  }

  /** The strip under the windows opens with the world and closes with it (the gallery opens it at once). */
  showBuffStrip(on: boolean): WndBuffStrip | null {
    if (!on) {
      this.#wm.close(APP_BUFF_STRIP);
      return null;
    }
    const actions = this.#buffs;
    if (!actions) return null;
    return this.#wm.open(
      APP_BUFF_STRIP,
      () => new WndBuffStrip({ actions, tooltip: this.#tooltip }),
    );
  }

  /** The world opens the status window while the player has a skill buff and closes it when none is left. */
  showBuffStatus(on: boolean): WndBuffStatus | null {
    if (!on) {
      this.#wm.close(APP_BUFF_STATUS);
      return null;
    }
    const actions = this.#buffs;
    if (!actions) return null;
    return this.#wm.open(
      APP_BUFF_STATUS,
      () => new WndBuffStatus({ actions, tooltip: this.#tooltip }),
    );
  }

  buffStatus(): WndBuffStatus | null {
    return this.#wm.get(APP_BUFF_STATUS) as WndBuffStatus | null;
  }

  buffStrip(): WndBuffStrip | null {
    return this.#wm.get(APP_BUFF_STRIP) as WndBuffStrip | null;
  }
}
