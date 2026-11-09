// `Gc1to1WindowsService` (`ui.gc1to1`, Plan/42 §19.4 K; WP-UI-55): the shell of the 1:1 guild combat application status.
//
//   ui.gc1to1.openOfferState( request )     676: the applying guilds, the countdown, the own rank and tender (a `StateRequest` of the siege; an open one is replaced)
//   ui.gc1to1.offerState()                  the open window
//   ui.gc1to1.closeAll()                    the window goes
//
// The wiring (menus 60-70, `Gc1to1TenderViewNtf`, the flag gate and the nine `SendGC1to1*` sends) is `features/events/gc1to1` (WP-CFEAT-45, task ui-minigames).

import { APP_GUILDCOMBAT_1TO1_OFFERSTATE } from '../../../gen/defs/resdata';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import { MB, MBID } from '../../msgbox/types';
import type { WindowManager } from '../../wm/WindowManager';
import { openLazy } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { fmt } from '../siege/format';
import type { SiegeHost, StateRequest } from '../siege/ports';
import { GC1TO1_LAZY } from './lazy';
import type { WndGuildCombat1to1OfferState } from './WndGuildCombat1to1OfferState';

export type { WndGuildCombat1to1OfferState };
export { MAX_JOIN_GUILD_1TO1 } from './rows';

export interface Gc1to1WindowDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  readonly getText: (tid: number) => string;
}

/** The `APP_*` of every window of the family. */
export const GC1TO1_APPS: readonly number[] = [APP_GUILDCOMBAT_1TO1_OFFERSTATE];

export class Gc1to1WindowsService {
  readonly #wm: WindowManager;
  readonly #host: SiegeHost;

  constructor(deps: Gc1to1WindowDeps) {
    this.#wm = deps.wm;
    const { msgbox, getText } = deps;
    this.#host = {
      text: getText,
      fmt: (tid, ...args) => fmt(getText(tid), args),
      box: (text) => {
        msgbox.open(text);
      },
      confirm: (text, onOk, owner) => {
        msgbox.open(text, MB.OKCANCEL, (owner as WndNeuz | undefined) ?? undefined, (id) => {
          if (id === MBID.OK) onOk();
        });
      },
    };
  }

  /** The texts and boxes the shell uses (tests). */
  get host(): SiegeHost {
    return this.#host;
  }

  /** `SAFE_DELETE( m_pWndGuildCombatState ); new CWndGuildCombatState( 1 )`: opens the window, replacing an open one; `null` while its chunk is loading (it then opens by itself). */
  openOfferState(request: StateRequest): WndGuildCombat1to1OfferState | null {
    return openLazy(
      this.#wm,
      APP_GUILDCOMBAT_1TO1_OFFERSTATE,
      GC1TO1_LAZY.WndGuildCombat1to1OfferState,
      (C) => new C(request, { host: this.#host }),
      (app) => {
        const old = this.#wm.get(app);
        if (!old) return;
        old.destroy();
        this.#wm.flushDestroyed();
      },
    );
  }

  /** The open window. */
  offerState(): WndGuildCombat1to1OfferState | null {
    const w = this.#wm.get(APP_GUILDCOMBAT_1TO1_OFFERSTATE);
    const ctor = GC1TO1_LAZY.WndGuildCombat1to1OfferState.ctor;
    return ctor && w instanceof ctor && !this.#wm.isDestroying(w) ? w : null;
  }

  isOpen(): boolean {
    return this.offerState() !== null;
  }

  /** The window goes (the character left the world). */
  closeAll(): void {
    for (const app of GC1TO1_APPS) this.#wm.get(app)?.destroy();
    this.#wm.flushDestroyed();
  }
}
