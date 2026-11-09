// Ports of the guild-siege windows (Plan/42 §19.4 K, 45 §28.4; WP-UI-49, WP-UI-50, WP-UI-51; wired by 45 WP-CFEAT-41 in `features/events/siege`, the 1:1 mode by WP-CFEAT-45): each
// window is opened with ONE request object that carries what it shows and the callbacks of its buttons. The shells keep the control behaviour of the original
// (`src:_Interface/WndField.cpp:15945-18750`, `WndGuildCombat1to1.cpp:545-805`: which button does what, in which order the checks run, which text a refusal prints); the
// decisions behind the sends and the read models (the rows of a ranking, the kill log) are the siege feature's.
//
// Send table (ref/99 App. B; the feature carries the `// legacy-send:` markers):
//
//   window                          control                          callback                   message
//   -----------------------------------------------------------------------------------------------------------------------------------------------------------
//   WndGuildCombatOffer (675/545)   ACCEPT, confirm OK, mode 0        OfferRequest.bid           SendGuildCombatApp      InGuildCombat{gcInApp{penya}}
//                                   ACCEPT, confirm OK, mode 1        OfferRequest.bid           SendGC1to1Tender        Gc1to1Tender{penya}            (CFEAT-45)
//   WndGuildCombatState (547)       BUTTON1 (apply), mode 0           StateRequest.apply         SendGuildCombatWindow   InGuildCombat{gcInWindow}
//                                   BUTTON2 (refresh)                 StateRequest.refresh       SendGCRequestStatusWindow RequestStatus
//   WndGuildCombatSelection (551)   FINISH                            SelectionRequest.submit    SendGCSelectPlayer      SelectPlayerGuildCombat{window: false, defender, ids}
//   WndGuildCombatJoinSelection     BUTTON1..4, the timeout           SelStartRequest.select     SendGCSelectMap         SelectMapGuildCombat{map}
//   WndGuildCombatInfoBox (563)     YES, mode 0                       InfoBoxRequest.yes         SendGCTele              TeleGuildCombat
//                                   YES, mode 1                       InfoBoxRequest.yes         SendGC1to1TeleportToNPC Gc1to1TeleportToNpc            (CFEAT-45)
//   WndGuildWarCancelConfirm (561)  YES, mode 0                       CancelRequest.yes          SendGuildCombatCancel   OutGuildCombat
//                                   YES, mode 1                       CancelRequest.yes          SendGC1to1TenderCancel  Gc1to1TenderCancel             (CFEAT-45)
//   WndGuildWarJoinConfirm (266)    YES (not a vagrant), mode 0       JoinRequest.join           SendGCJoin              JoinGuildCombat
//                                   YES (not a vagrant), mode 1       JoinRequest.join           SendGC1to1TeleportToStage Gc1to1TeleportToStage        (CFEAT-45)
//   WndGuildCombatTabResultLog      BUTTON1 (save)                    ResultRequest.save         (a browser download, no message)
//   WndGuildCombatRankPerson (575)  nothing is sent (the list arrives with `GcPlayerPoint`, asked by the menu entry SendGCPlayerPoint)
//
// Not here: `SendGCSelectPlayerWindow`, `SendGCGetPenyaGuild`, `SendGCGetPenyaPlayer`, `SendGCJoin`'s menu half and `SendGCPlayerPoint` are menu entries of the siege NPC
// (`features/events/siege/menu.ts`); `SendGCGetItem` is inactive in V15.

import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** The combat the shared shells serve: 0 = the guild siege, 1 = the 1:1 guild combat (M7, WP-CFEAT-45; only the titles and the port differ). */
export type CombatMode = 0 | 1;

/** `MAXJOINGUILDSIZE`: the guilds the siege admits (`GuildCombat.txt` is server data and no bundle carries it, so the client keeps the constant; FIX of the original's leftover 8). */
export const MAX_JOIN_GUILD_SIZE = 15;

/** `INT_MAX`: the highest bid (the server refuses more; FIX: the original `atoi` overflowed). */
export const I32_MAX = 2_147_483_647;

/** `m_nMapNum` of a gate that is not chosen yet (the server's random gate, 99). */
export const GATE_RANDOM = 99;

/** The level a player must have to be put in the line-up (`TID_GAME_GUILDCOMBAT_LIMIT_LEVEL_NOTICE`). */
export const MIN_LINEUP_LEVEL = 30;

/** The war state in which the line-up may be edited (`NOTENTER_COUNT_STATE`). */
export const WAR_STATE_NOTENTER_COUNT = 101;

/** What every siege window needs of the rest of the client: the texts and the standard boxes (`prj.GetText`, `g_WndMng.OpenMessageBox`, `OpenCustomBox`). */
export interface SiegeHost {
  /** `prj.GetText( tid )`; `''` while the texts bundle is missing. */
  text(tid: number): string;
  /** `str.Format( prj.GetText( tid ), ... )`. */
  fmt(tid: number, ...args: readonly (string | number | bigint)[]): string;
  /** `g_WndMng.OpenMessageBox( text )`. */
  box(text: string): void;
  /** An OK / Cancel box over a window (`CWndMessageBox::Initialize( ..., MB_OKCANCEL )`): `onOk` runs for OK only. */
  confirm(text: string, onOk: () => void, owner?: object): void;
}

// ---- APP_GUILDCOMBAT_1TO1_OFFER (675) and APP_GUILDCOMBAT_OFFER (545) -------------------------------------------------------------

/** `CWndGuildCombat1to1Offer( mode )` (the V15 siege bid window) and `CWndGuildCombatOffer` (CA1 only, no opener). */
export interface OfferRequest {
  readonly mode: CombatMode;
  /** `true` builds the 545 layout (the pre-V11 window with the prize line); the siege opens 675. */
  readonly classic?: boolean;
  /** `m_dwMinGold`: the minimum bid. */
  readonly min: number;
  /** `m_dwReqGold`: the bid of the guild, 0 = none yet. */
  readonly myBid: number;
  /** The total of the admitted bids; shown by the 545 layout only (`WIDC_STATIC6`). */
  readonly prize?: number;
  /** The panel `guildcombatredchipguild` of the lazy help bundle (`WIDC_TEXT1`); empty = no text. */
  readonly info: string;
  /** `EnableAccept( TRUE )`: the button is enabled (default). */
  readonly acceptEnabled?: boolean;
  /** The confirmed bid in penya (1 .. 2 147 483 647): `SendGuildCombatApp` / `SendGC1to1Tender`. */
  bid(penya: number): void;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_OFFERSTATE (547) ---------------------------------------------------------------------------------------------

export interface StateRequest {
  /** The seconds until the next siege opens (`tNext`), counted down by the window every process. */
  readonly secondsToNext: number;
  /** The applying guilds in rank order (`"No.%.2d\t\t<guild>"`, the order sent). */
  readonly guilds: readonly string[];
  /** The own rank (1-based), `-1` = the own guild is not in the list. */
  readonly rank: number;
  /** The bid of the own guild (`WIDC_STATIC6`, money style). */
  readonly myBid: number;
  /** The total of the admitted bids (`WIDC_STATIC8`, money style). */
  readonly prize: number;
  /** `BUTTON1`: closes the window and asks for the bid window: `SendGuildCombatWindow`. */
  apply(): void;
  /** `BUTTON2`: `SendGCRequestStatusWindow` (the answer replaces the window). */
  refresh(): void;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_SELECTION (551) ------------------------------------------------------------------------------------------------

/** A guild member the line-up editor lists. */
export interface SelectionMember {
  readonly id: number;
  readonly name: string;
  readonly level: number;
  /** The name of the job (`prj.m_aJob[ job ].szName`). */
  readonly job: string;
}

export interface SelectionRequest {
  /** `nMaxWarMember`: the fighters at the same time (the blue band of the right list). */
  readonly maxWar: number;
  /** `nMaxJoinMember`: the players that may be put in the line-up. */
  readonly maxJoin: number;
  /** The online members in the order of the left list: ascending by level, equal levels in member order. */
  members(): readonly SelectionMember[];
  /** A member of the own guild (online or not) for a pre-selected id, `null` when the id is no member. */
  memberOf(id: number): SelectionMember | null;
  /** The stored line-up (`GC_SELECTPLAYER.ids`, in order) and the stored defender (0 = none). */
  readonly selected: readonly number[];
  readonly defender: number;
  /** `g_GuildCombatMng.m_nGCState`, read when the window opens and when FINISH is pressed. */
  warState(): number;
  /** `pGuild->IsMaster( id )`. */
  isMaster(id: number): boolean;
  /** The member is the master or a kingpin of the guild. */
  isMasterOrKingpin(id: number): boolean;
  /** FINISH: `SendGCSelectPlayer( ids, defender )`. */
  submit(defender: number, ids: readonly number[]): void;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_SELSTART (557) ---------------------------------------------------------------------------------------------------

export interface SelStartRequest {
  /** The stored gate 0..3, `99` = random. */
  readonly map: number;
  /** The time to choose, in ms (`join_time_s * 1000`). */
  readonly timeoutMs: number;
  /** `SendGCSelectMap( map )`: at once for a gate button, and with the current choice at the timeout. */
  select(map: number): void;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_BOARD (559) ---------------------------------------------------------------------------------------------------------

export interface BoardRequest {
  readonly mode: CombatMode;
  /** The panel text of the lazy help bundle (`GuildCombatTEXT_<n>_ENG.inc`). */
  readonly text: string;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_MSG (563) and APP_GUILDCOMBAT_MSG2 (565) ---------------------------------------------------------------------------------

export interface InfoBoxRequest {
  readonly mode: CombatMode;
  readonly text: string;
  /** YES: `SendGCTele` (mode 0) or `SendGC1to1TeleportToNPC` (mode 1). */
  yes(): void;
  closed?(): void;
}

export interface InfoBox2Request {
  readonly text: string;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_CANCEL_CONFIRM (561) and the join confirm on APP_CLOSE_EXISTING_CONNECTION (266) ----------------------------------------------

export interface CancelRequest {
  readonly mode: CombatMode;
  /** The colour of `TID_GAME_GUILDWAR_CANCEL` (the TID colour). */
  readonly textColor?: number;
  /** YES: `SendGuildCombatCancel` / `SendGC1to1TenderCancel`. */
  yes(): void;
  closed?(): void;
}

export interface JoinRequest {
  readonly mode: CombatMode;
  /** The colour of `TID_GAME_GUILDWAR_JOIN` (the TID colour). */
  readonly textColor?: number;
  /** The own character is a vagrant (`IsBaseJob`): the entrance is refused with a box. */
  vagrant(): boolean;
  /** YES for a non-vagrant: `SendGCJoin` / `SendGC1to1TeleportToStage`. */
  join(): void;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_RESULT_POINT (567) with the tabs 569 and 571 -----------------------------------------------------------------------------

export interface ResultRequest {
  /** The guild ranking rows (`"%2d   <name>\t(<pts>) WINNER"`), built from the stored precedence slices at open time. */
  readonly guildRows: readonly string[];
  /** The person ranking rows (`... MVP`). */
  readonly personRows: readonly string[];
  /** The kill log in markup (`#cff009c00 ... #nc`), two lines and a blank line per kill. */
  readonly log: string;
  /** The plain text of the log was saved (`GuildLog_YYYYMMDD.txt`): the feature downloads it and answers the file name. */
  save(plainText: string): string;
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_RANK_P (575) with the pages 555 -----------------------------------------------------------------------------------------------

/** One row of a ranking page. */
export interface RankRow {
  /** The dense rank (ties share it). */
  readonly rank: number;
  /** A tie continuation row draws no rank (three blanks). */
  readonly tie: boolean;
  readonly id: number;
  /** The name now known to the player-data cache (`''` while the query is unanswered). */
  readonly name: string;
  readonly job: string;
  readonly points: number;
}

/** The five pages of the person ranking: all, Mercenary, Magician, Acrobat, Assist (at most 100 rows each) and the own row. */
export interface RankView {
  readonly pages: readonly (readonly RankRow[])[];
  /** The own row of each page (found anywhere in the list), `null` when the character is not on the page. */
  readonly own: readonly (RankRow | null)[];
}

export const EMPTY_RANK_VIEW: RankView = {
  pages: [[], [], [], [], []],
  own: [null, null, null, null, null],
};

export interface RankPersonRequest {
  readonly view: ViewSource<RankView>;
  /** The captions of the five tabs: `TID_GAME_TOOLTIP_LOG1`, then the job names Mercenary, Magician, Acrobat, Assist. */
  readonly tabs: readonly string[];
  closed?(): void;
}

// ---- APP_GUILDCOMBAT_RANKING (521) and APP_GUILD_WAR_STATE (519): shells with fixtures only (V15 never instantiates their classes) ------------------

export interface RankingRequest {
  /** The guilds with their win counts, in the order of insertion. */
  readonly guilds: readonly { readonly name: string; readonly wins: number }[];
  closed?(): void;
}

export interface WarStateRequest {
  /** The entry list: guild, master, member count (`InsertGuild( guild, name, n )`; an entry with 0 members is dropped). */
  readonly entries: readonly {
    readonly guild: string;
    readonly master: string;
    readonly count: number;
  }[];
  closed?(): void;
}
