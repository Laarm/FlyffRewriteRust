// `SiegeWindowsService` (`ui.siege`, Plan/42 §19.4 K; WP-UI-49, -50, -51): the guild-siege windows.
//
//   ui.siege.openOffer( request )             675 (545 with `classic`): the bid window, mode 0 = the siege, mode 1 = the 1:1 tender (WP-CFEAT-45)
//   ui.siege.openOfferState( request )        547: the applying guilds, the countdown, the own rank and bid
//   ui.siege.openSelection( request )         551: the line-up editor            ui.siege.openJoinSelection( request )   557: the gate choice with its timeout
//   ui.siege.openBoard( request )             559: the information boards        ui.siege.openInfoBox / openInfoBox2     563 (with a leave button) / 565
//   ui.siege.openCancelConfirm / openJoinConfirm   561 / the join confirm on the layout of 266
//   ui.siege.openResult( request )            567 with the pages 569 and 571     ui.siege.openRankPerson( request )      575 with five pages 555
//   ui.siege.openRanking / openWarState       521 / 519: shells that V15 never opens
//   ui.siege.closeAll()                       every siege window goes
//
// Each `open*` replaces the open window of the same kind (the original: `SAFE_DELETE( m_pWndX ); new ...`) and answers `null` while the chunk of the window is still loading (the window then
// opens by itself).

import {
  APP_CLOSE_EXISTING_CONNECTION,
  APP_GUILD_WAR_STATE,
  APP_GUILDCOMBAT_1TO1_OFFER,
  APP_GUILDCOMBAT_BOARD,
  APP_GUILDCOMBAT_CANCEL_CONFIRM,
  APP_GUILDCOMBAT_MSG,
  APP_GUILDCOMBAT_MSG2,
  APP_GUILDCOMBAT_OFFER,
  APP_GUILDCOMBAT_OFFERSTATE,
  APP_GUILDCOMBAT_RANK_P,
  APP_GUILDCOMBAT_RANKING,
  APP_GUILDCOMBAT_RESULT_POINT,
  APP_GUILDCOMBAT_SELECTION,
  APP_GUILDCOMBAT_SELSTART,
} from '../../../gen/defs/resdata';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { MB, MBID } from '../../msgbox/types';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import { fmt } from './format';
import { SIEGE_LAZY } from './lazy';
import type {
  BoardRequest,
  CancelRequest,
  InfoBox2Request,
  InfoBoxRequest,
  JoinRequest,
  OfferRequest,
  RankingRequest,
  RankPersonRequest,
  ResultRequest,
  SelectionRequest,
  SelStartRequest,
  SiegeHost,
  StateRequest,
  WarStateRequest,
} from './ports';
import type { WndGuildCombatBoard } from './WndGuildCombatBoard';
import type { WndGuildCombatInfoBox } from './WndGuildCombatInfoBox';
import type { WndGuildCombatInfoBox2 } from './WndGuildCombatInfoBox2';
import type { WndGuildCombatJoinSelection } from './WndGuildCombatJoinSelection';
import type { WndGuildCombatOffer } from './WndGuildCombatOffer';
import type { WndGuildCombatRankPerson } from './WndGuildCombatRankPerson';
import type { WndGuildCombatRanking } from './WndGuildCombatRanking';
import type { WndGuildCombatResult } from './WndGuildCombatResult';
import type { WndGuildCombatSelection } from './WndGuildCombatSelection';
import type { WndGuildCombatState } from './WndGuildCombatState';
import type { WndGuildWarCancelConfirm } from './WndGuildWarCancelConfirm';
import type { WndGuildWarJoinConfirm } from './WndGuildWarJoinConfirm';
import type { WndGuildWarState } from './WndGuildWarState';

export * from './ports';
export * from './format';
export * from './rows';
export {
  WndGuildCombatRankClass,
  RANK_COLUMNS,
  RANK_MAX_ROWS,
  RANK_PAGE_ROWS,
  rankTableRow,
} from './WndGuildCombatRankClass';
export { WndGuildCombatTabResultLog } from './WndGuildCombatTabResultLog';
export { WndGuildCombatTabResultRate } from './WndGuildCombatTabResultRate';
export { TableList } from './TableList';
export type { TableRow, TableCell } from './TableList';
export type {
  WndGuildCombatBoard,
  WndGuildCombatInfoBox,
  WndGuildCombatInfoBox2,
  WndGuildCombatJoinSelection,
  WndGuildCombatOffer,
  WndGuildCombatRankPerson,
  WndGuildCombatRanking,
  WndGuildCombatResult,
  WndGuildCombatSelection,
  WndGuildCombatState,
  WndGuildWarCancelConfirm,
  WndGuildWarJoinConfirm,
  WndGuildWarState,
};

export interface SiegeWindowDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  readonly getText: (tid: number) => string;
}

/** The `APP_*` of every window of the family (`closeAll`; the join confirm shares the layout of `APP_CLOSE_EXISTING_CONNECTION` with the login screen and is closed by its class). */
export const SIEGE_APPS: readonly number[] = [
  APP_GUILDCOMBAT_1TO1_OFFER,
  APP_GUILDCOMBAT_OFFER,
  APP_GUILDCOMBAT_OFFERSTATE,
  APP_GUILDCOMBAT_SELECTION,
  APP_GUILDCOMBAT_SELSTART,
  APP_GUILDCOMBAT_BOARD,
  APP_GUILDCOMBAT_MSG,
  APP_GUILDCOMBAT_MSG2,
  APP_GUILDCOMBAT_CANCEL_CONFIRM,
  APP_GUILDCOMBAT_RESULT_POINT,
  APP_GUILDCOMBAT_RANK_P,
  APP_GUILDCOMBAT_RANKING,
  APP_GUILD_WAR_STATE,
];

export class SiegeWindowsService {
  readonly #wm: WindowManager;
  readonly #host: SiegeHost;

  constructor(deps: SiegeWindowDeps) {
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

  /** The texts and boxes the shells use (tests). */
  get host(): SiegeHost {
    return this.#host;
  }

  // ---- helpers -----------------------------------------------------------------------------------------------------------

  /** `GetApplet( app )` that is not on its way out. */
  #live(app: number): WndNeuz | null {
    const w = this.#wm.get(app);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `SAFE_DELETE( pWnd )`: the old window of the app goes at once. */
  #replaceApp(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replaceApp(a);
    });
  }

  #get<C extends WndClass>(app: number, lazy: LazyWnd<C>): InstanceType<C> | null {
    const w = this.#wm.get(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  /** The `APP_*` of the siege windows that are open now (the debug probe). */
  openApps(): number[] {
    return [...SIEGE_APPS, APP_CLOSE_EXISTING_CONNECTION].filter((app) =>
      app === APP_CLOSE_EXISTING_CONNECTION ? this.joinConfirm() !== null : this.isOpen(app),
    );
  }

  // ---- the windows ---------------------------------------------------------------------------------------------------------

  /** `SAFE_DELETE( g_WndMng.m_pWndGuildCombatOffer ); new CWndGuildCombat1to1Offer( mode )`. */
  openOffer(request: OfferRequest): WndGuildCombatOffer | null {
    const app = request.classic === true ? APP_GUILDCOMBAT_OFFER : APP_GUILDCOMBAT_1TO1_OFFER;
    return this.#open(
      app,
      SIEGE_LAZY.WndGuildCombatOffer,
      (C) => new C(request, { host: this.#host }),
    );
  }
  offer(classic = false): WndGuildCombatOffer | null {
    return this.#get(
      classic ? APP_GUILDCOMBAT_OFFER : APP_GUILDCOMBAT_1TO1_OFFER,
      SIEGE_LAZY.WndGuildCombatOffer,
    );
  }

  openOfferState(request: StateRequest): WndGuildCombatState | null {
    return this.#open(
      APP_GUILDCOMBAT_OFFERSTATE,
      SIEGE_LAZY.WndGuildCombatState,
      (C) => new C(request, { host: this.#host }),
    );
  }
  offerState(): WndGuildCombatState | null {
    return this.#get(APP_GUILDCOMBAT_OFFERSTATE, SIEGE_LAZY.WndGuildCombatState);
  }

  openSelection(request: SelectionRequest): WndGuildCombatSelection | null {
    return this.#open(
      APP_GUILDCOMBAT_SELECTION,
      SIEGE_LAZY.WndGuildCombatSelection,
      (C) => new C(request, { host: this.#host }),
    );
  }
  selection(): WndGuildCombatSelection | null {
    return this.#get(APP_GUILDCOMBAT_SELECTION, SIEGE_LAZY.WndGuildCombatSelection);
  }

  openJoinSelection(request: SelStartRequest): WndGuildCombatJoinSelection | null {
    return this.#open(
      APP_GUILDCOMBAT_SELSTART,
      SIEGE_LAZY.WndGuildCombatJoinSelection,
      (C) => new C(request, { host: this.#host }),
    );
  }
  joinSelection(): WndGuildCombatJoinSelection | null {
    return this.#get(APP_GUILDCOMBAT_SELSTART, SIEGE_LAZY.WndGuildCombatJoinSelection);
  }

  openBoard(request: BoardRequest): WndGuildCombatBoard | null {
    return this.#open(
      APP_GUILDCOMBAT_BOARD,
      SIEGE_LAZY.WndGuildCombatBoard,
      (C) => new C(request, { host: this.#host }),
    );
  }
  board(): WndGuildCombatBoard | null {
    return this.#get(APP_GUILDCOMBAT_BOARD, SIEGE_LAZY.WndGuildCombatBoard);
  }

  openInfoBox(request: InfoBoxRequest): WndGuildCombatInfoBox | null {
    return this.#open(
      APP_GUILDCOMBAT_MSG,
      SIEGE_LAZY.WndGuildCombatInfoBox,
      (C) => new C(request, { host: this.#host }),
    );
  }
  infoBox(): WndGuildCombatInfoBox | null {
    return this.#get(APP_GUILDCOMBAT_MSG, SIEGE_LAZY.WndGuildCombatInfoBox);
  }

  openInfoBox2(request: InfoBox2Request): WndGuildCombatInfoBox2 | null {
    return this.#open(
      APP_GUILDCOMBAT_MSG2,
      SIEGE_LAZY.WndGuildCombatInfoBox2,
      (C) => new C(request),
    );
  }
  infoBox2(): WndGuildCombatInfoBox2 | null {
    return this.#get(APP_GUILDCOMBAT_MSG2, SIEGE_LAZY.WndGuildCombatInfoBox2);
  }

  openCancelConfirm(request: CancelRequest): WndGuildWarCancelConfirm | null {
    return this.#open(
      APP_GUILDCOMBAT_CANCEL_CONFIRM,
      SIEGE_LAZY.WndGuildWarCancelConfirm,
      (C) => new C(request, { host: this.#host }),
    );
  }
  cancelConfirm(): WndGuildWarCancelConfirm | null {
    return this.#get(APP_GUILDCOMBAT_CANCEL_CONFIRM, SIEGE_LAZY.WndGuildWarCancelConfirm);
  }

  openJoinConfirm(request: JoinRequest): WndGuildWarJoinConfirm | null {
    return this.#open(
      APP_CLOSE_EXISTING_CONNECTION,
      SIEGE_LAZY.WndGuildWarJoinConfirm,
      (C) => new C(request, { host: this.#host }),
    );
  }
  joinConfirm(): WndGuildWarJoinConfirm | null {
    return this.#get(APP_CLOSE_EXISTING_CONNECTION, SIEGE_LAZY.WndGuildWarJoinConfirm);
  }

  openResult(request: ResultRequest): WndGuildCombatResult | null {
    return this.#open(
      APP_GUILDCOMBAT_RESULT_POINT,
      SIEGE_LAZY.WndGuildCombatResult,
      (C) => new C(request, { host: this.#host }),
    );
  }
  result(): WndGuildCombatResult | null {
    return this.#get(APP_GUILDCOMBAT_RESULT_POINT, SIEGE_LAZY.WndGuildCombatResult);
  }

  openRankPerson(request: RankPersonRequest): WndGuildCombatRankPerson | null {
    return this.#open(
      APP_GUILDCOMBAT_RANK_P,
      SIEGE_LAZY.WndGuildCombatRankPerson,
      (C) => new C(request, { getText: (tid) => this.#host.text(tid) }),
    );
  }
  rankPerson(): WndGuildCombatRankPerson | null {
    return this.#get(APP_GUILDCOMBAT_RANK_P, SIEGE_LAZY.WndGuildCombatRankPerson);
  }

  openRanking(request: RankingRequest): WndGuildCombatRanking | null {
    return this.#open(
      APP_GUILDCOMBAT_RANKING,
      SIEGE_LAZY.WndGuildCombatRanking,
      (C) => new C(request),
    );
  }
  ranking(): WndGuildCombatRanking | null {
    return this.#get(APP_GUILDCOMBAT_RANKING, SIEGE_LAZY.WndGuildCombatRanking);
  }

  openWarState(request: WarStateRequest): WndGuildWarState | null {
    return this.#open(APP_GUILD_WAR_STATE, SIEGE_LAZY.WndGuildWarState, (C) => new C(request));
  }
  warState(): WndGuildWarState | null {
    return this.#get(APP_GUILD_WAR_STATE, SIEGE_LAZY.WndGuildWarState);
  }

  /** Every siege window goes (the character left the world). */
  closeAll(): void {
    for (const app of SIEGE_APPS) this.#wm.get(app)?.destroy();
    this.joinConfirm()?.destroy();
    this.#wm.flushDestroyed();
  }
}
