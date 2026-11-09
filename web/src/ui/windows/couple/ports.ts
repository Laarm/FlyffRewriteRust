// Ports of the couple and honor windows (Plan/42 §4.5, §19.4 K / O, WP-UI-52; implemented by 45 WP-CFEAT-40). The READ MODELS (`*View`, read through a `ViewSource` and polled
// by the page's `process`: the shell diffs the object it gets, so a feature that returns the same object while nothing changed costs nothing) and the ACTIONS the shells call.
// Plain TS interfaces: `ui/` never imports `features/`. Every action names the legacy send behind it; the decisions (the cache of the partner, the texts, the sends) are the
// feature's.
//
//   send table                                                                       which control
//   -------------------------------------------------------------------------------  --------------------------------------------------------------------
//   CoupleActions.breakUp()      the couple message window `TID_GAME_COUPLECANCEL`    WIDC_BUTTON1 of APP_COUPLE_TAB_INFO (acts only with `canBreak`)
//   WndCoupleMessage OK           SendDecouple (break up) / SendPropose (proposal) / SendCouple (accept)     CoupleMessageRequest.ok
//   WndCoupleMessage Cancel       nothing (break up, proposal) / SendRefuse (accept)                           CoupleMessageRequest.cancel
//   HonorActions.opened()        SendReqHonorList      the page is created (the character window opens)
//   HonorActions.change(id)      SendReqHonorTitleChange( id | -1 )                  WIDC_BUTTON1 of APP_HONOR

import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

// ---- APP_COUPLE_TAB_INFO -----------------------------------------------------------------------------------------------------

/** What `CWndCoupleTabInfo::OnDraw` rewrites every frame, joined from the couple record and the player-data cache. Every text is `''` while the player is single. */
export interface CoupleInfoView {
  readonly coupled: boolean;
  /** The partner's name, `''` until the cache has answered (the partner's data stay blank until `ver > 0`). */
  readonly name: string;
  /** The job name of the partner. */
  readonly job: string;
  /** The level of the partner as text. */
  readonly level: string;
  /** The couple level as text. */
  readonly coupleLevel: string;
  /** The sprite (index in `icon/icon_playerdata.inc`) of the master / hero mark beside the level, `null` for none. */
  readonly mark: number | null;
  /** The EXP gauge fill, `0..1`, with the minimum fills of the original already applied. */
  readonly rate: number;
  /** The break-up button acts: coupled and the partner's name is in the cache (FIX: the original asked about an empty name). */
  readonly canBreak: boolean;
}

export const EMPTY_COUPLE_INFO: CoupleInfoView = {
  coupled: false,
  name: '',
  job: '',
  level: '',
  coupleLevel: '',
  mark: null,
  rate: 0,
  canBreak: false,
};

// ---- APP_COUPLE_TAB_SKILL ----------------------------------------------------------------------------------------------------

/** One of the three skill cells: the item of the skill kind, lit when the couple level reaches a level of it. */
export interface CoupleSkillCell {
  /** The item name: the tooltip of the cell, even when it is dark. */
  readonly name: string;
  /** `item/<icon>`, `null` when the item is unknown. */
  readonly icon: string | null;
  readonly lit: boolean;
}

export interface CoupleSkillView {
  readonly cells: readonly CoupleSkillCell[];
  /** The text panel `coupleskillinfo` of the help bundle (markup), `''` until it is loaded. */
  readonly info: string;
}

export const EMPTY_COUPLE_SKILL: CoupleSkillView = { cells: [], info: '' };

export interface CoupleActions {
  /** `WIDC_BUTTON1` ("break up"): the feature opens the couple message window (`TID_GAME_COUPLECANCEL( partner )`, OK = `SendDecouple`). */
  breakUp(): void;
  /** The skill page was created: the feature asks for the lazy `help` bundle that holds the skill text. */
  skillPageOpened(): void;
}

export interface CouplePorts {
  readonly info: ViewSource<CoupleInfoView>;
  readonly skill: ViewSource<CoupleSkillView>;
  readonly actions: CoupleActions;
}

// ---- the couple message window -------------------------------------------------------------------------------------------------

/** The window-manager key of the couple box (an instance of `APP_MESSAGEBOX` of its own, so that `ui.msgbox.open` and the couple box never replace each other). */
export const COUPLE_MESSAGE_KEY = 'couple';

/**
 * `CWndCoupleMessage` (`MB_OKCANCEL` on `APP_MESSAGEBOX`): one text, OK and Cancel. The three modes of the original (`CM_SENDPROPOSE`, `CM_RECEIVEPROPOSE`, `CM_CANCELCOUPLE`) differ only
 * in the text and in what the two buttons do, which is the feature's.
 */
export interface CoupleMessageRequest {
  readonly text: string;
  /** OK. */
  ok(): void;
  /** Cancel (`SendRefuse` for a received proposal, nothing for the other two). */
  cancel?(): void;
  /** The window went away by any route (OK, Cancel, the title bar's close, a replacement). */
  closed?(): void;
}

// ---- APP_HONOR ---------------------------------------------------------------------------------------------------------------------

/** One earned title of the list (`EarnedTitle`). */
export interface HonorEntry {
  readonly id: number;
  readonly title: string;
}

export interface HonorView {
  /** The earned titles in list order: the list box shows "No Title" first, then these. */
  readonly titles: readonly HonorEntry[];
}

export const EMPTY_HONOR_VIEW: HonorView = { titles: [] };

export interface HonorActions {
  /** The page was created (the character window opened): `SendReqHonorList` (Plan/45 §24.3; the original relied on the join data, harmless). */
  opened(): void;
  /** `WIDC_BUTTON1` (OK): `SendReqHonorTitleChange( id )`, `-1` for "No Title". */
  change(id: number): void;
}

export interface HonorPorts {
  readonly view: ViewSource<HonorView>;
  readonly actions: HonorActions;
}
