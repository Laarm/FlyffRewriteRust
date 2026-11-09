// Ports of the party windows (Plan/42 §19.4 H / C / S, WP-UI-39, WP-UI-40; wired by 45 WP-CFEAT-34, -35). The READ MODELS (`PartyView`,
// `FinderView`: read through a `ViewSource`, diffed by the shells) and the ACTIONS the shells call; the confirm and prompt windows take one
// request object each (the texts and the callbacks), like the duel windows. Plain TS interfaces: `ui/` never imports `features/`. Every
// action names the legacy send behind it; the decisions (the leader checks, the boxes, the messages) are the feature's.

import type { ViewSource } from '../login/ports';

export type { ViewSource } from '../login/ports';
export { constantView, mutableView } from '../login/ports';

/** The colour of the status square of a member (`CWndPartyInfo::OnDraw`): blue fine, yellow under 10 % HP, red dead, grey out of view, black offline. */
export type MemberStatus = 'ok' | 'low' | 'dead' | 'away' | 'offline';

/** One party slot. */
export interface PartyMemberRow {
  /** The character id (`m_idPlayer`). */
  readonly id: number;
  /** Slot 0. */
  readonly leader: boolean;
  /** `m_bRemove`. */
  readonly offline: boolean;
  /** The mover is in view (its HP is known). */
  readonly inView: boolean;
  readonly status: MemberStatus;
  /** The level as the info tab writes it: `75`, `75-M` (master), `75-H` (hero). */
  readonly levelText: string;
  /** `TID_GAME_LEADER` / `TID_GAME_MEMBER` ("L" / "M"). */
  readonly classText: string;
  readonly jobName: string;
  readonly name: string;
  /** The quick window's line: `"<level>. <name>"` (the master / hero formats of the texts). */
  readonly quickText: string;
  /** HP / max HP of a member in view, 0..1; 0 out of view. */
  readonly hp: number;
}

/** One cell of the skill tab (`prj.GetPartySkill(1..9)`). */
export interface PartySkillRow {
  /** `ST_*` (1..8). */
  readonly id: number;
  readonly name: string;
  /** The icon file below `Icon/`. */
  readonly icon: string | null;
  /** usable (black name), `noPoints` (blue name: the points are short), `locked` (red name, dimmed icon: party kind or level). */
  readonly state: 'usable' | 'noPoints' | 'locked';
  /** The time left of a running mode (`"29:58"`), `''` when it does not run. */
  readonly remain: string;
  /** The party level reaches the skill's required level: the cell can be dragged to the taskbar (even in a normal party). */
  readonly dragAllowed: boolean;
}

export interface PartyView {
  readonly members: readonly PartyMemberRow[];
  /** `g_Party.m_nKindTroup`. */
  readonly kind: number;
  /** The text of `WIDC_NAME`: `TID_GAME_PARTY1`, `TID_GAME_PARTY2` or the party name. */
  readonly nameText: string;
  readonly level: number;
  /** The text of `WIDC_EXP`: `"%.2f %%"`. */
  readonly expText: string;
  readonly point: number;
  readonly shareExp: number;
  readonly shareItem: number;
  readonly allowJoin: boolean;
  /** The own character is the leader. */
  readonly leader: boolean;
  /** `g_Party.GetLevel() >= 10 && IsLeader(self)`: the "Change" button. */
  readonly canChange: boolean;
  /** The own character is in a party duel (`m_nDuel == 2`). */
  readonly dueling: boolean;
  /** `features.party_find`. */
  readonly findEnabled: boolean;
  /** The nine cells of the skill tab (the data has eight; a ninth would be empty). */
  readonly skills: readonly PartySkillRow[];
}

export const EMPTY_PARTY_VIEW: PartyView = {
  members: [],
  kind: 0,
  nameText: '',
  level: 1,
  expText: '',
  point: 0,
  shareExp: 0,
  shareItem: 0,
  allowJoin: true,
  leader: false,
  canChange: false,
  dueling: false,
  findEnabled: true,
  skills: [],
};

export interface PartyActions {
  /** Select a member as the target (`SetObjFocus`): a member in view takes the focus, the own row and a click beside the rows clear it (`null`). */
  focusMember(id: number | null): void;
  /** `WIDC_LEAVE`: the leave / kick confirmation for `memberId` (the selected row for a leader, the own id otherwise). */
  leave(selected: number | null): void;
  /** `WIDC_BUTTON1`: hand the lead to the selected member. */
  transferLeader(selected: number | null): void;
  /** `WIDC_CHANGE`: the "Convert to an Advanced Party?" window. */
  openChange(): void;
  /** The item-share radios (`SendChangeShareItem`); a non-leader gets `TID_DIAG_0008`. */
  setItemShare(mode: number): void;
  /** The EXP-share radios (`SendChangeShareExp`); only an advanced party takes it. */
  setExpShare(mode: number): void;
  /** `WIDC_RADIO7`: the allow-join flag (`SendPartyJoinAllow`). */
  setAllowJoin(allow: boolean): void;
  /** `WIDC_BUTTON2`: the simple party display (`APP_PARTY_QUICK`). */
  toggleQuick(): void;
  /** `APP_PARTY_QUICK`'s button: the detailed display (`APP_PARTY`). */
  toggleParty(): void;
  /** The skill tab: double click (`ObjectExecutor(SHORTCUT_SKILL, id, 2)`). */
  useSkill(id: number): void;
  /** The tooltip of a party skill (`PutToolTip_Troupe`); `null` hides it. */
  skillTooltip(id: number, rect: { l: number; t: number; r: number; b: number } | null): void;
}

export interface PartyPorts {
  readonly view: ViewSource<PartyView>;
  readonly actions: PartyActions;
}

// ---- the finder -----------------------------------------------------------------------------------------------------------------

export interface FinderRow {
  readonly partyId: number;
  readonly name: string;
  readonly leaderName: string;
  readonly leaderId: number;
  /** `"3/8"`. */
  readonly sizeText: string;
  readonly level: number;
  readonly point: number;
  readonly memberList: string;
  /** The six mode icons in the order of the list: skill full, EXP up, skill up, link attack, fortune circle, gift box. */
  readonly flags: readonly [boolean, boolean, boolean, boolean, boolean, boolean];
}

export interface FinderView {
  readonly rows: readonly FinderRow[];
  /** The icon files (`item/...` / `icon/...`) of the six flags, `null` where the data has none. */
  readonly flagIcons: readonly (string | null)[];
}

export interface FinderActions {
  /** `JoinParty()` (open and Refresh). */
  refresh(): void;
  /** `SendAddPartyMember(leader, ...)` (Join); the window closes. */
  join(leaderId: number): void;
}

export interface FinderPorts {
  readonly view: ViewSource<FinderView>;
  readonly actions: FinderActions;
}

// ---- the dialogs ---------------------------------------------------------------------------------------------------------------

/** `CWndPartyConfirm` (`APP_PARTY_CONFIRM`): "`<leader>` invites you to join the party. Do you accept?" with Yes / No. */
export interface PartyConfirmRequest {
  readonly text: string;
  /** Yes: `SendAddPartyMember`. */
  yes(): void;
  /** No / the close button: `SendPartyMemberCancle(leader, member, 0)`. */
  no(): void;
  closed?(): void;
}

/** `CWndPartyLeaveConfirm` (`APP_PARTYLEAVE_CONFIRM`): "Are you going to leave the party?" with Yes / No. */
export interface PartyLeaveRequest {
  readonly text: string;
  /** Yes: `SendRemovePartyMember(self, leaveId)`. */
  yes(): void;
  closed?(): void;
}

/** `CWndPartyChangeTroup` (`APP_CHANGETROUP`): "Convert to an Advanced Party?" with OK / Cancel. */
export interface PartyChangeTroupRequest {
  /** OK: the name window opens. */
  ok(): void;
  closed?(): void;
}

/** `CWndPartyChangeName` (`APP_PARTYCHANGENAME`): the name of the advanced party. */
export interface PartyChangeNameRequest {
  /** The text the edit starts with (the last name the character used, else `TID_GAME_PARTY_NAME`). */
  readonly initial: string;
  /** OK: the checks of the original (a refused name opens its message box), then the send. `true` = sent, the window closes. */
  submit(name: string): boolean;
  closed?(): void;
}

/** `CWndSummonParty` (`APP_SUMMON_PARTY`): the leader's question after using the scroll. */
export interface SummonPartyRequest {
  readonly text: string;
  /** OK: `SendSummonParty`. */
  ok(): void;
  closed?(): void;
}

/** `CWndSummonPartyMsg` (`APP_MESSAGEBOX_SUMMONPARTY`): the notice a member gets; OK only closes it. */
export interface SummonPartyMsgRequest {
  /** `TID_DIAG_SUMMONPARTY_CONFIRM`. */
  readonly text: string;
  /** `TID_DIAG_SUMMONPARTY_CONFIRM_USE`. */
  readonly how: string;
  /** The scroll icon (`item/<icon>`). */
  readonly icon: string | null;
  closed?(): void;
}

/** `CWndSummonPartyUse` (`APP_SUMMON_PARTY_USE`): the member's teleport question. */
export interface SummonPartyUseRequest {
  readonly text: string;
  /** OK: `SendSummonPartyConfirm`. */
  ok(): void;
  closed?(): void;
}
