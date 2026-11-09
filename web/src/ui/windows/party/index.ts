// `PartyWindowsService` (`ui.party`, Plan/42 §19.4 H / C / S; WP-UI-39, WP-UI-40): the party windows.
//
//   ui.party.setPorts({ party, find })          45 WP-CFEAT-34, -35: the view and the actions of the party window, the quick window and the finder
//   (applets)   APP_PARTY (hotkey P), APP_PARTY_QUICK, APP_PARTY_FIND (hotkey U, `features.party_find`) open through the applet registry
//   ui.party.openConfirm(request)               APP_PARTY_CONFIRM: the invitation (a second one is the caller's `SendBlock( 7 )`)
//   ui.party.openLeaveConfirm(request)          APP_PARTYLEAVE_CONFIRM: "Are you going to leave the party?" (opens once)
//   ui.party.openChangeTroup(request)           APP_CHANGETROUP: "Convert to an Advanced Party?"
//   ui.party.openChangeName(request)            APP_PARTYCHANGENAME: the name of the advanced party
//   ui.party.openSummon / openSummonUse / openSummonMsg      the party-summon windows
//
// The service registers the three applets so that the taskbar, the start menu and the hotkeys open them; the factories read the ports at the
// moment a window is created, so ports installed later reach windows opened later. It also tells the applet table when the quick window may be
// created together with the party window (two or more members).

import {
  APP_CHANGETROUP,
  APP_MESSAGEBOX_SUMMONPARTY,
  APP_PARTY,
  APP_PARTY_CONFIRM,
  APP_PARTY_FIND,
  APP_PARTY_QUICK,
  APP_PARTYCHANGENAME,
  APP_PARTYLEAVE_CONFIRM,
  APP_SUMMON_PARTY,
  APP_SUMMON_PARTY_USE,
} from '../../../gen/defs/resdata';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import {
  constantView,
  EMPTY_PARTY_VIEW,
  type FinderActions,
  type FinderPorts,
  type FinderView,
  type PartyActions,
  type PartyChangeNameRequest,
  type PartyChangeTroupRequest,
  type PartyConfirmRequest,
  type PartyLeaveRequest,
  type PartyPorts,
  type SummonPartyMsgRequest,
  type SummonPartyRequest,
  type SummonPartyUseRequest,
} from './ports';
import { PARTY_LAZY } from './lazy';
import { WndParty } from './WndParty';
import type { WndPartyChangeName } from './WndPartyChangeName';
import type { WndPartyChangeTroup } from './WndPartyChangeTroup';
import type { WndPartyConfirm } from './WndPartyConfirm';
import { WndPartyFind } from './WndPartyFind';
import type { WndPartyLeaveConfirm } from './WndPartyLeaveConfirm';
import { WndPartyQuick } from './WndPartyQuick';
import type { WndSummonParty, WndSummonPartyMsg, WndSummonPartyUse } from './WndSummonParty';

export * from './ports';
export { WndParty } from './WndParty';
export { PartyInfoPage } from './PartyInfoPage';
export { PartySkillPage } from './PartySkillPage';
export { WndPartyQuick } from './WndPartyQuick';
export type { WndPartyConfirm } from './WndPartyConfirm';
export type { WndPartyLeaveConfirm } from './WndPartyLeaveConfirm';
export type { WndPartyChangeTroup } from './WndPartyChangeTroup';
export type { WndPartyChangeName } from './WndPartyChangeName';
export { WndPartyFind } from './WndPartyFind';
export { FinderList } from './FinderList';
export type { WndSummonParty, WndSummonPartyMsg, WndSummonPartyUse } from './WndSummonParty';
export { RectLayer } from './RectLayer';
export * from './partyGeometry';

export interface PartyWindowDeps {
  readonly wm: WindowManager;
  readonly getText: (tid: number) => string;
}

const NO_ACTIONS: PartyActions = {
  focusMember: () => undefined,
  leave: () => undefined,
  transferLeader: () => undefined,
  openChange: () => undefined,
  setItemShare: () => undefined,
  setExpShare: () => undefined,
  setAllowJoin: () => undefined,
  toggleQuick: () => undefined,
  toggleParty: () => undefined,
  useSkill: () => undefined,
  skillTooltip: () => undefined,
};
const NO_FINDER_ACTIONS: FinderActions = { refresh: () => undefined, join: () => undefined };
const EMPTY_FINDER: FinderView = { rows: [], flagIcons: [] };

/** The ports before 45 installs the real ones: no party, actions that do nothing. */
export const NULL_PARTY_PORTS: { party: PartyPorts; find: FinderPorts } = {
  party: { view: constantView(EMPTY_PARTY_VIEW), actions: NO_ACTIONS },
  find: { view: constantView(EMPTY_FINDER), actions: NO_FINDER_ACTIONS },
};

export class PartyWindowsService {
  readonly #wm: WindowManager;
  readonly #getText: (tid: number) => string;
  #ports = NULL_PARTY_PORTS;

  constructor(deps: PartyWindowDeps) {
    this.#wm = deps.wm;
    this.#getText = deps.getText;
    const applets = deps.wm.applets;
    applets.register({ app: APP_PARTY, factory: () => this.createParty() });
    applets.register({ app: APP_PARTY_QUICK, factory: () => this.createQuick() });
    applets.register({ app: APP_PARTY_FIND, factory: () => this.createFind() });
    // `APP_PARTY` toggles the quick window with it, created only with two or more members (the rule of the applet table)
    deps.wm.appletHooks = {
      ...deps.wm.appletHooks,
      partyHasQuick: () => this.#ports.party.view.get().members.length >= 2,
    };
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<{ party: PartyPorts; find: FinderPorts }>): void {
    this.#ports = { ...this.#ports, ...ports };
  }

  get ports(): { party: PartyPorts; find: FinderPorts } {
    return this.#ports;
  }

  // ---- the applets -----------------------------------------------------------------------------------------------------

  createParty(): WndParty {
    const p = this.#ports.party;
    return new WndParty({ view: p.view, actions: p.actions, getText: this.#getText });
  }

  createQuick(): WndPartyQuick {
    const p = this.#ports.party;
    return new WndPartyQuick({ view: p.view, actions: p.actions });
  }

  createFind(): WndPartyFind {
    const p = this.#ports.find;
    return new WndPartyFind({ view: p.view, actions: p.actions });
  }

  party(): WndParty | null {
    const w = this.#wm.get(APP_PARTY);
    return w instanceof WndParty ? w : null;
  }
  quick(): WndPartyQuick | null {
    const w = this.#wm.get(APP_PARTY_QUICK);
    return w instanceof WndPartyQuick ? w : null;
  }
  find(): WndPartyFind | null {
    const w = this.#wm.get(APP_PARTY_FIND);
    return w instanceof WndPartyFind ? w : null;
  }

  /** `GetApplet( app )` that is not on its way out. */
  #live(app: number): WndNeuz | null {
    const w = this.#wm.get(app);
    return w && !this.#wm.isDestroying(w) ? w : null;
  }

  /** `CWndParty` button 2: opens the simple display with the members, or closes it. */
  toggleQuick(): void {
    if (this.#live(APP_PARTY_QUICK)) {
      this.#wm.get(APP_PARTY_QUICK)?.destroy();
      return;
    }
    this.#wm.flushDestroyed();
    this.#wm.open(APP_PARTY_QUICK);
  }

  /** The quick window's button: opens the detailed window, or closes it. */
  toggleParty(): void {
    if (this.#live(APP_PARTY)) {
      this.#wm.get(APP_PARTY)?.destroy();
      return;
    }
    this.#wm.flushDestroyed();
    this.#wm.open(APP_PARTY);
  }

  // ---- the dialogs --------------------------------------------------------------------------------------------------------

  /** Opens `APP_PARTY_CONFIRM`; `null` when one is open already (the caller answers the newcomer with `SendBlock( 7 )`). */
  openConfirm(request: PartyConfirmRequest): WndPartyConfirm | null {
    if (this.#live(APP_PARTY_CONFIRM)) return null;
    this.#wm.flushDestroyed();
    return openLazy(
      this.#wm,
      APP_PARTY_CONFIRM,
      PARTY_LAZY.WndPartyConfirm,
      (C) => new C(request),
      () => undefined,
    );
  }

  isConfirmOpen(): boolean {
    return this.#live(APP_PARTY_CONFIRM) !== null;
  }

  /** `SAFE_DELETE( m_pWndPartyConfirm )`: the invitation window goes without an answer. */
  closeConfirm(): void {
    this.#wm.get(APP_PARTY_CONFIRM)?.destroy();
  }

  confirm(): WndPartyConfirm | null {
    const w = this.#wm.get(APP_PARTY_CONFIRM);
    const ctor = PARTY_LAZY.WndPartyConfirm.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /** `if( pWndPartyLeaveConfirm == NULL )`: the question opens once. */
  openLeaveConfirm(request: PartyLeaveRequest): WndPartyLeaveConfirm | null {
    if (this.#live(APP_PARTYLEAVE_CONFIRM)) return null;
    this.#wm.flushDestroyed();
    return openLazy(
      this.#wm,
      APP_PARTYLEAVE_CONFIRM,
      PARTY_LAZY.WndPartyLeaveConfirm,
      (C) => new C(request),
      () => undefined,
    );
  }

  leaveConfirm(): WndPartyLeaveConfirm | null {
    const w = this.#wm.get(APP_PARTYLEAVE_CONFIRM);
    const ctor = PARTY_LAZY.WndPartyLeaveConfirm.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /** `SAFE_DELETE( m_WndPartyChangeTroup ); new ...`: a new window replaces the old one. */
  openChangeTroup(request: PartyChangeTroupRequest): WndPartyChangeTroup | null {
    return this.#replace(APP_CHANGETROUP, PARTY_LAZY.WndPartyChangeTroup, (C) => new C(request));
  }

  changeTroup(): WndPartyChangeTroup | null {
    const w = this.#wm.get(APP_CHANGETROUP);
    const ctor = PARTY_LAZY.WndPartyChangeTroup.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  openChangeName(request: PartyChangeNameRequest): WndPartyChangeName | null {
    return this.#replace(APP_PARTYCHANGENAME, PARTY_LAZY.WndPartyChangeName, (C) => new C(request));
  }

  changeName(): WndPartyChangeName | null {
    const w = this.#wm.get(APP_PARTYCHANGENAME);
    const ctor = PARTY_LAZY.WndPartyChangeName.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  openSummon(request: SummonPartyRequest): WndSummonParty | null {
    // `if( !pWndSummonParty )`: the question opens once
    if (this.#live(APP_SUMMON_PARTY)) return null;
    this.#wm.flushDestroyed();
    return openLazy(
      this.#wm,
      APP_SUMMON_PARTY,
      PARTY_LAZY.WndSummonParty,
      (C) => new C(request),
      () => undefined,
    );
  }

  openSummonUse(request: SummonPartyUseRequest): WndSummonPartyUse | null {
    if (this.#live(APP_SUMMON_PARTY_USE)) return null;
    this.#wm.flushDestroyed();
    return openLazy(
      this.#wm,
      APP_SUMMON_PARTY_USE,
      PARTY_LAZY.WndSummonPartyUse,
      (C) => new C(request),
      () => undefined,
    );
  }

  /** The member's notice: a second summon request refreshes the texts of the open window (the original re-ran `SetData`). */
  openSummonMsg(request: SummonPartyMsgRequest): WndSummonPartyMsg | null {
    return this.#replace(
      APP_MESSAGEBOX_SUMMONPARTY,
      PARTY_LAZY.WndSummonPartyMsg,
      (C) => new C(request),
    );
  }

  summon(): WndSummonParty | null {
    const w = this.#wm.get(APP_SUMMON_PARTY);
    const ctor = PARTY_LAZY.WndSummonParty.ctor;
    return ctor && w instanceof ctor ? w : null;
  }
  summonUse(): WndSummonPartyUse | null {
    const w = this.#wm.get(APP_SUMMON_PARTY_USE);
    const ctor = PARTY_LAZY.WndSummonPartyUse.ctor;
    return ctor && w instanceof ctor ? w : null;
  }
  summonMsg(): WndSummonPartyMsg | null {
    const w = this.#wm.get(APP_MESSAGEBOX_SUMMONPARTY);
    const ctor = PARTY_LAZY.WndSummonPartyMsg.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /** Opens the window of a lazy class, replacing the open one (`null` while its chunk is still loading: the window then opens by itself). */
  #replace<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      const old = this.#wm.get(a);
      if (!old) return;
      old.destroy();
      this.#wm.flushDestroyed();
    });
  }
}
