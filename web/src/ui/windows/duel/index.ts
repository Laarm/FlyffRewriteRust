// `DuelWindowsService` (`ui.duel`, Plan/42 §19.4 K / C; WP-UI-41): the duel request, the duel result and the resurrection offer.
//
//   ui.duel.openConfirm(request)        APP_DUEL_CONFIRM (modal, Enter = Yes): "<name> challenges you to a duel"
//   ui.duel.openResult(request)         APP_DUEL_RESULT: "<opponent> loses / wins the Duel!"
//   ui.duel.openResurrection(request)   APP_RESURRECTION (modal, no title bar): "do you want to be resurrected?"
//   ui.duel.closeConfirm()              the duel request is void (own death, the duel started): no answer is sent
//   ui.duel.closeResurrection()         the offer is void (a revival happened)
//
// Each `open*` replaces the window of the same kind that is still open (`SAFE_DELETE(m_pWndDuelConfirm); new ...`) without
// answering it. A resurrection offer is NOT replaced: `OnResurrectionMessage` opens the window only when none exists. The
// requests carry the texts and the callbacks; what the callbacks send is 45's (WP-CFEAT-36).

import { APP_DUEL_CONFIRM, APP_DUEL_RESULT, APP_RESURRECTION } from '../../../gen/defs/resdata';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { DUEL_LAZY } from './lazy';
import type { DuelConfirmRequest, DuelResultRequest, ResurrectionRequest } from './ports';
import type { WndDuelConfirm } from './WndDuelConfirm';
import type { WndDuelResult } from './WndDuelResult';
import type { WndResurrectionConfirm } from './WndResurrectionConfirm';

export * from './ports';
export { DUEL_WINDOW_Y } from './geometry';
export type { WndDuelConfirm } from './WndDuelConfirm';
export type { WndDuelResult } from './WndDuelResult';
export type { WndResurrectionConfirm } from './WndResurrectionConfirm';

export interface DuelWindowsDeps {
  readonly wm: WindowManager;
}

export class DuelWindowsService {
  readonly #wm: WindowManager;

  constructor(deps: DuelWindowsDeps) {
    this.#wm = deps.wm;
  }

  /** Closes the open window of an app so that a new one can take its place (no answer is sent). */
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

  openConfirm(request: DuelConfirmRequest): WndDuelConfirm | null {
    return this.#open(APP_DUEL_CONFIRM, DUEL_LAZY.WndDuelConfirm, (C) => new C(request));
  }

  openResult(request: DuelResultRequest): WndDuelResult | null {
    return this.#open(APP_DUEL_RESULT, DUEL_LAZY.WndDuelResult, (C) => new C(request));
  }

  /** `OnResurrectionMessage`: the window opens only when none is open; returns `null` for the second offer. */
  openResurrection(request: ResurrectionRequest): WndResurrectionConfirm | null {
    if (this.#live(APP_RESURRECTION)) return null;
    // an answered window waits for its removal until the next tick: it must not shadow the new offer
    this.#wm.flushDestroyed();
    return openLazy(
      this.#wm,
      APP_RESURRECTION,
      DUEL_LAZY.WndResurrectionConfirm,
      (C) => new C(request),
      () => undefined,
    );
  }

  /** The open window of an app that is not being closed (`GetWndBase`). */
  #live(app: number): WndNeuz | null {
    const w = this.#wm.get(app);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  isConfirmOpen(): boolean {
    return this.#live(APP_DUEL_CONFIRM) !== null;
  }

  isResultOpen(): boolean {
    return this.#live(APP_DUEL_RESULT) !== null;
  }

  isResurrectionOpen(): boolean {
    return this.#live(APP_RESURRECTION) !== null;
  }

  closeConfirm(): void {
    this.#wm.get(APP_DUEL_CONFIRM)?.destroy();
  }

  closeResult(): void {
    this.#wm.get(APP_DUEL_RESULT)?.destroy();
  }

  closeResurrection(): void {
    this.#wm.get(APP_RESURRECTION)?.destroy();
  }
}
