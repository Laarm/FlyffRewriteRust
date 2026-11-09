// Ports of the secret-room windows (Plan/42 §19.4 K, 45 §28.3, §28.6; WP-UI-56, WP-UI-57; wired by 45 WP-CFEAT-52 in `features/events/secretRoom` and `features/events/tax`): each window is opened with ONE
// request object that carries what it shows and the callbacks of its buttons, the pattern of `ui/windows/siege`. The shells keep the control behaviour of the original
// (`src:_Interface/WndSecretRoom.cpp`, `WndGuildCombat1to1.cpp:20-545, 730-760`: which button does what, in which order the checks run, which text a refusal prints); the decisions behind
// the sends and the read models are the features'.
//
// Send table (ref/99 App. B; the features carry the `// legacy-send:` markers):
//
//   window                              control                      callback                        message
//   -----------------------------------------------------------------------------------------------------------------------------------------------------------
//   WndSecretRoomOffer (700)            ACCEPT, confirm OK           OfferRequest.tender             SendSecretRoomTender       SecretRoomTender{penya}
//   WndSecretRoomOfferState (704)       BUTTON1 (apply)              OfferStateRequest.apply         SendSecretRoomTenderOpenWnd SecretRoomTenderOpenWndReq
//                                       BUTTON2 (refresh)            OfferStateRequest.refresh       SendSecretRoomTenderView   SecretRoomTenderViewReq
//   WndLineUpSelection (702)            FINISH                       LineUpRequest.submit            SendSecretRoomLineUpMember SecretRoomLineUpMember{ids}
//   WndLineUpSelection (672)            FINISH                       LineUpRequest.submit            SendGC1to1MemberLienUp     Gc1to1MemberLineUp{ids}      (ui-minigames, WP-CFEAT-45)
//   WndSecretRoomInfoBox (710)          YES                          InfoBoxRequest.yes              SendSecretRoomTeleportToNPC SecretRoomTeleportToNpc
//   WndSecretRoomCancelConfirm (760)    YES                          CancelRequest.yes               SendSecretRoomTenderCancelReturn SecretRoomTenderCancelReturn
//   WndSecretRoomChangeTaxRate (715)    OK, confirm OK               ChangeTaxRequest.setRate        SendTaxRate                TaxSetTaxRate{continent, sales_rate, purchase_rate}
//   WndSecretRoomBoard (747), WndSecretRoomCheckTaxRate (716), WndSecretRoomQuick (754): nothing is sent (754 sets the target through `QuickActions.click`)
//
// Not here: `SendSecretRoomTenderOpenWnd` (menu 84), `SendSecretRoomLineUpOpenWnd` (menu 86), `SendSecretRoomEntrance` (87), `SendTeleportToSecretRoomDungeon` (100) and
// `SendTeleportToHeavenTower` are menu entries (`features/events/secretRoom/menu.ts`, `features/events/heavenTower`).

import type { ViewSource } from '../login/ports';
import type { SelectionMember } from '../siege/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';
export { I32_MAX, MIN_LINEUP_LEVEL } from '../siege/ports';
export type { SelectionMember } from '../siege/ports';

/** What every secret-room window needs of the rest of the client: the texts and the standard boxes (`prj.GetText`, `g_WndMng.OpenMessageBox`, `OpenCustomBox`). */
export interface SecretRoomHost {
  /** `prj.GetText( tid )`; `''` while the texts bundle is missing. */
  text(tid: number): string;
  /** `str.Format( prj.GetText( tid ), ... )`. */
  fmt(tid: number, ...args: readonly (string | number | bigint)[]): string;
  /** `g_WndMng.OpenMessageBox( text )`. */
  box(text: string): void;
  /** An OK / Cancel box over a window (`CWndMessageBox::Initialize( ..., MB_OKCANCEL )`): `onOk` runs for OK only. */
  confirm(text: string, onOk: () => void, owner?: object): void;
}

// ---- APP_SECRETROOM_OFFER (700) ---------------------------------------------------------------------------------------------------------------------

/** `SecretRoomTenderOpenWndNtf` as the bid window shows it (`CWndSecretRoomOffer`, `WndSecretRoom.cpp:407-540`). */
export interface OfferRequest {
  /** `m_dwMinGold`: the minimum bid. */
  readonly min: number;
  /** `m_dwReqGold`: the bid of the guild, 0 = none yet. */
  readonly req: number;
  /** The panel `secretroomdesc` of the lazy help bundle (`WIDC_TEXT1`); empty = no text. */
  readonly info: string;
  /** The confirmed bid in penya (1 .. 2 147 483 647): `SendSecretRoomTender`. */
  tender(penya: number): void;
  closed?(): void;
}

// ---- APP_SECRETROOM_OFFERSTATE (704) ------------------------------------------------------------------------------------------------------------------

export interface OfferStateRequest {
  /** The seconds until the next opening (`remain_s`), counted down by the window every process. */
  readonly secondsToNext: number;
  /** The bidding guilds in rank order, the KNOWN ones only (`"No.%.2d\t\t<guild>"`). */
  readonly guilds: readonly string[];
  /** The own rank (1-based), `0` = the own guild is not in the list (the field is blank). */
  readonly rank: number;
  /** The bid of the own guild (`WIDC_STATIC6`, money style). */
  readonly myTender: number;
  /** `BUTTON1`: closes the window and asks for the bid window: `SendSecretRoomTenderOpenWnd`. */
  apply(): void;
  /** `BUTTON2`: `SendSecretRoomTenderView` (the answer replaces the window). */
  refresh(): void;
  closed?(): void;
}

// ---- APP_SECRETROOM_SELECTION (702) and APP_GUILDCOMBAT_1TO1_SELECTION (672) ------------------------------------------------------------------------

/** Which of the two copies of the line-up editor a `WndLineUpSelection` is. */
export type LineUpKind = 'secretRoom' | 'gc1to1';

/** The facts of the line-up editor both kinds share. */
export interface LineUpBase {
  /** The fewest members the line-up needs (`m_nMinGuildMemberNum` / `nMinMember`). */
  readonly min: number;
  /** The most members it may hold (`m_nMaxGuildMemberNum` / `nMaxMember`). */
  readonly max: number;
  /** The stored line-up (character ids, in order). */
  readonly selected: readonly number[];
  /** The online members in the order of the left list: ascending by level, equal levels in member order. */
  members(): readonly SelectionMember[];
  /** A member of the own guild (online or not) for a pre-selected id, `null` when the id is no member. */
  memberOf(id: number): SelectionMember | null;
  /** `pGuild->IsMaster( id )`. */
  isMaster(id: number): boolean;
  /** FINISH: the line-up in order. */
  submit(ids: readonly number[]): void;
  closed?(): void;
}

/** `SecretRoomLineUpOpenWndNtf`: the editor of the secret room (702). The level limit is `MIN_LINEUP_LEVEL` (30, the server's `MinGuildMemberLevel`). */
export type SecretRoomSelectionRequest = LineUpBase;

/** The editor of the 1:1 guild combat (672, `CWndGuildCombat1to1Selection`; opened by ui-minigames through `ui.secretRoom.openGc11Selection`, the `1TO1` texts). */
export interface Gc11SelectionRequest extends LineUpBase {
  /** `Gc1to1MemberLineUpOpenWndNtf.min_level`: the lowest level of a line-up member. */
  readonly minLevel: number;
  /** The member is the master or a kingpin. */
  isKingpin(id: number): boolean;
  /** `g_GuildCombat1to1Mng.m_nState == OPEN`: the line-up may be edited (else `CANNOT_MAKEUP`). */
  canEdit(): boolean;
}

/** What the one class works with. */
export interface LineUpRequest extends LineUpBase {
  readonly kind: LineUpKind;
  readonly minLevel: number;
  isKingpin(id: number): boolean;
  canEdit(): boolean;
}

// ---- APP_SECRETROOM_BOARD (747), APP_SECRETROOM_MSG (710), APP_SECRETROOM_CANCEL_CONFIRM (760) ----------------------------------------------------

export interface BoardRequest {
  /** The panel text of the lazy help bundle (`SecretRoomBoard.inc`). */
  readonly text: string;
  closed?(): void;
}

export interface InfoBoxRequest {
  readonly text: string;
  /** YES: `SendSecretRoomTeleportToNPC` (the server honours it inside the worlds 151 .. 167 only). */
  yes(): void;
  closed?(): void;
}

export interface CancelRequest {
  /** YES: `SendSecretRoomTenderCancelReturn`. */
  yes(): void;
  closed?(): void;
}

// ---- APP_SECRETROOM_TAXRATE_CHANGE (715) and APP_SECRETROOM_TAXRATE_CHECK (716) ------------------------------------------------------------------

export interface ChangeTaxRequest {
  /** The continent whose rates are set (15 East, 240 West). */
  readonly continent: number;
  /** The CURRENT rates in percent (the integer of the tax slice). */
  readonly salesNow: number;
  readonly purchaseNow: number;
  /** The presets of the new rates (`max( current, min )`). */
  readonly salesPreset: number;
  readonly purchasePreset: number;
  readonly min: number;
  readonly max: number;
  /** The panel `secretroomchangetaxrate` of the lazy help bundle. */
  readonly info: string;
  /** The confirmed rates: `SendTaxRate( continent, sales, purchase )`. */
  setRate(continent: number, sales: number, purchase: number): void;
  closed?(): void;
}

export interface CheckTaxRequest {
  /** The integer rates of the player's area, 0 for no area. */
  readonly salesRate: number;
  readonly purchaseRate: number;
  /** The name of the occupying guild, `null` for none (or unknown). */
  readonly guild: string | null;
  /** The master of that guild: a name, `''` while the player-data cache does not know it yet. */
  readonly master: ViewSource<string | null>;
  /** The panel `secretroomchecktaxrate` of the lazy help bundle. */
  readonly info: string;
  closed?(): void;
}

// ---- APP_SECRETROOM_QUICK (754) ---------------------------------------------------------------------------------------------------------------------

/** One member row of the quick window. */
export interface QuickRowView {
  /** The character id. */
  readonly id: number;
  /** The text before the cut at 13 characters. */
  readonly text: string;
  /** `0xAARRGGBB`. */
  readonly color: number;
  /** The HP fraction 0 .. 1; 0 out of view. */
  readonly hp: number;
  readonly inView: boolean;
  /** The row is the own character (a click clears the target). */
  readonly self: boolean;
}

export interface QuickView {
  readonly rows: readonly QuickRowView[];
}

export const EMPTY_QUICK_VIEW: QuickView = { rows: [] };

export interface QuickActions {
  /** A click on a row: `true` when it was taken (another member in view, or the own row), `false` when it changes nothing. */
  click(id: number): boolean;
}

export interface QuickRequest {
  readonly view: ViewSource<QuickView>;
  readonly actions: QuickActions;
  closed?(): void;
}

// ---- APP_SECRETROOM_GUILDMEM_MNG (751) and APP_SECRETROOM_GUILDMEMBER (752): shells that V15 never opens ------------------------------------------------

export interface GuildMemMngRequest {
  /** The members: `"%d %s"` (the rank number and the name cut at 14). */
  readonly members: readonly { readonly name: string }[];
  closed?(): void;
}

export interface GuildMemberRequest {
  /** The member's name (the window's title). */
  readonly name: string;
  readonly level: number;
  /** The member is in view (else the text is grey and the gauge empty). */
  readonly inView: boolean;
  /** The HP fraction 0 .. 1. */
  readonly hp: number;
  closed?(): void;
}
