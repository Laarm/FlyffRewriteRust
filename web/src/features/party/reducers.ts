// The reducers of the party messages (Plan/45 §22.1, §22.2, §7.6, §7.8; ref/09a §5.7; WP-CFEAT-34, -35): one pure function per legacy handler
// of `CDPClient` (`OnAddPartyMember` ... `OnPartyMapInfo`, `src:Neuz/DPClient.cpp:4439-5640`), taking the party mirror and the message and
// returning the new mirror plus the effects the module carries out (chat lines, boxes, windows, buff icons). No store, no window, no clock:
// the tests are tables of messages and expected lines.

import type { PartyInfo } from '../../gen/proto/flyff/v1/social_types_pb';
import {
  TID_DIAG_0019,
  TID_DIAG_0033,
  TID_DIAG_0046,
  TID_GAME_DIFFRENTPARTY,
  TID_GAME_FULLPARTY1,
  TID_GAME_FULLPARTY2,
  TID_GAME_FULLPARTY3,
  TID_GAME_NOPARTY,
  TID_GAME_NOTTARGET,
  TID_GAME_PARTY,
  TID_GAME_PARTYCHANGE,
  TID_GAME_PARTYCREATE,
  TID_GAME_PARTYDENY,
  TID_GAME_PARTYDESTROY,
  TID_GAME_PARTYEXISTCHR,
  TID_GAME_PARTYEXIT,
  TID_GAME_PARTYFAILENTER,
  TID_GAME_PARTYFAILINVATE,
  TID_GAME_PARTYINVATEOTHERSVR,
  TID_GAME_PARTYJOIN1,
  TID_GAME_PARTYJOIN2,
  TID_GAME_PARTYLUP,
  TID_GAME_PARTYNOINVATE,
  TID_GAME_PARTYNOTEXMEMBER,
  TID_GAME_PARTYOTHER,
  TID_GAME_PARTYREMOVE,
  TID_GAME_PARTY_ALREADYMASTER,
  TID_GAME_PARTY_NAME,
  TID_GAME_TROUPESKILLSTOP,
  TID_GAME_TROUPESKILLUSE,
  TID_GAME_TROUPEUP,
} from '../../gen/defs/text';
import {
  emptyRecord,
  type FinderEntry,
  findMember,
  isLeader,
  MAX_PARTYMODE,
  PARTY_MODE,
  type PartyMember,
  type PartyState,
  skillOfMode,
  ST,
} from './store';

/** `MAX_PARTYLEVEL`: the level at which a normal party stops (and `TID_GAME_TROUPEUP` is printed). */
export const MAX_PARTYLEVEL = 10;
/** `MAX_PTMEMBER_SIZE_SPECIAL`. */
export const MAX_PARTY_MEMBERS = 8;
/** `NaviPoint.Time = 200` draws at the 60 Hz client rate. */
export const NAVI_MARK_MS = 3334;
/** `m_vOtherPoint.size() < 10`. */
export const MAX_NAVI_MARKS = 10;

// ---- the error codes of `MsgHdr.h` ------------------------------------------------------------------------------------------------

export const ERROR_PARTY = {
  NOLEADER: 200,
  FULLPARTY: 201,
  NOPARTY: 202,
  DIFFRENTPARTY: 203,
  DIFFERNTUSERNAME: 204,
  NOTPARTYPOINT: 205,
  NOTPARTYSKILL: 206,
  NOTTARGET: 207,
  NOTMAMBER: 208,
  SAMLEADER: 209,
} as const;

// ---- effects -----------------------------------------------------------------------------------------------------------------------

export type PartyEffect =
  /** `g_WndMng.PutString(text(tid, args), colour of tid)`. */
  | { readonly kind: 'line'; readonly tid: number; readonly args: readonly string[] }
  /** `g_WndMng.OpenMessageBox(text(tid))`. */
  | { readonly kind: 'box'; readonly tid: number }
  /** Open `APP_PARTYCHANGENAME` (after the box of a name that is taken, or for a leader whose party has no name). */
  | { readonly kind: 'openChangeName'; readonly box: number | null }
  /** `pWndWorld->m_buffs.Clear()`: the party buff icons go. */
  | { readonly kind: 'clearBuffs' }
  /** `m_buffs.AddBuff(BUFF_SKILL, skill, 1, ms)`. */
  | { readonly kind: 'buffOn'; readonly skill: number; readonly ms: number }
  /** The mode ended: its icon goes (the original lets it time out). */
  | { readonly kind: 'buffOff'; readonly skill: number }
  /** `g_pPlayer->m_idparty` follows the record. */
  | { readonly kind: 'selfParty'; readonly id: number }
  /** `g_pPlayer->m_idDuelParty = ...; m_nDuel = 2`. */
  | { readonly kind: 'duelParty'; readonly id: number }
  /** STRETCHING started and the own character is idle: `MotionReq{SITDOWN}`. */
  | { readonly kind: 'sit' };

export interface Reduced {
  readonly state: PartyState;
  readonly effects: readonly PartyEffect[];
}

/** What the reducers read of the client. */
export interface PartyEnv {
  /** The own character id (`m_idPlayer`). */
  readonly selfId: number;
  /** `ctx.now()` ms. */
  readonly now: number;
  readonly text: (tid: number) => string;
  /** The name of a party skill (`prj.GetPartySkill(id)->szName`). */
  readonly skillName: (skillId: number) => string;
  /** The name of a character (in view, else the player-data cache), `''` when unknown. */
  readonly nameOf: (idPlayer: number) => string;
  /** The mover of a character is in view. */
  readonly visible: (idPlayer: number) => boolean;
}

const line = (tid: number, ...args: string[]): PartyEffect => ({ kind: 'line', tid, args });

// ---- PartyMember (OnAddPartyMember) ------------------------------------------------------------------------------------------

/** The wire record of the party (`CParty::Serialize` load) as the mirror's record fields. */
export function recordOf(p: PartyInfo, now: number, prev: PartyState): Partial<PartyState> {
  const members: PartyMember[] = p.members.map((m) => ({ id: m.idPlayer, offline: m.removed }));
  const modes: number[] = [];
  const modeEnds: number[] = [];
  for (let i = 0; i < MAX_PARTYMODE; i += 1) {
    const v = p.modeTime[i] ?? 0;
    modes.push(v > 0 ? 1 : 0);
    // modes 0..3 carry the remaining milliseconds, PARSKILL a flag
    modeEnds.push(i < PARTY_MODE.PARSKILL && v > 0 ? now + v : 0);
  }
  const alive = new Set(members.map((m) => m.id));
  const mapPos: Record<number, readonly [number, number, number]> = {};
  for (const [k, v] of Object.entries(prev.mapPos)) if (alive.has(Number(k))) mapPos[Number(k)] = v;
  return {
    id: p.partyId,
    kind: p.kindTroupe,
    name: p.kindTroupe !== 0 ? p.name : '',
    level: p.level,
    exp: p.exp,
    point: p.point,
    shareExp: p.shareExpMode,
    shareItem: p.shareItemMode,
    duelParty: p.idDuelParty,
    modes,
    modeEnds,
    members,
    mapPos,
  };
}

export interface PartyMemberMsg {
  readonly changed: number;
  readonly leaderName: string;
  readonly memberName: string;
  readonly party: PartyInfo | undefined;
}

/**
 * `OnAddPartyMember`: the record replaces the mirror. Old 0 -> new 2: the leader prints `TID_GAME_PARTYCREATE` and
 * `TID_GAME_PARTYJOIN1(member)`, the member `TID_GAME_PARTYJOIN2(leader)`; a growing party `PARTYJOIN1(member)`, a shrinking one
 * `PARTYREMOVE(member)`; no record: `PARTYEXIT` when the leaver is the receiver, else `PARTYDESTROY`, and the party buffs go. The record
 * of the join batch (`changed == 0`, nobody joined) prints nothing: the original printed the "created" / "joined" line for it at every
 * login.
 */
export function onPartyMember(prev: PartyState, m: PartyMemberMsg, env: PartyEnv): Reduced {
  const oldSize = prev.members.length;
  const effects: PartyEffect[] = [];
  const p = m.party;
  if (p && p.members.length > 0) {
    const next: PartyState = { ...prev, ...recordOf(p, env.now, prev) };
    const size = next.members.length;
    if (m.changed !== 0) {
      if (oldSize === 0 && size === 2) {
        if (isLeader(next, env.selfId)) {
          effects.push(line(TID_GAME_PARTYCREATE));
          effects.push(line(TID_GAME_PARTYJOIN1, m.memberName));
        } else {
          effects.push(line(TID_GAME_PARTYJOIN2, m.leaderName));
        }
      } else if (oldSize < size) {
        effects.push(line(TID_GAME_PARTYJOIN1, m.memberName));
      } else if (oldSize > size) {
        effects.push(line(TID_GAME_PARTYREMOVE, m.memberName));
      }
    }
    effects.push({ kind: 'selfParty', id: next.id });
    if (next.duelParty !== 0) effects.push({ kind: 'duelParty', id: next.duelParty });
    return { state: next, effects };
  }
  const next: PartyState = { ...prev, ...emptyRecord() };
  effects.push({ kind: 'selfParty', id: 0 });
  effects.push(line(m.changed === env.selfId ? TID_GAME_PARTYEXIT : TID_GAME_PARTYDESTROY));
  effects.push({ kind: 'clearBuffs' });
  return { state: next, effects };
}

// ---- PartyRequestCancel (OnPartyRequestCancel) -------------------------------------------------------------------------------

/**
 * `OnPartyRequestCancel`: 0 `PARTYFAILINVATE(member)`; 1 `PARTYOTHER(member)` (in another party) or `PARTYEXISTCHR(member)` (in this
 * one); 2 `PARTYFAILENTER`; 3 `PARTYDENY(member)`; 4 `PARTYINVATEOTHERSVR`. The names are resolved by `env.nameOf`; a line that needs
 * a name that is not known is not printed (the original printed nothing for a member out of view).
 */
export function onPartyRequestCancel(
  prev: PartyState,
  m: { readonly leaderId: number; readonly memberId: number; readonly mode: number },
  env: PartyEnv,
): readonly PartyEffect[] {
  const name = env.nameOf(m.memberId);
  switch (m.mode) {
    case 0:
      return name === '' ? [] : [line(TID_GAME_PARTYFAILINVATE, name)];
    case 1:
      if (name === '') return [];
      return [
        line(
          findMember(prev, m.memberId) === -1 ? TID_GAME_PARTYOTHER : TID_GAME_PARTYEXISTCHR,
          name,
        ),
      ];
    case 2:
      return [line(TID_GAME_PARTYFAILENTER)];
    case 3:
      return name === '' ? [] : [line(TID_GAME_PARTYDENY, name)];
    case 4:
      return [line(TID_GAME_PARTYINVATEOTHERSVR)];
    default:
      return [];
  }
}

// ---- ErrorParty (OnErrorParty) -----------------------------------------------------------------------------------------------

/** The `TID_GAME_FULLPARTY*` of a full party by its level: 1-5, 6-9, else the cap of eight. */
export function fullPartyTid(level: number): number {
  if (level >= 1 && level <= 5) return TID_GAME_FULLPARTY1;
  if (level >= 6 && level <= 9) return TID_GAME_FULLPARTY2;
  return TID_GAME_FULLPARTY3;
}

/**
 * `OnErrorParty` (Plan/45 §22.1): 200 `PARTYNOINVATE`; 201 `FULLPARTY1/2/3` by the party level; 202 `NOPARTY`; 203 `DIFFRENTPARTY`; 204
 * the name window again plus the box `TID_DIAG_0033`; 205 box `TID_DIAG_0046`; 206 box `TID_DIAG_0019`; 207 `NOTTARGET` (the cooldown of
 * Blitz / Sphere Circle starts anyway: a no-op in this build, the check is the running buff); 208 `PARTYNOTEXMEMBER`; 209
 * `PARTY_ALREADYMASTER`.
 */
export function onErrorParty(prev: PartyState, code: number): readonly PartyEffect[] {
  switch (code) {
    case ERROR_PARTY.NOLEADER:
      return [line(TID_GAME_PARTYNOINVATE)];
    case ERROR_PARTY.FULLPARTY:
      return [line(fullPartyTid(prev.level))];
    case ERROR_PARTY.NOPARTY:
      return [line(TID_GAME_NOPARTY)];
    case ERROR_PARTY.DIFFRENTPARTY:
      return [line(TID_GAME_DIFFRENTPARTY)];
    case ERROR_PARTY.DIFFERNTUSERNAME:
      return [{ kind: 'openChangeName', box: TID_DIAG_0033 }];
    case ERROR_PARTY.NOTPARTYPOINT:
      return [{ kind: 'box', tid: TID_DIAG_0046 }];
    case ERROR_PARTY.NOTPARTYSKILL:
      return [{ kind: 'box', tid: TID_DIAG_0019 }];
    case ERROR_PARTY.NOTTARGET:
      return [line(TID_GAME_NOTTARGET)];
    case ERROR_PARTY.SAMLEADER:
      return [line(TID_GAME_PARTY_ALREADYMASTER)];
    case ERROR_PARTY.NOTMAMBER:
      return [line(TID_GAME_PARTYNOTEXMEMBER)];
    default:
      return [];
  }
}

// ---- the state messages ------------------------------------------------------------------------------------------------------

/** `OnPartyExpLevel`: the new EXP, level and points; a higher level prints `TROUPEUP` at level 10, else `PARTYLUP`. */
export function onPartyExp(
  prev: PartyState,
  m: { readonly exp: number; readonly level: number; readonly point: number },
): Reduced {
  const effects: PartyEffect[] = [];
  if (m.level > prev.level) {
    effects.push(line(m.level === MAX_PARTYLEVEL ? TID_GAME_TROUPEUP : TID_GAME_PARTYLUP));
  }
  return { state: { ...prev, exp: m.exp, level: m.level, point: m.point }, effects };
}

/** `CParty::SwapPartyMember(0, index)`. */
function swapToFront(members: readonly PartyMember[], index: number): PartyMember[] {
  const out = members.slice();
  const first = out[0];
  const other = out[index];
  if (index <= 0 || !first || !other) return out;
  out[0] = other;
  out[index] = first;
  return out;
}

/** `OnPartyChangeLeader` (`ADDPARTYCHANGELEADER`): the new leader takes slot 0; the party buff icons go. */
export function onChangeLeader(prev: PartyState, newLeader: number): Reduced {
  const index = findMember(prev, newLeader);
  return {
    state: index > 0 ? { ...prev, members: swapToFront(prev.members, index) } : prev,
    effects: [{ kind: 'clearBuffs' }],
  };
}

/**
 * `OnSetPartyMode`: the flag of the mode; on: the points, the end time and, for modes 0..3, the icon (`m_buffs.AddBuff(BUFF_SKILL, id, 1,
 * time)`) and `TID_GAME_TROUPESKILLUSE(name)`; off: `TID_GAME_TROUPESKILLSTOP(name)`. STRETCHING on makes an idle character sit.
 */
export function onSetPartyMode(
  prev: PartyState,
  m: {
    readonly mode: number;
    readonly timeMs: number;
    readonly on: boolean;
    readonly point: number;
  },
  env: PartyEnv,
): Reduced {
  if (m.mode < 0 || m.mode >= MAX_PARTYMODE) return { state: prev, effects: [] };
  const modes = prev.modes.slice();
  const modeEnds = prev.modeEnds.slice();
  modes[m.mode] = m.on ? 1 : 0;
  modeEnds[m.mode] = m.on && m.mode < PARTY_MODE.PARSKILL ? env.now + m.timeMs : 0;
  const state: PartyState = { ...prev, modes, modeEnds, ...(m.on ? { point: m.point } : {}) };
  const skill = skillOfMode(m.mode);
  const effects: PartyEffect[] = [];
  if (skill > 0) {
    const name = env.skillName(skill);
    if (m.on) {
      effects.push({ kind: 'buffOn', skill, ms: m.timeMs });
      effects.push(line(TID_GAME_TROUPESKILLUSE, name));
      if (skill === ST.STRETCHING) effects.push({ kind: 'sit' });
    } else {
      effects.push({ kind: 'buffOff', skill });
      effects.push(line(TID_GAME_TROUPESKILLSTOP, name));
    }
  }
  return { state, effects };
}

/** `OnPartyChangeItemMode`. */
export function onItemMode(prev: PartyState, mode: number): PartyState {
  return { ...prev, shareItem: mode };
}

/** `OnPartyChangeExpMode`. */
export function onExpMode(prev: PartyState, mode: number): PartyState {
  return { ...prev, shareExp: mode };
}

/**
 * `OnPartyChangeName`: a name that is not empty and not the default `TID_GAME_PARTY_NAME` is stored and printed (`TID_GAME_PARTY(name)`;
 * the leader keeps it as the own last name); otherwise the leader is asked for a name (`APP_PARTYCHANGENAME`).
 */
export function onChangeName(prev: PartyState, name: string, env: PartyEnv): Reduced {
  if (name !== '' && name !== env.text(TID_GAME_PARTY_NAME)) {
    const state: PartyState = {
      ...prev,
      name,
      ...(isLeader(prev, env.selfId) ? { ownName: name } : {}),
    };
    return { state, effects: [line(TID_GAME_PARTY, name)] };
  }
  return {
    state: prev,
    effects: isLeader(prev, env.selfId) ? [{ kind: 'openChangeName', box: null }] : [],
  };
}

/** `OnPartyChangeTroup`: the party is an advanced one now (`TID_GAME_PARTYCHANGE`); a non-empty name is stored. */
export function onChangeTroup(prev: PartyState, name: string): Reduced {
  return {
    state: { ...prev, kind: 1, ...(name !== '' ? { name } : {}) },
    effects: [line(TID_GAME_PARTYCHANGE)],
  };
}

/**
 * `OnSetPartyMemberParam` (`PP_REMOVE`): the offline flag of a member. The leader going offline hands the lead to the first online member
 * (swap with slot 0, the buff icons go); with nobody online the record is dropped (`InitParty`).
 */
export function onMemberParam(
  prev: PartyState,
  m: { readonly idPlayer: number; readonly param: number; readonly value: number },
): Reduced {
  if (m.param !== 0) return { state: prev, effects: [] };
  const i = findMember(prev, m.idPlayer);
  if (i < 0) return { state: prev, effects: [] };
  let members = prev.members.map((p, k) => (k === i ? { ...p, offline: m.value !== 0 } : p));
  if (m.value === 1 && i === 0) {
    const j = members.findIndex((p, k) => k >= 1 && !p.offline);
    if (j < 0) return { state: { ...prev, ...emptyRecord() }, effects: [] };
    members = swapToFront(members, j);
    return { state: { ...prev, members }, effects: [{ kind: 'clearBuffs' }] };
  }
  return { state: { ...prev, members }, effects: [] };
}

/** `OnPartyChangeJoin` (`PARTYALLOWJOIN`). */
export function onAllowJoin(prev: PartyState, allow: boolean): PartyState {
  return { ...prev, allowJoin: allow };
}

/** `OnMyPartyName` (`ADDPARTYNAME`): the character's last party name. */
export function onOwnName(prev: PartyState, name: string): PartyState {
  return { ...prev, ownName: name };
}

/**
 * `OnPartyMapInfo`: the position of the member in slot `index`, kept only while that member is NOT in view (a visible member has its live
 * position). A slot out of range is ignored.
 */
export function onMapInfo(
  prev: PartyState,
  m: { readonly index: number; readonly pos: readonly [number, number, number] },
  env: PartyEnv,
): PartyState {
  const member = prev.members[m.index];
  if (!member || env.visible(member.id)) return prev;
  return { ...prev, mapPos: { ...prev.mapPos, [member.id]: m.pos } };
}

// ---- navigator marks (OnSetNaviPoint) ----------------------------------------------------------------------------------------

/** `OnSetNaviPoint`: an entry of the same sender is replaced, a new one is added while there are fewer than 10. */
export function onNaviPoint(
  prev: PartyState,
  m: {
    readonly objId: number;
    readonly name: string;
    readonly pos: readonly [number, number, number];
    readonly at: number;
  },
): PartyState {
  const mark = { objId: m.objId, name: m.name, pos: m.pos, at: m.at };
  const i = prev.marks.findIndex((k) => k.objId === m.objId);
  if (i >= 0) return { ...prev, marks: prev.marks.map((k, n) => (n === i ? mark : k)) };
  if (prev.marks.length >= MAX_NAVI_MARKS) return prev;
  return { ...prev, marks: [...prev.marks, mark] };
}

/** The marks that are still drawn at `now` (`Time` counts 200 draws down). */
export function activeMarks(
  marks: readonly PartyState['marks'][number][],
  now: number,
): readonly PartyState['marks'][number][] {
  return marks.filter((k) => now - k.at < NAVI_MARK_MS);
}

// ---- the finder list (OnRefreshPartyList) ------------------------------------------------------------------------------------

/** One wire entry as the list shows it. */
export function finderEntry(e: {
  readonly partyId: number;
  readonly name: string;
  readonly memberCount: number;
  readonly level: number;
  readonly point: number;
  readonly leaderId: number;
  readonly leaderName: string;
  readonly memberList: string;
  readonly linkAttack: boolean;
  readonly fortuneCircle: boolean;
  readonly giftBox: boolean;
  readonly skillFull: boolean;
  readonly expUp: boolean;
  readonly skillUp: boolean;
}): FinderEntry {
  return {
    partyId: e.partyId,
    name: e.name,
    members: e.memberCount,
    level: e.level,
    point: e.point,
    leaderId: e.leaderId,
    leaderName: e.leaderName,
    memberList: e.memberList,
    linkAttack: e.linkAttack,
    fortuneCircle: e.fortuneCircle,
    giftBox: e.giftBox,
    skillFull: e.skillFull,
    expUp: e.expUp,
    skillUp: e.skillUp,
  };
}
