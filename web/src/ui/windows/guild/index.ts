// `GuildWindowsService` (`ui.guild`, Plan/42 §19.4 J; WP-UI-45..48): the guild windows.
//
//   ui.guild.setPorts({ guild })                     45 WP-CFEAT-37: the view, the power draft and the actions of the guild window
//   (applet)  APP_GUILD (hotkey G)                   opens through the applet registry; the five pages are its tab pages
//   ui.guild.openInvite / openName / openNickname / openDismiss / openNotice / openLogo / openPayConfirm      the small dialogs
//   ui.guild.openMerit / openBank / openBankLog      the contribution window, the guild bank and its log
//   ui.guild.openWarDecl / openWarGiveUp / openWarPeace / openWarPeaceConfirm / openWarRequest        the guild-war dialogs
//   ui.guild.closeAll()                              every guild window goes (the player lost the guild)
//
// The factory of the applet reads the ports when a window is created, so ports installed later reach windows opened later. Each `open*` replaces the open window of the
// same kind (the original kept one pointer per window and `SAFE_DELETE`d it first) and answers `null` while the chunk of a dialog is still loading (the window then opens by itself).

import {
  APP_GUILD,
  APP_GUILD_BANK,
  APP_GUILD_BANK_LOG,
  APP_GUILD_DISMISS,
  APP_GUILD_INVATE,
  APP_GUILD_MERIT1,
  APP_GUILD_NICKNAME,
  APP_GUILD_NOTICE,
  APP_GUILD_PAYCONFIRM,
  APP_GUILD_SETLOGO,
  APP_GUILD_WAR,
  APP_GUILD_WARGIVEUP,
  APP_GUILD_WARPEACE,
  APP_GUILD_WARPEACECONFIRM,
  APP_GUILD_WARREQUEST,
  APP_GUILDNAME,
} from '../../../gen/defs/resdata';
import type { UiAtlases } from '../../skin/atlas';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import type { WndNeuz } from '../../wnd/WndNeuz';
import { GUILD_LAZY } from './lazy';
import {
  constantView,
  type DismissRequest,
  EMPTY_GUILD_VIEW,
  EMPTY_POWER_VIEW,
  type GuildActions,
  type GuildBankRequest,
  type GuildPorts,
  type InviteRequest,
  type LogoRequest,
  type MeritRequest,
  type NameRequest,
  type NicknameRequest,
  type NoticeRequest,
  type PayRequest,
  type WarDeclRequest,
  type WarRequestRequest,
  type WarYesNoRequest,
  type BankLogRequest,
} from './ports';
import { WndGuild } from './WndGuild';
import type { WndGuildBank } from './WndGuildBank';
import type { WndGuildBankLog } from './WndGuildBankLog';
import type { WndGuildDismiss } from './WndGuildDismiss';
import type { WndGuildInvite } from './WndGuildInvite';
import type { WndGuildMerit } from './WndGuildMerit';
import type { WndGuildName, WndGuildNickName } from './WndGuildName';
import type { WndGuildNotice } from './WndGuildNotice';
import type { WndGuildPayConfirm } from './WndGuildPayConfirm';
import type { WndGuildSetLogo } from './WndGuildSetLogo';
import type { WndGuildWarDecl } from './WndGuildWarDecl';
import type { WndGuildWarGiveUp } from './WndGuildWarGiveUp';
import type { WndGuildWarPeace } from './WndGuildWarPeace';
import type { WndGuildWarPeaceConfirm } from './WndGuildWarPeaceConfirm';
import type { WndGuildWarRequest } from './WndGuildWarRequest';

export * from './ports';
export * from './logoPath';
export * from './logoGrid';
export * from './memberSort';
export * from './memberIcons';
export { WndGuild, GUILD_TAB } from './WndGuild';
export { GuildInfoPage } from './GuildInfoPage';
export { GuildMemberPage } from './GuildMemberPage';
export { GuildMemberList } from './GuildMemberList';
export { GuildAppPage, APP_BITS, appCheckName } from './GuildAppPage';
export { GuildPowerPage, POWER_CHECKS } from './GuildPowerPage';
export { GuildWarPage, formatLeft, WAR_SECONDS } from './GuildWarPage';
export { BankLogList } from './BankLogList';
export { atoiPenya } from './penya';
export type {
  WndGuildBank,
  WndGuildBankLog,
  WndGuildDismiss,
  WndGuildInvite,
  WndGuildMerit,
  WndGuildName,
  WndGuildNickName,
  WndGuildNotice,
  WndGuildPayConfirm,
  WndGuildSetLogo,
  WndGuildWarDecl,
  WndGuildWarGiveUp,
  WndGuildWarPeace,
  WndGuildWarPeaceConfirm,
  WndGuildWarRequest,
};

export interface GuildWindowDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
  readonly atlases: UiAtlases;
  readonly getText: (tid: number) => string;
}

const NO_ACTIONS: GuildActions = {
  setName: () => undefined,
  setNotice: () => undefined,
  setLogo: () => undefined,
  disband: () => undefined,
  memberMenu: () => [],
  pickMember: () => undefined,
  setPower: () => undefined,
  sendPower: () => undefined,
  openPay: () => undefined,
  declareWar: () => undefined,
  truce: () => undefined,
  surrender: () => undefined,
};

/** The ports before 45 installs the real ones: no guild, actions that do nothing. */
export const NULL_GUILD_PORTS: { guild: GuildPorts } = {
  guild: {
    view: constantView(EMPTY_GUILD_VIEW),
    power: constantView(EMPTY_POWER_VIEW),
    actions: NO_ACTIONS,
    now: () => Math.floor(Date.now() / 1000),
  },
};

/** The `APP_*` of every window of the family (`closeAll`). */
export const GUILD_APPS: readonly number[] = [
  APP_GUILD,
  APP_GUILD_INVATE,
  APP_GUILDNAME,
  APP_GUILD_NICKNAME,
  APP_GUILD_DISMISS,
  APP_GUILD_NOTICE,
  APP_GUILD_SETLOGO,
  APP_GUILD_PAYCONFIRM,
  APP_GUILD_MERIT1,
  APP_GUILD_BANK,
  APP_GUILD_BANK_LOG,
  APP_GUILD_WAR,
  APP_GUILD_WARGIVEUP,
  APP_GUILD_WARPEACE,
  APP_GUILD_WARPEACECONFIRM,
  APP_GUILD_WARREQUEST,
];

export class GuildWindowsService {
  readonly #wm: WindowManager;
  readonly #deps: GuildWindowDeps;
  #ports = NULL_GUILD_PORTS;

  constructor(deps: GuildWindowDeps) {
    this.#wm = deps.wm;
    this.#deps = deps;
    deps.wm.applets.register({ app: APP_GUILD, factory: () => this.createGuild() });
  }

  /** Installs the ports of 45 (the rest keeps its previous value). */
  setPorts(ports: Partial<{ guild: GuildPorts }>): void {
    this.#ports = { ...this.#ports, ...ports };
  }

  get ports(): { guild: GuildPorts } {
    return this.#ports;
  }

  // ---- the applet --------------------------------------------------------------------------------------------------------------

  createGuild(): WndGuild {
    return new WndGuild({
      ports: this.#ports.guild,
      tooltip: this.#deps.tooltip,
      atlases: this.#deps.atlases,
      getText: this.#deps.getText,
    });
  }

  guild(): WndGuild | null {
    const w = this.#wm.get(APP_GUILD);
    return w instanceof WndGuild ? w : null;
  }

  // ---- helpers -------------------------------------------------------------------------------------------------------------------

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

  /** Opens the window of a lazy class, replacing the open one (`null` while its chunk is still loading: the window then opens by itself). */
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

  // ---- the dialogs -----------------------------------------------------------------------------------------------------------------

  /** `SAFE_DELETE( g_WndMng.m_pWndGuildConfirm ); new ...`: a new invitation replaces the old one. */
  openInvite(request: InviteRequest): WndGuildInvite | null {
    return this.#open(APP_GUILD_INVATE, GUILD_LAZY.WndGuildInvite, (C) => new C(request));
  }
  invite(): WndGuildInvite | null {
    return this.#get(APP_GUILD_INVATE, GUILD_LAZY.WndGuildInvite);
  }
  closeInvite(): void {
    this.#wm.get(APP_GUILD_INVATE)?.destroy();
  }

  openName(request: NameRequest): WndGuildName | null {
    return this.#open(APP_GUILDNAME, GUILD_LAZY.WndGuildName, (C) => new C(request));
  }
  name(): WndGuildName | null {
    return this.#get(APP_GUILDNAME, GUILD_LAZY.WndGuildName);
  }

  openNickname(request: NicknameRequest): WndGuildNickName | null {
    return this.#open(APP_GUILD_NICKNAME, GUILD_LAZY.WndGuildNickName, (C) => new C(request));
  }
  nickname(): WndGuildNickName | null {
    return this.#get(APP_GUILD_NICKNAME, GUILD_LAZY.WndGuildNickName);
  }

  openDismiss(request: DismissRequest): WndGuildDismiss | null {
    return this.#open(APP_GUILD_DISMISS, GUILD_LAZY.WndGuildDismiss, (C) => new C(request));
  }
  dismiss(): WndGuildDismiss | null {
    return this.#get(APP_GUILD_DISMISS, GUILD_LAZY.WndGuildDismiss);
  }

  openNotice(request: NoticeRequest): WndGuildNotice | null {
    return this.#open(APP_GUILD_NOTICE, GUILD_LAZY.WndGuildNotice, (C) => new C(request));
  }
  notice(): WndGuildNotice | null {
    return this.#get(APP_GUILD_NOTICE, GUILD_LAZY.WndGuildNotice);
  }

  openLogo(request: LogoRequest): WndGuildSetLogo | null {
    return this.#open(APP_GUILD_SETLOGO, GUILD_LAZY.WndGuildSetLogo, (C) => new C(request));
  }
  logo(): WndGuildSetLogo | null {
    return this.#get(APP_GUILD_SETLOGO, GUILD_LAZY.WndGuildSetLogo);
  }

  openPayConfirm(request: PayRequest): WndGuildPayConfirm | null {
    return this.#open(APP_GUILD_PAYCONFIRM, GUILD_LAZY.WndGuildPayConfirm, (C) => new C(request));
  }
  payConfirm(): WndGuildPayConfirm | null {
    return this.#get(APP_GUILD_PAYCONFIRM, GUILD_LAZY.WndGuildPayConfirm);
  }

  // ---- the contribution window, the bank and its log ---------------------------------------------------------------------

  openMerit(request: MeritRequest): WndGuildMerit | null {
    return this.#open(
      APP_GUILD_MERIT1,
      GUILD_LAZY.WndGuildMerit,
      (C) => new C(request, { tooltip: this.#deps.tooltip }),
    );
  }
  merit(): WndGuildMerit | null {
    return this.#get(APP_GUILD_MERIT1, GUILD_LAZY.WndGuildMerit);
  }

  /** `SAFE_DELETE( g_WndMng.m_pWndGuildBank ); new CWndGuildBank` (the inventory exists: `CreateApplet( APP_INVENTORY )` ran first). */
  openBank(request: GuildBankRequest): WndGuildBank | null {
    return this.#open(
      APP_GUILD_BANK,
      GUILD_LAZY.WndGuildBank,
      (C) =>
        new C({
          view: request.view,
          actions: request.actions,
          tooltip: this.#deps.tooltip,
          getText: this.#deps.getText,
        }),
    );
  }
  bank(): WndGuildBank | null {
    return this.#get(APP_GUILD_BANK, GUILD_LAZY.WndGuildBank);
  }
  closeBank(): void {
    this.#wm.get(APP_GUILD_BANK)?.destroy();
  }

  openBankLog(request: BankLogRequest): WndGuildBankLog | null {
    return this.#open(
      APP_GUILD_BANK_LOG,
      GUILD_LAZY.WndGuildBankLog,
      (C) => new C(request, { getText: this.#deps.getText }),
    );
  }
  bankLog(): WndGuildBankLog | null {
    return this.#get(APP_GUILD_BANK_LOG, GUILD_LAZY.WndGuildBankLog);
  }
  closeBankLog(): void {
    this.#wm.get(APP_GUILD_BANK_LOG)?.destroy();
  }

  // ---- the guild war -------------------------------------------------------------------------------------------------------------------

  openWarDecl(request: WarDeclRequest): WndGuildWarDecl | null {
    return this.#open(APP_GUILD_WAR, GUILD_LAZY.WndGuildWarDecl, (C) => new C(request));
  }
  warDecl(): WndGuildWarDecl | null {
    return this.#get(APP_GUILD_WAR, GUILD_LAZY.WndGuildWarDecl);
  }

  openWarGiveUp(request: WarYesNoRequest): WndGuildWarGiveUp | null {
    return this.#open(APP_GUILD_WARGIVEUP, GUILD_LAZY.WndGuildWarGiveUp, (C) => new C(request));
  }
  warGiveUp(): WndGuildWarGiveUp | null {
    return this.#get(APP_GUILD_WARGIVEUP, GUILD_LAZY.WndGuildWarGiveUp);
  }

  openWarPeace(request: WarYesNoRequest): WndGuildWarPeace | null {
    return this.#open(APP_GUILD_WARPEACE, GUILD_LAZY.WndGuildWarPeace, (C) => new C(request));
  }
  warPeace(): WndGuildWarPeace | null {
    return this.#get(APP_GUILD_WARPEACE, GUILD_LAZY.WndGuildWarPeace);
  }

  openWarPeaceConfirm(request: WarYesNoRequest): WndGuildWarPeaceConfirm | null {
    return this.#open(
      APP_GUILD_WARPEACECONFIRM,
      GUILD_LAZY.WndGuildWarPeaceConfirm,
      (C) => new C(request),
    );
  }
  warPeaceConfirm(): WndGuildWarPeaceConfirm | null {
    return this.#get(APP_GUILD_WARPEACECONFIRM, GUILD_LAZY.WndGuildWarPeaceConfirm);
  }

  openWarRequest(request: WarRequestRequest): WndGuildWarRequest | null {
    return this.#open(APP_GUILD_WARREQUEST, GUILD_LAZY.WndGuildWarRequest, (C) => new C(request));
  }
  warRequest(): WndGuildWarRequest | null {
    return this.#get(APP_GUILD_WARREQUEST, GUILD_LAZY.WndGuildWarRequest);
  }

  /** Every guild window goes (the player lost the guild: dissolved, left, removed). */
  closeAll(): void {
    for (const app of GUILD_APPS) this.#wm.get(app)?.destroy();
    this.#wm.flushDestroyed();
  }
}
