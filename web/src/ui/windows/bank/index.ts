// `BankWindowsService` (`ui.bank`, Plan/42 §19.4 E; WP-UI-25): the bank, the PIN entry and the PIN window.
//
//   ui.bank.openBank(request)       APP_COMMON_BANK: three compartments with 42 slots and the penya of each
//   ui.bank.openConfirm(request)    APP_CONFIRM_BANK: the four-digit PIN
//   ui.bank.openPassword(request)   APP_BANK_PASSWORD: the first PIN or a PIN change
//   ui.bank.closeAll()              the three windows go (`SAFE_DELETE` of `OnBankWindow` / `OnConfirmBankPass` / `OnChangeBankPass`)
//
// Each `open*` replaces the window of the same kind. The requests carry the view and the callbacks; what they send is 45's (`features/bank`). The
// shells load on demand (`lazy.ts`): only their types are exported here.

import { APP_BANK_PASSWORD, APP_COMMON_BANK, APP_CONFIRM_BANK } from '../../../gen/defs/resdata';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { BANK_LAZY } from './lazy';
import type { BankPasswordRequest, BankRequest, ConfirmBankRequest } from './ports';
import type { WndBank } from './WndBank';
import type { WndBankPassword } from './WndBankPassword';
import type { WndConfirmBank } from './WndConfirmBank';

export * from './ports';
export { checkChangePin, checkConfirmPin, isPin } from './pin';
export {
  BANK_COIN_NAMES,
  BANK_GOLD_NAMES,
  BANK_NOUSE_NAMES,
  BANK_TAB_NAMES,
  DEFAULT_PIN,
  LEFT_OF_INVENTORY,
  NOUSE_ALPHA,
  NOUSE_TEXTURE,
} from './names';
export type { WndBank, WndBankPassword, WndConfirmBank };

export interface BankWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly msgbox: Pick<MessageBoxApi, 'open'>;
  /** `getText(tid)` of the texts bundle. */
  readonly getText: (tid: number) => string;
}

export class BankWindowsService {
  readonly #wm: WindowManager;
  readonly #deps: BankWindowsDeps;

  constructor(deps: BankWindowsDeps) {
    this.#wm = deps.wm;
    this.#deps = deps;
  }

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    // the next window of a PIN flow replaces this one: its `closed` must say so
    (old as { markReplaced?: () => void }).markReplaced?.();
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

  openBank(request: BankRequest): WndBank | null {
    return this.#open(
      APP_COMMON_BANK,
      BANK_LAZY.WndBank,
      (C) =>
        new C({
          view: request.view,
          actions: request.actions,
          tooltip: this.#deps.tooltip,
          getText: this.#deps.getText,
        }),
    );
  }

  openConfirm(request: ConfirmBankRequest): WndConfirmBank | null {
    return this.#open(
      APP_CONFIRM_BANK,
      BANK_LAZY.WndConfirmBank,
      (C) => new C({ msgbox: this.#deps.msgbox, getText: this.#deps.getText }, request),
    );
  }

  openPassword(request: BankPasswordRequest): WndBankPassword | null {
    return this.#open(
      APP_BANK_PASSWORD,
      BANK_LAZY.WndBankPassword,
      (C) => new C({ msgbox: this.#deps.msgbox, getText: this.#deps.getText }, request),
    );
  }

  /** The three windows of the flow, as they are open now. */
  get windows(): {
    readonly bank: WndBank | null;
    readonly confirm: WndConfirmBank | null;
    readonly password: WndBankPassword | null;
  } {
    return {
      bank: (this.#wm.get(APP_COMMON_BANK) as WndBank | null) ?? null,
      confirm: (this.#wm.get(APP_CONFIRM_BANK) as WndConfirmBank | null) ?? null,
      password: (this.#wm.get(APP_BANK_PASSWORD) as WndBankPassword | null) ?? null,
    };
  }

  /**
   * `SAFE_DELETE( m_pWndBank ); SAFE_DELETE( m_pWndConfirmBank ); SAFE_DELETE( m_pWndBankPassword )`: the PIN windows report `replaced`
   * (`how = 'replaced'`, the session goes on) or `user` (a transactional close: the session ends).
   */
  closeAll(how: 'replaced' | 'user' = 'replaced'): void {
    for (const app of [APP_COMMON_BANK, APP_CONFIRM_BANK, APP_BANK_PASSWORD]) {
      const w = this.#wm.get(app);
      if (!w) continue;
      if (how === 'replaced') (w as { markReplaced?: () => void }).markReplaced?.();
      w.destroy();
    }
    this.#wm.flushDestroyed();
  }
}
