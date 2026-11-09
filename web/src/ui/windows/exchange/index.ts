// `ExchangeWindowsService` (`ui.exchange`, Plan/42 §19.4 L, WP-UI-36 shell; WP-CFEAT-26): the item-exchange list of an NPC.
//
//   ui.exchange.open(request)    APP_DIALOG_EVENT: the description, the list of the script, OK / Cancel
//   ui.exchange.window()         the open window, `null` when none (the `ExchangeNtf` answer asks for it)
//
// `open` replaces the window that is still open (the original's `SAFE_DELETE( m_pWndDialogEvent ); new ...`). The request carries the texts and the
// callbacks; the send is 45's (`features/exchange`).

import { APP_DIALOG_EVENT } from '../../../gen/defs/resdata';
import type { WindowManager } from '../../wm/WindowManager';
import { openLazy } from '../../wnd/lazy';
import { EXCHANGE_LAZY } from './lazy';
import type { ExchangeRequest } from './ports';
import type { WndDialogEvent } from './WndDialogEvent';

export * from './ports';
export * from './geometry';
// the shell loads on demand (`lazy.ts`): only its type is exported here
export type { WndDialogEvent };

export interface ExchangeWindowsDeps {
  readonly wm: WindowManager;
}

export class ExchangeWindowsService {
  readonly #wm: WindowManager;

  constructor(deps: ExchangeWindowsDeps) {
    this.#wm = deps.wm;
  }

  /** Opens the list (`null` while the chunk of the window is still loading: the window then opens by itself). */
  open(request: ExchangeRequest): WndDialogEvent | null {
    return openLazy(
      this.#wm,
      APP_DIALOG_EVENT,
      EXCHANGE_LAZY.WndDialogEvent,
      (C) => new C(request),
      (a) => {
        const old = this.#wm.get(a);
        if (!old) return;
        old.destroy();
        this.#wm.flushDestroyed();
      },
    );
  }

  window(): WndDialogEvent | null {
    return this.#wm.get(APP_DIALOG_EVENT) as WndDialogEvent | null;
  }
}
