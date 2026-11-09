// Ports of the lord windows (Plan/42 §19.4 K / O, 45 §28.2; WP-UI-54, WP-UI-55; wired by 45 WP-CFEAT-43 in `features/events/lord`): the READ MODEL (`LordView`, observed through a `ViewSource`) and
// the ACTIONS the shells call. The shells keep the control behaviour of the original (`src:_Interface/WndLord.cpp`: which button does what, in which order the checks run, which text a
// refusal prints); the decisions behind the sends (the focus rules of a skill use, the taskbar) are the feature's.
//
// Send table (ref/99 App. B; the feature carries the `// legacy-send:` markers):
//
//   window                          control                              callback                 message
//   ------------------------------------------------------------------------------------------------------------------------------------------------------
//   WndLordTender (718)             BUTTON1, bid >= 100 000 000          actions.addDeposit       SendElectionAddDeposit   ElectionAddDepositReq{total}
//   WndLordPledge (720)             BUTTON2, first pledge, not empty     actions.setPledge        SendElectionSetPledge    ElectionSetPledgeReq{pledge}
//   WndLordConfirm (738)            BUTTON1, re-pledge, not empty        actions.setPledge        SendElectionSetPledge    ElectionSetPledgeReq{pledge}
//   WndLordVote (724)               BUTTON1, name equals a candidate     actions.incVote          SendElectionIncVote      ElectionIncVoteReq{candidate}
//   WndLordEvent (726)              BUTTON5                              actions.createEvent      SendLEventCreate         LEventCreateReq{eIndex, iIndex}
//   WndLordSkillConfirm (736)       BUTTON1                              actions.useSkill         SendLordSkillUse         LordSkillUseReq{skill, targetName}
//   WndLordSkill (728)              double click on a skill icon         actions.castSkill        (`lord.useSkill( id )`: SendLordSkillUse by the focus rules, or the confirm window)
//   WndLordState (722), WndLordInfo (756), WndLordRPInfo (758)   nothing is sent
//
// Opening a window sends nothing: the NPC menu entries 91-94, 101 and 102 only open (`features/events/lord/menu.ts`).

import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** `IElection::ELECTION_STATE`. */
export type LordElectionState = 0 | 1 | 2 | 3;

/** One candidate of the election, in the order of the list (the rank is the position, 1-based). */
export interface LordCandidateRow {
  readonly rank: number;
  /** The character id. */
  readonly id: number;
  /** The name from the player-data cache; `''` while the query is unanswered. */
  readonly name: string;
  readonly level: number;
  /** The name of the job (`prj.m_aJob[ job ].szName`), `''` while unknown. */
  readonly job: string;
  readonly deposit: number;
  readonly votes: number;
  readonly pledge: string;
}

/** One row of `lordevent.inc`. */
export interface LordRateRow {
  readonly factor: number;
  readonly cost: number;
}

/** One lord skill of `lordskill.txt` with its cooldown. */
export interface LordSkillRow {
  /** `LI_*`, 0..7. */
  readonly id: number;
  readonly name: string;
  readonly desc: string;
  /** The icon file under `icon/` (lower case), `null` when the table has none. */
  readonly icon: string | null;
  /** The minutes left of the cooldown. */
  readonly tick: number;
  /** The cooldown in minutes. */
  readonly cooltime: number;
}

/** What the lord windows read of the lord system. */
export interface LordView {
  /** `CCLord::Get()`, `null` = no lord. */
  readonly lordId: number | null;
  /** The name of the lord, `''` when none or unknown. */
  readonly lordName: string;
  readonly state: LordElectionState;
  /** `IElection::IsValid`: the votes reach the requirement. */
  readonly requirementOk: boolean;
  /** `GetBegin() + tCandidacy + tVote` in unix seconds (the server clock), 0 = not scheduled. */
  readonly endVoteAt: number;
  readonly candidates: readonly LordCandidateRow[];
  /** `g_pPlayer->m_idPlayer`. */
  readonly selfId: number;
  /** The own deposit, 0 = not a candidate. */
  readonly ownDeposit: number;
  /** `CCLord::IsLord( own )`. */
  readonly isLord: boolean;
  /** `features.select_lord` (`__SELECT_LORD`): the vote window shows the votes and fills the edit by a click. */
  readonly selectLord: boolean;
  /** `features.player_lordevent` (`__PLAYER_LORDEVENT`): any player may start an event (up to `maxLordEvent` run). */
  readonly playerLordEvent: boolean;
  /** `gameplay.max_lord_event` (`__MAX_LORD_EVENT`). */
  readonly maxLordEvent: number;
  /** `GetComponentSize()`. */
  readonly eventCount: number;
  /** An event of the own character runs (`GetComponent( own )`). */
  readonly ownEvent: boolean;
  /** `lordevent.inc`: the experience rows. */
  readonly expRates: readonly LordRateRow[];
  /** `lordevent.inc`: the item drop rows. */
  readonly itemRates: readonly LordRateRow[];
  readonly skills: readonly LordSkillRow[];
}

export const EMPTY_LORD_VIEW: LordView = {
  lordId: null,
  lordName: '',
  state: 0,
  requirementOk: false,
  endVoteAt: 0,
  candidates: [],
  selfId: 0,
  ownDeposit: 0,
  isLord: false,
  selectLord: true,
  playerLordEvent: true,
  maxLordEvent: 2,
  eventCount: 0,
  ownEvent: false,
  expRates: [],
  itemRates: [],
  skills: [],
};

/** The actions of the lord windows (each one lists the legacy send it covers). */
export interface LordActions {
  /** `SendElectionAddDeposit( total )`. */
  addDeposit(total: number): void;
  /** `SendElectionSetPledge( pledge )`. */
  setPledge(pledge: string): void;
  /** `SendElectionIncVote( candidate )`. */
  incVote(candidate: number): void;
  /** `SendLEventCreate( eIndex, iIndex )`. */
  createEvent(eIndex: number, iIndex: number): void;
  /** `SendLordSkillUse( skill, targetName )`: the confirm window of SUMMON and TELEPORT. */
  useSkill(skill: number, targetName: string): void;
  /** `ObjectExecutor( SHORTCUT_LORDSKILL, id )`: the feature's rules (the confirm window, the focus, the cooldown text). */
  castSkill(skill: number): void;
}

/** Everything the lord windows need of the rest of the client. */
export interface LordPorts {
  readonly view: ViewSource<LordView>;
  readonly actions: LordActions;
  /** The server clock in unix seconds (`time_null()` of the original read the PC clock: FIX). */
  nowS(): number;
  /** A panel of the lazy help bundle (`lordcandidate`, `lordinfo`, `lordrpinfo`); `''` while it is missing. */
  panel(key: string): string;
  /** `g_WndMng.PutString( GetText( tid ), NULL, GetTextColor( tid ) )`. */
  say(tid: number): void;
  /** `g_WndMng.PutString( text )`: a line that has no text id. */
  sayText(text: string): void;
}

/** The texts and boxes every lord window needs (`prj.GetText`, `g_WndMng.OpenMessageBox`). */
export interface LordHost {
  /** `prj.GetText( tid )`; `''` while the texts bundle is missing. */
  text(tid: number): string;
  /** `str.Format( prj.GetText( tid ), ... )`. */
  fmt(tid: number, ...args: readonly (string | number | bigint)[]): string;
  /** `g_WndMng.OpenMessageBox( text )`. */
  box(text: string): void;
}

/** The dependencies of a lord window: the ports of the feature and the texts. */
export interface LordWindowDeps {
  readonly ports: LordPorts;
  readonly host: LordHost;
}

/** The windows a lord window opens or closes (the state window owns the pledge window, the pledge window the confirm: the original deleted them with their owner). */
export interface LordWindowOps {
  /** `SAFE_DELETE( m_pWndPledge ); new CWndLordPledge( rank ); Initialize( this )`. */
  openPledge(rank: number): void;
  /** `SAFE_DELETE( m_pWndConfirm ); new CWndLordConfirm( pledge ); Initialize()`. */
  openConfirm(request: LordConfirmRequest): void;
  /** The pledge window and its confirm window go (`g_WndMng.m_pWndLordState->m_pWndPledge->Destroy()`, the destructor of the state window). */
  closePledge(): void;
  /** Only the confirm window goes. */
  closeConfirm(): void;
}

/** What the confirm window of a re-pledge needs. */
export interface LordConfirmRequest {
  /** The new pledge (`m_pPledge`). */
  readonly pledge: string;
  /** BUTTON1 sent the pledge: the pledge window closes (`g_WndMng.m_pWndLordState->m_pWndPledge->Destroy()`). */
  readonly sent: () => void;
}

/** The hard-coded line of the event window when `__MAX_LORD_EVENT` events run. */
export const MAX_LORD_EVENT_TEXT = 'The Max Lord events is already activated.  Try again Later!';

/** `LI_SUMMON` and `LI_TELEPORT`: the two lord skills that ask for a character name. */
export const LI_SUMMON = 5;
export const LI_TELEPORT = 6;
/** `CCandidate::nMaxPledgeLen`. */
export const MAX_PLEDGE_BYTES = 255;
/** `CCandidate::nMinDeposit`. */
export const MIN_DEPOSIT = 100_000_000;
/** The most digits the deposit edit takes (a total above `2^53` is no penya amount; the original `atol` overflowed a DWORD). */
export const MAX_DEPOSIT_DIGITS = 15;
