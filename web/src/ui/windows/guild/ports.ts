// Ports of the guild windows (Plan/42 §19.4 J, WP-UI-45..48; wired by the `guild` feature of 45, WP-CFEAT-37, -38): the guild window with its five pages, the
// small dialogs (invitation, name, nickname, disband, notice, logo, salary, contribution) and the bank, the bank log and the guild-war dialogs. The READ MODELS
// (`*View`, observed through a `ViewSource`) and the ACTIONS the shells call, plain TS without protobuf types. Every action names the legacy send behind it
// (ref/99 App. B); the decisions (the pre-checks of the original, the boxes, the sends) are the feature's. The shells only draw and report.

import type { Shortcut } from '../../dnd/shortcut';
import type { InventoryItem } from '../char/ports';
import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** `GUD_MASTER` .. `GUD_ROOKIE` (`guild.h`): the member levels, 0 is the best. */
export const GUD = { MASTER: 0, KINGPIN: 1, CAPTAIN: 2, SUPPORTER: 3, ROOKIE: 4 } as const;
/** `MAX_GM_LEVEL`: the number of member levels (and of power words). */
export const GUILD_RANKS = 5;
/** `PF_*` (`guild.h`): the bits of a power word. */
export const PF = {
  MEMBERLEVEL: 0x01,
  LEVEL: 0x02,
  INVITATION: 0x04,
  PENYA: 0x08,
  ITEM: 0x10,
  GH_FURNITURE: 0x20,
  GH_UPKEEP: 0x40,
} as const;

/** The guild bank has 42 slots (`MAX_GUILDBANK_SLOT`). */
export const GUILD_BANK_SLOTS = 42;
/** The four log lists of the bank log window (`GI_LOG_VIEW_*`): 1 item added, 2 item removed, 3 penya added, 4 penya removed. */
export type BankLogKind = 1 | 2 | 3 | 4;

// ---- the guild window ---------------------------------------------------------------------------------------------------------------------

/** One member as the member page draws it (`MEMBERLIST`): the facts of the guild record plus the name, level and job of the player-data cache. */
export interface GuildMemberRow {
  /** `m_idPlayer`. */
  readonly id: number;
  /** The name from the cache (`''` until it is known). */
  readonly name: string;
  /** `m_szAlias`. */
  readonly alias: string;
  /** `m_nMemberLv`, 0 master .. 4 rookie. */
  readonly rank: number;
  /** `m_nClass`, 0..2 (rank stars). */
  readonly cls: number;
  readonly level: number;
  /** The job id (`JOB_*`). */
  readonly job: number;
  /** `prj.m_aJob[job].dwJobType` (the comparators group by it). */
  readonly jobType: number;
  /** `uLogin > 0`, or the presence overlay of `GuildGameJoin` / `GuildGameLogin`. */
  readonly online: boolean;
  /** `m_dwGivePxpCount`. */
  readonly givePxp: number;
  /** `m_nGiveGold` (may be negative after withdrawals). */
  readonly giveGold: number;
  /** `m_nSurrender`. */
  readonly surrender: number;
}

/** The war tab (`CWndGuildTabWar::UpdateData` / `OnDraw`). */
export interface GuildWarView {
  /** The enemy guild name (`''` without a war). */
  readonly enemyName: string;
  /** The logo of the enemy guild, 0 = none. */
  readonly enemyLogo: number;
  /** `m_nWin`, `m_nLose`, `m_nSurrender` of the OWN guild. */
  readonly win: number;
  readonly lose: number;
  readonly surrender: number;
  /** The three lines of the tab: `TID_GAME_GUILD_WAR_DATA_WIN`, `_LOSE`, `_SURRENDER` with the counters filled in. */
  readonly winText: string;
  readonly loseText: string;
  readonly surrenderText: string;
  /** Unix seconds the running war started; `null` without a war (the time left is empty). */
  readonly startAt: number | null;
}

export interface GuildView {
  /** Bumps with every change of the guild store (the pages rebuild on it: `UpdateDataAll`). */
  readonly version: number;
  readonly hasGuild: boolean;
  /** `m_szGuild` (`''` while unnamed). */
  readonly name: string;
  readonly level: number;
  /** The master's name from the cache. */
  readonly masterName: string;
  readonly memberCount: number;
  /** `GetMaxMemeber( level )`. */
  readonly maxMembers: number;
  /** `m_dwContributionPxp`. */
  readonly pxp: number;
  /** `m_nGoldGuild`. */
  readonly penya: number;
  readonly notice: string;
  /** `m_dwLogo`, 0 = none. */
  readonly logo: number;
  /** The own character is the guild master. */
  readonly isMaster: boolean;
  /** `m_adwPenya[5]`: the salary of each member level. */
  readonly salaries: readonly number[];
  /** The "%.2d/%.2d" counts of the appellation page (member level / cap), one per rank. */
  readonly rankCounts: readonly string[];
  /** The members in character id order (`m_mapPMember`). */
  readonly members: readonly GuildMemberRow[];
  /** `g_pPlayer->m_idWar != 0`. */
  readonly selfWar: boolean;
  readonly war: GuildWarView;
}

export const EMPTY_GUILD_WAR: GuildWarView = {
  enemyName: '',
  enemyLogo: 0,
  win: 0,
  lose: 0,
  surrender: 0,
  winText: '',
  loseText: '',
  surrenderText: '',
  startAt: null,
};

export const EMPTY_GUILD_VIEW: GuildView = {
  version: 0,
  hasGuild: false,
  name: '',
  level: 0,
  masterName: '',
  memberCount: 0,
  maxMembers: 0,
  pxp: 0,
  penya: 0,
  notice: '',
  logo: 0,
  isMaster: false,
  salaries: [0, 0, 0, 0, 0],
  rankCounts: ['00/00', '00/00', '00/00', '00/00', '00/00'],
  members: [],
  selfWar: false,
  war: EMPTY_GUILD_WAR,
};

/**
 * The power matrix the appellation page and the agit page share (FIX: the original kept two copies that each sent all five words). `words` is the
 * DRAFT (the words of the guild plus the player's unsent changes), `dirty` says it differs from the guild's, `editable` is the master.
 */
export interface PowerView {
  readonly words: readonly number[];
  readonly editable: boolean;
  readonly dirty: boolean;
}

export const EMPTY_POWER_VIEW: PowerView = {
  words: [0, 0, 0, 0, 0],
  editable: false,
  dirty: false,
};

/** `MGI_*` of the member menu (`WndGuildTabMember.h`), in the order of the original's menu. */
export const MGI = {
  APPELL_UP: 1,
  APPELL_DOWN: 2,
  CLASS_UP: 3,
  CLASS_DOWN: 4,
  NICKNAME: 5,
  GUILD_LEAVE: 6,
  CHG_MASTER: 7,
} as const;

/** One entry of the member menu. */
export interface GuildMenuEntry {
  readonly id: number;
  readonly text: string;
}

export interface GuildActions {
  /** `WIDC_BUTTON4` of the info tab: the first naming (`APP_GUILDNAME`), or the box `TID_GAME_GUILDNOTCHGNAME` when the guild has a name. */
  setName(): void;
  /** `WIDC_BUTTON1`: the notice window. */
  setNotice(): void;
  /** `WIDC_BUTTON2`: the logo window after the checks of the original. */
  setLogo(): void;
  /** `WIDC_BUTTON3`: the disband question after the checks of the original. */
  disband(): void;
  /** The right-click menu of the member page for member `id`: only the entries the own power allows (`OnRButtonUp`). */
  memberMenu(id: number): readonly GuildMenuEntry[];
  /** An entry of that menu: `SendGuildMemberLv`, `SendGuildClass`, `SendGuildNickName` window, `SendRemoveGuildMember`, `SendChgMaster`. */
  pickMember(id: number, entry: number): void;
  /** A check box of the appellation page or the agit page (`rank` 0..4, one `PF_*` bit). */
  setPower(rank: number, bit: number, on: boolean): void;
  /** `WIDC_OK` of the appellation page / `WIDC_BUTTON1` of the agit page: `SendGuildAuthority( power[5] )`, only when something changed. */
  sendPower(): void;
  /** `WIDC_BUTTON1..5` of the appellation page: the salary window of the rank. */
  openPay(rank: number): void;
  /** `WIDC_BUTTON1` of the war tab (the master): the declaration window. */
  declareWar(): void;
  /** `WIDC_BUTTON2` (the master): the truce question. */
  truce(): void;
  /** `WIDC_BUTTON3` (every member): the surrender question. */
  surrender(): void;
}

export interface GuildPorts {
  readonly view: ViewSource<GuildView>;
  readonly power: ViewSource<PowerView>;
  readonly actions: GuildActions;
  /** The wall clock in unix seconds (the time left of the war tab). */
  now(): number;
}

// ---- the dialogs --------------------------------------------------------------------------------------------------------------------------------

/**
 * `CWndGuildConfirm` (`APP_GUILD_INVATE`): "`<guild>` is inviting you. Will you join the Guild?". Yes (`WIDC_OK`, or Enter) asks the caller to send
 * `AddGuildMemberReq`; No and the close button just close (KEEP: the legacy client never sent `IgnoreGuildInvite`).
 */
export interface InviteRequest {
  readonly guildName: string;
  /** `SendAddGuildMember( idMaster, info )`. */
  yes(): void;
  closed?(): void;
}

/**
 * `CWndGuildName` (`APP_GUILDNAME`): the first naming of the guild, or the rename with a scroll. OK hands the trimmed text to the caller, which runs the
 * pre-checks of the original (`TID_DIAG_0011` ...) and sends `SendGuildSetName` / `SendQuerySetGuildName`; `false` keeps the window.
 */
export interface NameRequest {
  readonly initial: string;
  submit(name: string): boolean;
  closed?(): void;
}

/** `CWndGuildNickName` (`APP_GUILD_NICKNAME`): the alias of a member; OK -> `SendGuildNickName( idPlayer, alias )` after the checks. */
export interface NicknameRequest {
  readonly initial: string;
  submit(alias: string): boolean;
  closed?(): void;
}

/** `CWndGuildDisMiss` (`APP_GUILD_DISMISS`, modal): Yes -> `SendDestroyGuild`. */
export interface DismissRequest {
  yes(): void;
  closed?(): void;
}

/** `CWndGuildNotice` (`APP_GUILD_NOTICE`, modal): `BUTTON2` -> `send( text )` (`SendGuildNotice`); `false` keeps the window (the notice was too long). */
export interface NoticeRequest {
  readonly initial: string;
  send(text: string): boolean;
  closed?(): void;
}

/** `CWndGuildSetLogo` (`APP_GUILD_SETLOGO`, modal): OK -> `SendGuildLogo( logo )`. `gm` shows the extra row of the logos 21..27. */
export interface LogoRequest {
  /** `pGuild->m_dwLogo` (the selection the window starts with). */
  readonly current: number;
  readonly gm: boolean;
  pick(logo: number): void;
  closed?(): void;
}

/** `CWndGuildPayConfirm` (`APP_GUILD_PAYCONFIRM`, modal): the salary of one rank; `submit( text )` runs the digit and range checks; `false` clears the edit. */
export interface PayRequest {
  readonly rank: number;
  submit(text: string): boolean;
  closed?(): void;
}

/** `CWndGuildMerit` (`APP_GUILD_MERIT1`): the penya and the gems a member contributes. */
export interface MeritRequest {
  /** The gem stacks of the bag (`IK3_GEM`). */
  gems(): readonly InventoryItem[];
  /** `SetExtra( n )`: reserves `extra` pieces of a stack (0 releases it). */
  reserve(objId: number, extra: number): void;
  /** OK / Enter: the penya typed (`atoi`, never below 0) and whether the gems are contributed; the caller pre-checks and sends `SendGuildContribution`. */
  submit(penya: number, gems: boolean): void;
  closed?(): void;
}

// ---- the bank and its log -----------------------------------------------------------------------------------------------------------------

export interface GuildBankView {
  /** Bumps with every change of the bank. */
  readonly version: number;
  /** The 42 slots (`null` = empty). */
  readonly items: readonly (InventoryItem | null)[];
  /** `m_nGoldGuild`. */
  readonly gold: number;
  /** The own character is the master: the cloak button exists for the master only. */
  readonly isMaster: boolean;
  /** `features.new_font_color`. */
  readonly newFontColor: boolean;
}

export interface GuildBankActions {
  /** An inventory item dropped on the grid (`WIN_ITEMDROP` nID 11): the checks and `SendPutItemGuildBank`; `false` shows the forbid cursor. */
  putFromInventory(sc: Shortcut): boolean;
  /** `WIDC_BUTTON1`: the contribution window. */
  support(): void;
  /** `WIDC_BUTTON2`: `SendCreateGuildCloak` after the level check. */
  cloak(): void;
  /** `WIDC_LOG`: opens or closes the log window (master and kingpin only). */
  log(): void;
  /** The window was destroyed: `SendCloseGuildBankWnd`, the quantity box and the log window go. */
  closed(): void;
}

export interface GuildBankRequest {
  readonly view: ViewSource<GuildBankView>;
  readonly actions: GuildBankActions;
}

export interface BankLogView {
  /** Bumps with every answer. */
  readonly version: number;
  /** The formatted lines of the lists 1..4 (index 0..3), newest first. */
  readonly lines: readonly (readonly string[])[];
}

export const EMPTY_BANK_LOG: BankLogView = { version: 0, lines: [[], [], [], []] };

/** `CWndGuildBankLog` (`APP_GUILD_BANK_LOG`): four tabs; selecting one asks for its list (`SendReqGuildBankLogList( kind )`). */
export interface BankLogRequest {
  readonly view: ViewSource<BankLogView>;
  request(kind: BankLogKind): void;
  closed?(): void;
}

// ---- the war dialogs ---------------------------------------------------------------------------------------------------------------------

/** `CWndGuildWarDecl` (`APP_GUILD_WAR`): the target guild name; OK -> `SendDeclWar( idMaster, name )` when the name has 3 or more characters. */
export interface WarDeclRequest {
  submit(name: string): boolean;
  closed?(): void;
}

/** The Yes / No questions of the war: give up (`SendSurrender`), truce (`SendQueryTruce`), truce approval (`SendAcptTruce`). */
export interface WarYesNoRequest {
  yes(): void;
  closed?(): void;
}

/** `CWndGuildWarRequest` (`APP_GUILD_WARREQUEST`): "`<guild>` (`<master>`) declares war"; Yes -> `SendAcptWar( idMaster, idDecl )`. */
export interface WarRequestRequest {
  readonly text: string;
  yes(): void;
  closed?(): void;
}
