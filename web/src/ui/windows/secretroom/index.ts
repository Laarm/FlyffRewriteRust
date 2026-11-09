// `SecretRoomWindowsService` (`ui.secretRoom`, Plan/42 §19.4 K; WP-UI-56, WP-UI-57): the secret-room, tax and 1:1 line-up windows.
//
//   ui.secretRoom.openOffer( request )               700: the bid window            ui.secretRoom.openOfferState( request )   704: the bidding guilds, the countdown, the own rank and bid
//   ui.secretRoom.openSelection( request )           702: the line-up editor        ui.secretRoom.openGc11Selection( request ) 672: the same class for the 1:1 combat (the `1TO1` texts)
//   ui.secretRoom.openBoard( request )               747: the information board     ui.secretRoom.openInfoBox( request )      710: the entrance question (Yes / No)
//   ui.secretRoom.openCancelConfirm( request )       760                            ui.secretRoom.openQuick( request )        754: the member list of the war (`quick()`)
//   ui.secretRoom.openChangeTaxRate / openCheckTaxRate   715 / 716                  ui.secretRoom.openGuildMemMng / openGuildMember   751 / 752: shells that V15 never opens
//   ui.secretRoom.isOpen( app ) / openApps() / close( app ) / closeAll()
//
// Each `open*` replaces the open window of the same kind (the original: `SAFE_DELETE( m_pWndX ); new ...`) and answers `null` while the chunk of the window is still loading (the window
// then opens by itself). A getter answers `null` once its window is destroyed.

import {
  APP_GUILDCOMBAT_1TO1_SELECTION,
  APP_SECRETROOM_BOARD,
  APP_SECRETROOM_CANCEL_CONFIRM,
  APP_SECRETROOM_GUILDMEM_MNG,
  APP_SECRETROOM_GUILDMEMBER,
  APP_SECRETROOM_MSG,
  APP_SECRETROOM_OFFER,
  APP_SECRETROOM_OFFERSTATE,
  APP_SECRETROOM_QUICK,
  APP_SECRETROOM_SELECTION,
  APP_SECRETROOM_TAXRATE_CHANGE,
  APP_SECRETROOM_TAXRATE_CHECK,
} from '../../../gen/defs/resdata';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import { MB, MBID } from '../../msgbox/types';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { fmt } from '../siege/format';
import { MIN_LINEUP_LEVEL } from '../siege/ports';
import { SECRETROOM_LAZY } from './lazy';
import type {
  BoardRequest,
  CancelRequest,
  ChangeTaxRequest,
  CheckTaxRequest,
  Gc11SelectionRequest,
  GuildMemberRequest,
  GuildMemMngRequest,
  InfoBoxRequest,
  OfferRequest,
  OfferStateRequest,
  QuickRequest,
  SecretRoomHost,
  SecretRoomSelectionRequest,
} from './ports';
import type { WndLineUpSelection } from './WndLineUpSelection';
import type { WndSecretRoomBoard } from './WndSecretRoomBoard';
import type { WndSecretRoomCancelConfirm } from './WndSecretRoomCancelConfirm';
import type { WndSecretRoomChangeTaxRate } from './WndSecretRoomChangeTaxRate';
import type { WndSecretRoomCheckTaxRate } from './WndSecretRoomCheckTaxRate';
import type { WndSecretRoomGuildMember } from './WndSecretRoomGuildMember';
import type { WndSecretRoomGuildMemMng } from './WndSecretRoomGuildMemMng';
import type { WndSecretRoomInfoBox } from './WndSecretRoomInfoBox';
import type { WndSecretRoomOffer } from './WndSecretRoomOffer';
import type { WndSecretRoomOfferState } from './WndSecretRoomOfferState';
import type { WndSecretRoomQuick } from './WndSecretRoomQuick';

export * from './ports';
export * from './rows';
export type {
  WndLineUpSelection,
  WndSecretRoomBoard,
  WndSecretRoomCancelConfirm,
  WndSecretRoomChangeTaxRate,
  WndSecretRoomCheckTaxRate,
  WndSecretRoomGuildMember,
  WndSecretRoomGuildMemMng,
  WndSecretRoomInfoBox,
  WndSecretRoomOffer,
  WndSecretRoomOfferState,
  WndSecretRoomQuick,
};

export interface SecretRoomWindowDeps {
  readonly wm: WindowManager;
  readonly msgbox: MessageBoxApi;
  readonly getText: (tid: number) => string;
}

/** The `APP_*` of every window of the family (`closeAll`, the probe). */
export const SECRETROOM_APPS: readonly number[] = [
  APP_SECRETROOM_OFFER,
  APP_SECRETROOM_OFFERSTATE,
  APP_SECRETROOM_SELECTION,
  APP_GUILDCOMBAT_1TO1_SELECTION,
  APP_SECRETROOM_BOARD,
  APP_SECRETROOM_MSG,
  APP_SECRETROOM_QUICK,
  APP_SECRETROOM_CANCEL_CONFIRM,
  APP_SECRETROOM_TAXRATE_CHANGE,
  APP_SECRETROOM_TAXRATE_CHECK,
  APP_SECRETROOM_GUILDMEM_MNG,
  APP_SECRETROOM_GUILDMEMBER,
];

export class SecretRoomWindowsService {
  readonly #wm: WindowManager;
  readonly #host: SecretRoomHost;

  constructor(deps: SecretRoomWindowDeps) {
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
  get host(): SecretRoomHost {
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
    const w = this.#live(app);
    const ctor = lazy.ctor;
    return ctor && w instanceof ctor ? (w as InstanceType<C>) : null;
  }

  isOpen(app: number): boolean {
    return this.#live(app) !== null;
  }

  /** The `APP_*` of the secret-room windows that are open now (the debug probe). */
  openApps(): number[] {
    return SECRETROOM_APPS.filter((app) => this.isOpen(app));
  }

  /** Closes the window of one app (a no-op when it is not open). */
  close(app: number): void {
    this.#live(app)?.destroy();
  }

  // ---- the windows ---------------------------------------------------------------------------------------------------------

  /** `SAFE_DELETE( m_pWndSecretRoomOffer ); new CWndSecretRoomOffer`. */
  openOffer(request: OfferRequest): WndSecretRoomOffer | null {
    return this.#open(
      APP_SECRETROOM_OFFER,
      SECRETROOM_LAZY.WndSecretRoomOffer,
      (C) => new C(request, { host: this.#host }),
    );
  }
  offer(): WndSecretRoomOffer | null {
    return this.#get(APP_SECRETROOM_OFFER, SECRETROOM_LAZY.WndSecretRoomOffer);
  }

  openOfferState(request: OfferStateRequest): WndSecretRoomOfferState | null {
    return this.#open(
      APP_SECRETROOM_OFFERSTATE,
      SECRETROOM_LAZY.WndSecretRoomOfferState,
      (C) => new C(request, { host: this.#host }),
    );
  }
  offerState(): WndSecretRoomOfferState | null {
    return this.#get(APP_SECRETROOM_OFFERSTATE, SECRETROOM_LAZY.WndSecretRoomOfferState);
  }

  /** The line-up editor of the secret room (702, the `TID_GAME_SECRETROOM_*` texts; the level limit is 30). */
  openSelection(request: SecretRoomSelectionRequest): WndLineUpSelection | null {
    return this.#open(
      APP_SECRETROOM_SELECTION,
      SECRETROOM_LAZY.WndLineUpSelection,
      (C) =>
        new C(
          {
            ...request,
            kind: 'secretRoom',
            minLevel: MIN_LINEUP_LEVEL,
            isKingpin: () => false,
            canEdit: () => true,
          },
          { host: this.#host },
        ),
    );
  }
  selection(): WndLineUpSelection | null {
    return this.#get(APP_SECRETROOM_SELECTION, SECRETROOM_LAZY.WndLineUpSelection);
  }

  /** The line-up editor of the 1:1 guild combat (672, the `TID_GAME_GUILDCOMBAT1TO1_*` texts): the second kind of the same class (ui-minigames, WP-CFEAT-45). */
  openGc11Selection(request: Gc11SelectionRequest): WndLineUpSelection | null {
    return this.#open(
      APP_GUILDCOMBAT_1TO1_SELECTION,
      SECRETROOM_LAZY.WndLineUpSelection,
      (C) => new C({ ...request, kind: 'gc1to1' }, { host: this.#host }),
    );
  }
  gc11Selection(): WndLineUpSelection | null {
    return this.#get(APP_GUILDCOMBAT_1TO1_SELECTION, SECRETROOM_LAZY.WndLineUpSelection);
  }

  openBoard(request: BoardRequest): WndSecretRoomBoard | null {
    return this.#open(
      APP_SECRETROOM_BOARD,
      SECRETROOM_LAZY.WndSecretRoomBoard,
      (C) => new C(request),
    );
  }
  board(): WndSecretRoomBoard | null {
    return this.#get(APP_SECRETROOM_BOARD, SECRETROOM_LAZY.WndSecretRoomBoard);
  }

  openInfoBox(request: InfoBoxRequest): WndSecretRoomInfoBox | null {
    return this.#open(
      APP_SECRETROOM_MSG,
      SECRETROOM_LAZY.WndSecretRoomInfoBox,
      (C) => new C(request),
    );
  }
  infoBox(): WndSecretRoomInfoBox | null {
    return this.#get(APP_SECRETROOM_MSG, SECRETROOM_LAZY.WndSecretRoomInfoBox);
  }

  /** The member list of the war (754). */
  openQuick(request: QuickRequest): WndSecretRoomQuick | null {
    return this.#open(
      APP_SECRETROOM_QUICK,
      SECRETROOM_LAZY.WndSecretRoomQuick,
      (C) => new C(request),
    );
  }
  quick(): WndSecretRoomQuick | null {
    return this.#get(APP_SECRETROOM_QUICK, SECRETROOM_LAZY.WndSecretRoomQuick);
  }

  openCancelConfirm(request: CancelRequest): WndSecretRoomCancelConfirm | null {
    return this.#open(
      APP_SECRETROOM_CANCEL_CONFIRM,
      SECRETROOM_LAZY.WndSecretRoomCancelConfirm,
      (C) => new C(request, { host: this.#host }),
    );
  }
  cancelConfirm(): WndSecretRoomCancelConfirm | null {
    return this.#get(APP_SECRETROOM_CANCEL_CONFIRM, SECRETROOM_LAZY.WndSecretRoomCancelConfirm);
  }

  openChangeTaxRate(request: ChangeTaxRequest): WndSecretRoomChangeTaxRate | null {
    return this.#open(
      APP_SECRETROOM_TAXRATE_CHANGE,
      SECRETROOM_LAZY.WndSecretRoomChangeTaxRate,
      (C) => new C(request, { host: this.#host }),
    );
  }
  changeTaxRate(): WndSecretRoomChangeTaxRate | null {
    return this.#get(APP_SECRETROOM_TAXRATE_CHANGE, SECRETROOM_LAZY.WndSecretRoomChangeTaxRate);
  }

  openCheckTaxRate(request: CheckTaxRequest): WndSecretRoomCheckTaxRate | null {
    return this.#open(
      APP_SECRETROOM_TAXRATE_CHECK,
      SECRETROOM_LAZY.WndSecretRoomCheckTaxRate,
      (C) => new C(request, { host: this.#host }),
    );
  }
  checkTaxRate(): WndSecretRoomCheckTaxRate | null {
    return this.#get(APP_SECRETROOM_TAXRATE_CHECK, SECRETROOM_LAZY.WndSecretRoomCheckTaxRate);
  }

  /** The shell of 751 (V15 never opens it). */
  openGuildMemMng(request: GuildMemMngRequest): WndSecretRoomGuildMemMng | null {
    return this.#open(
      APP_SECRETROOM_GUILDMEM_MNG,
      SECRETROOM_LAZY.WndSecretRoomGuildMemMng,
      (C) => new C(request),
    );
  }
  guildMemMng(): WndSecretRoomGuildMemMng | null {
    return this.#get(APP_SECRETROOM_GUILDMEM_MNG, SECRETROOM_LAZY.WndSecretRoomGuildMemMng);
  }

  /** The shell of 752 (V15 never opens it). */
  openGuildMember(request: GuildMemberRequest): WndSecretRoomGuildMember | null {
    return this.#open(
      APP_SECRETROOM_GUILDMEMBER,
      SECRETROOM_LAZY.WndSecretRoomGuildMember,
      (C) => new C(request),
    );
  }
  guildMember(): WndSecretRoomGuildMember | null {
    return this.#get(APP_SECRETROOM_GUILDMEMBER, SECRETROOM_LAZY.WndSecretRoomGuildMember);
  }

  /** Every secret-room window goes (the character left the world). */
  closeAll(): void {
    for (const app of SECRETROOM_APPS) this.#live(app)?.destroy();
    this.#wm.flushDestroyed();
  }
}
