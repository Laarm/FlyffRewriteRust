// `ItemDialogsService` (`ui.dialogs`, Plan/42 §19.4 C/D; WP-UI-22): the death and item-handling dialogs.
//
//   ui.dialogs.openRevival({view, actions})         APP_REVIVAL (modal, no title bar, exempt from Esc)
//   ui.dialogs.openDropItem(request)                 APP_DROP_ITEM: how many to drop on the ground
//   ui.dialogs.openDropConfirm(request)              APP_DROP_CONFIRM: really drop this single item
//   ui.dialogs.openInvenRemove(request)              APP_INVEN_REMOVE_ITEM (modal): really delete (trash slot)
//   ui.dialogs.openQueItemWarning(request)           APP_QUEITMWARNING: quest item / skill reset / post item warning
//
// Each `open*` replaces the dialog of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`). The
// requests carry the data and the OK callback; what the callbacks send is 45's (WP-CFEAT-10, -12, -13).

import {
  APP_DROP_CONFIRM,
  APP_DROP_ITEM,
  APP_INVEN_REMOVE_ITEM,
  APP_QUEITMWARNING,
  APP_REVIVAL,
} from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { DIALOGS_LAZY } from './lazy';
import type {
  DropConfirmRequest,
  DropItemRequest,
  InvenRemoveRequest,
  QueItemWarningRequest,
  RevivalRequest,
} from './ports';
import type { WndDropConfirm } from './WndDropConfirm';
import type { WndDropItem } from './WndDropItem';
import type { WndInvenRemoveItem } from './WndInvenRemoveItem';
import type { WndQueItmWarning } from './WndQueItmWarning';
import type { WndRevival } from './WndRevival';

export * from './ports';
export type { WndDropConfirm } from './WndDropConfirm';
export type { WndDropItem } from './WndDropItem';
export type { WndInvenRemoveItem } from './WndInvenRemoveItem';
export type { WndQueItmWarning } from './WndQueItmWarning';
export type { WndRevival } from './WndRevival';

export interface ItemDialogsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class ItemDialogsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;
  readonly #getText: (tid: number) => string;

  constructor(deps: ItemDialogsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
    this.#getText = deps.getText;
  }

  /** Closes the open dialog of an app so that a new one can take its place. */
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

  openRevival(request: RevivalRequest): WndRevival | null {
    return this.#open(
      APP_REVIVAL,
      DIALOGS_LAZY.WndRevival,
      (C) => new C({ request, getText: this.#getText }),
    );
  }

  openDropItem(request: DropItemRequest): WndDropItem | null {
    return this.#open(APP_DROP_ITEM, DIALOGS_LAZY.WndDropItem, (C) => new C(request));
  }

  openDropConfirm(request: DropConfirmRequest): WndDropConfirm | null {
    return this.#open(APP_DROP_CONFIRM, DIALOGS_LAZY.WndDropConfirm, (C) => new C(request));
  }

  openInvenRemove(request: InvenRemoveRequest): WndInvenRemoveItem | null {
    return this.#open(
      APP_INVEN_REMOVE_ITEM,
      DIALOGS_LAZY.WndInvenRemoveItem,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }

  openQueItemWarning(request: QueItemWarningRequest): WndQueItmWarning | null {
    return this.#open(APP_QUEITMWARNING, DIALOGS_LAZY.WndQueItmWarning, (C) => new C(request));
  }
}
