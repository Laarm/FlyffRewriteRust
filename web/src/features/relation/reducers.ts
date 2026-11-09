// The reducers of the relation messages (Plan/45 §24.2, §24.3, §19.4; ref/09b §3.16, §6.1, §6.2, §7.12; WP-CFEAT-40): one pure function per legacy handler of `CDPClient` -
// `CCoupleHelper::OnCouple ... OnAddCoupleExperience` (`src:Neuz/couplehelper.cpp`), `CCampusHelper::OnUpdateCampus ...` (`CampusHelper.cpp`), `OnHonorListAck`, `OnHonorChangeAck`, `OnSetCheerParam`
// (`DPClient.cpp:15790-15815, 17370-17410`) and the title/campus part of `SpawnSelf` (`ObjSerializeOpt.cpp:480-500, 612-640`). Each takes the relation mirror and the plain message and returns the new
// mirror plus the effects the module carries out (lines, cache queries, windows, the title above a name). No store, no window, no clock: the tests are tables of messages and expected effects.

import {
  TID_GAME_COUPLE_LEVELUP,
  TID_GAME_COUPLE_S02,
  TID_GAME_COUPLE_S03,
  TID_GAME_GET_TITLE,
  TID_CHEER_MESSAGE5,
} from '../../gen/defs/text';
import { joinRestMs } from './cheer';
import { COUPLE_MAX_LEVEL, coupleLevel } from './coupleLevel';
import {
  applyHonorList,
  earnedFromCounters,
  type HonorTable,
  MAX_HONOR_TITLE,
} from './honorTitles';
import type { CampusMemberRecord, CampusRecord, CoupleRecord, RelationState } from './store';

export type RelationEffect =
  /** `g_WndMng.PutString( text( tid, args ), NULL, textColor( tid ) )`. */
  | { readonly kind: 'line'; readonly tid: number; readonly args: readonly (string | number)[] }
  /** `GetPlayerData( id, TRUE )`; `name` primes the placeholder when the message carried the name. */
  | { readonly kind: 'need'; readonly id: number; readonly name?: string }
  /** The list form (`QueryPlayerData2`). */
  | { readonly kind: 'needAll'; readonly ids: readonly number[] }
  /** `CM_RECEIVEPROPOSE`: the couple message window with `TID_GAME_RECEIVEPROPOSAL( name )`. */
  | { readonly kind: 'proposal'; readonly name: string }
  /** `CWndCampusInvitationConfirm( requester, name )`. */
  | { readonly kind: 'campusInvite'; readonly requester: number; readonly name: string }
  /** `CWndCampus::UpdatePlayerList`. */
  | { readonly kind: 'refreshCampus' }
  /** `pMover->SetTitle( GetTitle( id ) )` on the mover with that object id (`objId` 0 = the own character). */
  | { readonly kind: 'title'; readonly objId: number; readonly id: number };

export interface Reduced {
  readonly state: RelationState;
  readonly effects: readonly RelationEffect[];
}

/** What the reducers read of the data bundles. */
export interface RelationTables {
  /** `tables.couple.level_exp`. */
  readonly levelExp: readonly number[];
  /** The `honor` bundle. */
  readonly honor: HonorTable;
}

const same = (state: RelationState): Reduced => ({ state, effects: [] });

/** The partner of a couple: whichever of the two ids is not mine (`CCouple::GetPartner`). */
export const partnerOf = (couple: CoupleRecord, self: number): number =>
  couple.first === self ? couple.second : couple.first;

/** Whether a character is a member of the campus (`CCampus::IsMember`). */
export const isCampusMember = (campus: CampusRecord | null, id: number): boolean =>
  campus?.members.some((m) => m.id === id) ?? false;

/** `CCampus::IsMaster`: the master of the record. */
export const isCampusMaster = (campus: CampusRecord | null, id: number): boolean =>
  campus !== null && campus.master === id;

/** `CCampus::IsPupil`: a member with the pupil level (2). */
export const isCampusPupil = (campus: CampusRecord | null, id: number): boolean =>
  campus?.members.some((m) => m.id === id && m.level === 2) ?? false;

// ---- the join ------------------------------------------------------------------------------------------------------------------------

export interface SpawnSelfIn {
  /** `PlayerSelf.honorProgress`. */
  readonly honorProgress: readonly number[];
  /** `PlayerPublic.honor`: the selected title, `-1` none. */
  readonly honor: number;
  readonly campusPoint: number;
  readonly cheerPoint: number;
  readonly cheerRestMs: number;
}

/**
 * `SpawnSelf` (the join of every world): the earned list is rebuilt SILENTLY (`InitEarned` + `AddEarned` for every title that reaches its need, no chat line), the selected title, the campus
 * points and the cheer points are taken from it; the couple and the campus are cleared - the join batch sends `CoupleNtf` and `CampusUpdate` after it for a character that has them.
 * The title above the own name is set from `honor` (`m_nHonor != nTemp` -> `SetTitle`).
 */
export function onSpawnSelf(m: SpawnSelfIn, t: RelationTables, now: number): Reduced {
  const counters = m.honorProgress.length === MAX_HONOR_TITLE ? [...m.honorProgress] : [];
  return {
    state: {
      couple: null,
      campus: null,
      campusPoints: m.campusPoint,
      honor: { counters, earned: earnedFromCounters(t.honor, counters), selected: m.honor },
      cheer: { points: m.cheerPoint, restUntil: now + joinRestMs(m.cheerPoint, m.cheerRestMs) },
    },
    effects: [{ kind: 'title', objId: 0, id: m.honor }],
  };
}

// ---- couples -----------------------------------------------------------------------------------------------------------------------------

/** `CCoupleHelper::OnCouple`: the record is REPLACED (`SAFE_DELETE` + `new`), the level is computed from the EXP, the partner enters the cache. */
export function onCoupleNtf(
  s: RelationState,
  m: { readonly exp: number; readonly first: number; readonly second: number },
  self: number,
  t: RelationTables,
): Reduced {
  const couple: CoupleRecord = {
    exp: m.exp,
    first: m.first,
    second: m.second,
    level: coupleLevel(t.levelExp, m.exp),
  };
  return {
    state: { ...s, couple },
    effects: [{ kind: 'need', id: partnerOf(couple, self) }],
  };
}

/** `CCoupleHelper::OnProposeResult`: the proposer enters the cache, the proposal dialog opens (a second one replaces the first). */
export function onProposeResult(
  s: RelationState,
  m: { readonly proposer: number; readonly name: string },
): Reduced {
  return {
    state: s,
    effects: [
      { kind: 'need', id: m.proposer, name: m.name },
      { kind: 'proposal', name: m.name },
    ],
  };
}

/** `CCoupleHelper::OnCoupleResult`: a new couple (EXP 0, level 1) and the line `TID_GAME_COUPLE_S02( name )`. */
export function onCoupleResult(
  s: RelationState,
  m: { readonly partner: number; readonly name: string },
  self: number,
): Reduced {
  const couple: CoupleRecord = { exp: 0, first: self, second: m.partner, level: 1 };
  return {
    state: { ...s, couple },
    effects: [
      { kind: 'need', id: m.partner, name: m.name },
      { kind: 'line', tid: TID_GAME_COUPLE_S02, args: [m.name] },
    ],
  };
}

/** `CCoupleHelper::OnDecoupleResult`: only a coupled player is told (`TID_GAME_COUPLE_S03`). The selected honor title stays (KQ-39 f). */
export function onDecoupleResult(s: RelationState): Reduced {
  if (!s.couple) return same(s);
  return {
    state: { ...s, couple: null },
    effects: [{ kind: 'line', tid: TID_GAME_COUPLE_S03, args: [] }],
  };
}

/**
 * `CCoupleHelper::OnAddCoupleExperience`: ignored without a couple and at the maximum level (`CCouple::AddExperience`), otherwise the EXP grows and a changed level prints
 * `TID_GAME_COUPLE_LEVELUP( level )`.
 */
export function onAddCoupleExperience(s: RelationState, exp: number, t: RelationTables): Reduced {
  const c = s.couple;
  if (!c || c.level === COUPLE_MAX_LEVEL) return same(s);
  const total = c.exp + exp;
  const level = coupleLevel(t.levelExp, total);
  const couple: CoupleRecord = { ...c, exp: total, level };
  return {
    state: { ...s, couple },
    effects:
      level !== c.level ? [{ kind: 'line', tid: TID_GAME_COUPLE_LEVELUP, args: [level] }] : [],
  };
}

// ---- campus ---------------------------------------------------------------------------------------------------------------------------------

/** `OnCampusInvite`: the requester enters the cache and the confirmation window opens (a new invitation replaces the open one). */
export function onCampusInvite(
  s: RelationState,
  m: { readonly requester: number; readonly name: string },
): Reduced {
  return {
    state: s,
    effects: [
      { kind: 'need', id: m.requester, name: m.name },
      { kind: 'campusInvite', requester: m.requester, name: m.name },
    ],
  };
}

/** `CCampusHelper::OnUpdateCampus`: the campus is REPLACED, every member enters the cache, the campus page reads again. */
export function onCampusUpdate(
  s: RelationState,
  m: {
    readonly id: number;
    readonly master: number;
    readonly members: readonly CampusMemberRecord[];
  },
): Reduced {
  const campus: CampusRecord = {
    id: m.id,
    master: m.master,
    members: m.members.map((x) => ({ id: x.id, level: x.level })),
  };
  return {
    state: { ...s, campus },
    effects: [{ kind: 'needAll', ids: campus.members.map((x) => x.id) }, { kind: 'refreshCampus' }],
  };
}

/** `CCampusHelper::OnRemoveCampus`: the campus goes only when it is the current one. FIX: the state is null-safe (the original left a NULL campus that later callers dereferenced). */
export function onCampusRemove(s: RelationState, idCampus: number): Reduced {
  if (s.campus?.id !== idCampus) return same(s);
  return { state: { ...s, campus: null }, effects: [{ kind: 'refreshCampus' }] };
}

/** `CCampusHelper::OnUpdateCampusPoint`. */
export function onCampusUpdatePoint(s: RelationState, point: number): Reduced {
  return {
    state: { ...s, campusPoints: point },
    effects: [{ kind: 'refreshCampus' }],
  };
}

// ---- honor titles ---------------------------------------------------------------------------------------------------------------------------

/**
 * `CDPClient::OnHonorListAck`: a title that reaches its need and was not earned before is appended and announced (`TID_GAME_GET_TITLE( title )`), one that lost its requirement leaves
 * the list; FIX: a list that is not exactly 150 long is ignored.
 */
export function onHonorListAck(
  s: RelationState,
  counters: readonly number[],
  t: RelationTables,
): Reduced {
  if (counters.length !== MAX_HONOR_TITLE) return same(s);
  const r = applyHonorList(t.honor, counters, s.honor.earned);
  return {
    state: { ...s, honor: { ...s.honor, counters: [...counters], earned: r.earned } },
    effects: r.gained.map((g): RelationEffect => ({
      kind: 'line',
      tid: TID_GAME_GET_TITLE,
      args: [g.title],
    })),
  };
}

/**
 * `CDPClient::OnHonorChangeAck`: the title above the mover is set EVERY time (KQ-39 f: the server answers each change and each notifying recheck, even an unchanged one); for the own
 * character the selected id is stored too. No list refresh.
 */
export function onHonorChangeAck(
  s: RelationState,
  m: { readonly id: number; readonly objId: number },
  isSelf: boolean,
): Reduced {
  const effects: RelationEffect[] = [{ kind: 'title', objId: m.objId, id: m.id }];
  if (!isSelf || s.honor.selected === m.id) return { state: s, effects };
  return { state: { ...s, honor: { ...s.honor, selected: m.id } }, effects };
}

// ---- cheer ----------------------------------------------------------------------------------------------------------------------------------------

/** `CDPClient::OnSetCheerParam`: the points and when the next one comes; a gained point prints `TID_CHEER_MESSAGE5`. */
export function onSetCheerParam(
  s: RelationState,
  m: { readonly points: number; readonly restMs: number; readonly add: boolean },
  now: number,
): Reduced {
  return {
    state: { ...s, cheer: { points: m.points, restUntil: now + m.restMs } },
    effects: m.add ? [{ kind: 'line', tid: TID_CHEER_MESSAGE5, args: [] }] : [],
  };
}
