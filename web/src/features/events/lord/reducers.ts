// The reducers of the lord protocol (Plan/45 §28.2, WP-CFEAT-43; `src:Neuz/DPClient.cpp:17899-18016`, `Neuz/clord.cpp:54-325`, `_Common/lord.cpp:150-345, 425-660`, `_Common/lordskill.cpp:100-180`): one pure
// function per message, `reduce<Msg>( slice, msg, env ) -> { slice, effects }`. EVERY message is applied whether or not a window is open (PL-124 "list parsing without a window"): the lists land in
// `eventStore.lord`, a window that opens later reads them. What is done to the screen is data (`LordEffect`), carried out by the module: boxes, chat lines, captions, player-data queries,
// the closing of windows, the clean-up of the taskbar and the lord-skill effects.
//
// FIXes of the original that the reducers carry (each one a test): the deposit box prints the arguments in the order its text expects (raise, old total, new total: the original passed old, raise,
// total); the "you are one of the final 10" box of the vote start is shown to the candidates that are in the first ten (the original ran the test before the cut and showed it to rank 11 and 12 too);
// a lord skill used by ANOTHER lord never animates or turns the local player (PL-124: the original called `g_pPlayer->SetMotion` on every client in range).

import type {
  ElectionAddDepositNtf,
  ElectionBeginVote,
  ElectionEndVote,
  ElectionIncVoteNtf,
  ElectionSetPledgeNtf,
  LEventCreateNtf,
  LEventTick,
  Lord,
  LordSkillTick,
  LordSkillUseNtf,
} from '../../../gen/proto/flyff/v1/lord_pb';
import { APP_LORD_EVENT, APP_LORD_SKILL, APP_LORD_VOTE } from '../../../gen/defs/resdata';
import {
  TID_ADMIN_ANNOUNCE,
  TID_GAME_ELECTION_ADD_DEPOSIT_S001,
  TID_GAME_ELECTION_ADD_DEPOSIT_S002,
  TID_GAME_ELECTION_ADD_DEPOSIT_S003,
  TID_GAME_ELECTION_BEGIN_VOTE_S001,
  TID_GAME_ELECTION_CAPTION_BEGIN_CANDIDACY_01,
  TID_GAME_ELECTION_CAPTION_BEGIN_VOTE_01,
  TID_GAME_ELECTION_CAPTION_BEGIN_VOTE_02,
  TID_GAME_ELECTION_CAPTION_END_VOTE_01,
  TID_GAME_ELECTION_CAPTION_END_VOTE_02,
  TID_GAME_ELECTION_END_VOTE_S000,
  TID_GAME_ELECTION_END_VOTE_S001,
  TID_GAME_L_EVENT_CREATE_S001,
  TID_GAME_L_EVENT_CREATE_S002,
  TID_GAME_L_EVENT_CREATE_S003,
  TID_GAME_L_EVENT_CREATE_S004,
  TID_GAME_L_EVENT_CREATE_S005,
  TID_GAME_L_EVENT_CREATE_S006,
  TID_GAME_VOTE_COMPLETE,
} from '../../../gen/defs/text';
import { CHATSTY } from '../../../ui/chat/styles';
import {
  ELECTION_STATE,
  type Election,
  type ElectionState,
  initialElection,
  initialProperty,
  LORD_NULL_ID,
  LORD_SKILL_COUNT,
  type LordCandidate,
  type LordEventComponent,
  type LordSlice,
  MAX_CANDIDATES,
} from './slice';

/** What a reducer reads besides the slice (`g_pPlayer`, the player-data cache, `lordskill.txt`, the movers in view). */
export interface LordEnv {
  /** `ctx.now()` in ms. */
  readonly now: number;
  /** `g_pPlayer->m_idPlayer`. */
  readonly selfCharId: number;
  /** The object id of the own mover. */
  readonly selfObjId: number;
  /** `GetPlayerString( id )`: the name, `''` while unknown. */
  name(id: number): string;
  /** A lord skill of `lordskill.txt`, `null` for an id the table lacks (the original returned for a missing skill). */
  skill(id: number): LordSkillFacts | null;
  /** The mover with this object id is in view (`prj.GetMover( objid )`, `IsValidObj`). */
  hasObject(objId: number): boolean;
  /** The object id of the player mover with this CHARACTER id (`prj.GetUserByID`), the own mover included; `null` when none is in view. */
  moverOfCharacter(charId: number): number | null;
}

/** The columns of `lordskill.txt` the client reads. */
export interface LordSkillFacts {
  /** `nCooltime` in MINUTES. */
  readonly cooltime: number;
  /** `dwSrcSfx`, `NULL_ID` for none. */
  readonly srcSfx: number;
  /** `dwDstSfx`, `NULL_ID` for none. */
  readonly dstSfx: number;
}

export type LordEffect =
  /** `g_WndMng.OpenMessageBox( GetText( tid ) )` with the printf arguments. */
  | { readonly kind: 'box'; readonly tid: number; readonly args: readonly (string | number)[] }
  /** `g_WndMng.OpenMessageBoxUpper( ... )`. */
  | { readonly kind: 'boxUpper'; readonly tid: number; readonly args: readonly (string | number)[] }
  /**
   * `g_WndMng.PutString( GetText( tid ), NULL, GetTextColor( colorTid ?? tid ), style )`: the line in the colour of its own text and the default chat style; `colorTid` and `style` give
   * the system announcement of a lord event (`TID_ADMIN_ANNOUNCE`, `CHATSTY_SYSTEM`).
   */
  | {
      readonly kind: 'chat';
      readonly tid: number;
      readonly args: readonly (string | number)[];
      readonly colorTid?: number;
      readonly style?: number;
    }
  /** `g_Caption1.AddCaption( text, small ? m_pFontAPICaption : m_pFontAPITitle )`. */
  | {
      readonly kind: 'caption';
      readonly tid: number;
      readonly args: readonly (string | number)[];
      readonly small: boolean;
    }
  /** `GetPlayerData( id, TRUE )`: one query when the cache lacks the character. */
  | { readonly kind: 'need'; readonly id: number }
  | { readonly kind: 'needAll'; readonly ids: readonly number[] }
  /** `pWnd->Destroy()` of an open window of this `APP_*`. */
  | { readonly kind: 'close'; readonly app: number }
  /** `m_pWndLordTender->RefreshDeposit()`. */
  | { readonly kind: 'refreshTender' }
  /** `DoEndVoteComplete`: every `SHORTCUT_LORDSKILL` slot of the taskbar goes. */
  | { readonly kind: 'cleanShortcuts' }
  /** `CreateSfx( xi, mover.GetPos(), mover.GetId() )` on the mover with this object id. */
  | { readonly kind: 'sfx'; readonly xi: number; readonly objId: number }
  /** The motion of the casting lord (`MTI_ATK1`, `ANILOOP_1PLAY`, `MOP_SWDFORCE | MOP_NO_TRANS | MOP_FIXED`), after the turn toward `faceObjId` when it is not `null`. */
  | { readonly kind: 'localMotion'; readonly faceObjId: number | null };

export interface Reduced {
  readonly slice: LordSlice;
  readonly effects: readonly LordEffect[];
}

const same = (slice: LordSlice): Reduced => ({ slice, effects: [] });

const withElection = (
  slice: LordSlice,
  patch: Partial<Election>,
  effects: readonly LordEffect[] = [],
  more: Partial<LordSlice> = {},
): Reduced => ({
  slice: { ...slice, ...more, election: { ...slice.election, ...patch } },
  effects,
});

// ---- the sorts ---------------------------------------------------------------------------------------------------------------------

/** `DepositCompare`: the deposit descending, equal deposits by the creation time ascending. A stable sort (a complete tie keeps the order). */
export function sortByDeposit(list: readonly LordCandidate[]): LordCandidate[] {
  return list
    .map((c, i) => ({ c, i }))
    .sort((a, b) => b.c.deposit - a.c.deposit || a.c.createdAt - b.c.createdAt || a.i - b.i)
    .map(({ c }) => c);
}

/** `VoteCompare`: the votes descending, equal votes by the creation time ascending. */
export function sortByVotes(list: readonly LordCandidate[]): LordCandidate[] {
  return list
    .map((c, i) => ({ c, i }))
    .sort((a, b) => b.c.votes - a.c.votes || a.c.createdAt - b.c.createdAt || a.i - b.i)
    .map(({ c }) => c);
}

/** `sprintf( "%2.0f", x )`: rounded to an integer (ties to even, as the C library does), padded with blanks to two characters. */
export function printfF20(x: number): string {
  const floor = Math.floor(x);
  const diff = x - floor;
  const n = diff < 0.5 ? floor : diff > 0.5 ? floor + 1 : floor % 2 === 0 ? floor : floor + 1;
  return String(n).padStart(2, ' ');
}

/** The percentage of a factor as the lord-event texts print it: `factor * 100` of the f32 value. */
export function percentOf(factor: number): number {
  return Math.fround(factor) * 100;
}

// ---- Lord (the join batch) ----------------------------------------------------------------------------------------------------

/** `OnLord`: `CCLord::Serialize`. The whole state is replaced; `NULL_ID` is no lord. */
export function reduceLord(_slice: LordSlice, m: Lord, env: LordEnv): Reduced {
  const e = m.election;
  const p = e?.property;
  const candidates: LordCandidate[] = (e?.candidates ?? []).map((c) => ({
    id: c.idPlayer,
    deposit: Number(c.deposit),
    votes: c.votes,
    createdAt: Number(c.createdAt),
    pledge: c.pledge,
  }));
  const election: Election = e
    ? {
        id: e.electionId,
        state: Math.min(Math.max(e.state, 0), ELECTION_STATE.Expired) as ElectionState,
        beginAt: Number(e.beginAt),
        requirement: e.requirement,
        property: p
          ? {
              dayOfWeek: p.dayOfWeek,
              hour: p.hour,
              candidacyS: p.candidacyS,
              voteS: p.voteS,
              requirementFactor: p.requirementFactor,
              days: p.days,
              rates: [...p.depositReturnRates],
              items: p.items.map((l) => [...l.itemIds]),
            }
          : initialProperty(),
        candidates,
      }
    : initialElection();
  const events: LordEventComponent[] = m.events.map((c) => ({
    creator: c.idPlayer,
    tick: c.tick,
    expFactor: c.expFactor,
    itemFactor: c.itemFactor,
  }));
  const skillTicks = Array.from({ length: LORD_SKILL_COUNT }, () => 0);
  for (const t of m.skillTicks) {
    if (t.skill >= 0 && t.skill < LORD_SKILL_COUNT) skillTicks[t.skill] = t.tick;
  }
  const lord = m.idLord === LORD_NULL_ID ? null : m.idLord;
  const ids = [
    ...(lord !== null ? [lord] : []),
    ...candidates.map((c) => c.id),
    ...events.map((c) => c.creator),
  ];
  const unique = [...new Set(ids)];
  return {
    slice: { lord, election, events, skillTicks, skillTicksAt: env.now },
    effects: unique.length > 0 ? [{ kind: 'needAll', ids: unique }] : [],
  };
}

// ---- the election -----------------------------------------------------------------------------------------------------------------

/**
 * `OnElectionAddDeposit` -> `IElection::AddDeposit`: `deposit` is the RAISE of the message. The own deposit opens a box (`_S002` for a first bid, `_S001` for a raise) and prints the success
 * line; then the candidate is created or raised and the list sorted. The `createdAt` of a raise is ignored.
 */
export function reduceAddDeposit(
  slice: LordSlice,
  m: ElectionAddDepositNtf,
  env: LordEnv,
): Reduced {
  const add = Number(m.deposit);
  const existing = slice.election.candidates.find((c) => c.id === m.idPlayer);
  const old = existing?.deposit ?? 0;
  const effects: LordEffect[] = [];
  if (m.idPlayer === env.selfCharId) {
    // FIX of the argument order: the text is "You have bid %I64d more penya than previous %I64d penya, total %I64d penya have been bid."
    if (old !== 0) {
      effects.push({
        kind: 'boxUpper',
        tid: TID_GAME_ELECTION_ADD_DEPOSIT_S001,
        args: [add, old, old + add],
      });
    } else {
      effects.push({ kind: 'boxUpper', tid: TID_GAME_ELECTION_ADD_DEPOSIT_S002, args: [add] });
    }
    effects.push({ kind: 'chat', tid: TID_GAME_ELECTION_ADD_DEPOSIT_S003, args: [] });
  }
  const candidates = existing
    ? slice.election.candidates.map((c) =>
        c.id === m.idPlayer ? { ...c, deposit: c.deposit + add } : c,
      )
    : [
        ...slice.election.candidates,
        { id: m.idPlayer, deposit: add, votes: 0, createdAt: Number(m.createdAt), pledge: '' },
      ];
  effects.push({ kind: 'need', id: m.idPlayer }, { kind: 'refreshTender' });
  return withElection(slice, { candidates: sortByDeposit(candidates) }, effects);
}

/** `OnElectionSetPledge` -> `IElection::SetPledge`: an unknown candidate is ignored. */
export function reduceSetPledge(slice: LordSlice, m: ElectionSetPledgeNtf): Reduced {
  if (!slice.election.candidates.some((c) => c.id === m.idPlayer)) return same(slice);
  const candidates = slice.election.candidates.map((c) =>
    c.id === m.idPlayer ? { ...c, pledge: m.pledge } : c,
  );
  return withElection(slice, { candidates });
}

/** `OnElectionIncVote` -> `IElection::IncVote`: the vote window of the elector closes with a confirmation; the votes grow without a new sort. */
export function reduceIncVote(slice: LordSlice, m: ElectionIncVoteNtf, env: LordEnv): Reduced {
  const effects: LordEffect[] = [];
  if (m.idElector === env.selfCharId) {
    effects.push(
      { kind: 'close', app: APP_LORD_VOTE },
      { kind: 'box', tid: TID_GAME_VOTE_COMPLETE, args: [env.name(m.idCandidate)] },
    );
  }
  if (!slice.election.candidates.some((c) => c.id === m.idCandidate)) {
    return { slice, effects };
  }
  const candidates = slice.election.candidates.map((c) =>
    c.id === m.idCandidate ? { ...c, votes: c.votes + 1 } : c,
  );
  return withElection(slice, { candidates }, effects);
}

/** `OnElectionBeginCandidacy` -> `IElection::BeginCandidacy`. */
export function reduceBeginCandidacy(slice: LordSlice): Reduced {
  return withElection(slice, { state: ELECTION_STATE.Candidacy }, [
    {
      kind: 'caption',
      tid: TID_GAME_ELECTION_CAPTION_BEGIN_CANDIDACY_01,
      args: [],
      small: true,
    },
  ]);
}

/**
 * `OnElectionBeginVote` -> `IElection::BeginVote`: two captions, the list is cut to the first ten and the requirement stored. A candidate among those ten gets the box
 * (FIX: the original asked before the cut, so rank 11 and 12 got it too).
 */
export function reduceBeginVote(slice: LordSlice, m: ElectionBeginVote, env: LordEnv): Reduced {
  const kept = slice.election.candidates.slice(0, MAX_CANDIDATES);
  const effects: LordEffect[] = [
    { kind: 'caption', tid: TID_GAME_ELECTION_CAPTION_BEGIN_VOTE_01, args: [], small: true },
    { kind: 'caption', tid: TID_GAME_ELECTION_CAPTION_BEGIN_VOTE_02, args: [], small: true },
  ];
  if (kept.some((c) => c.id === env.selfCharId)) {
    effects.push({ kind: 'boxUpper', tid: TID_GAME_ELECTION_BEGIN_VOTE_S001, args: [] });
  }
  return withElection(
    slice,
    { state: ELECTION_STATE.Vote, candidates: kept, requirement: m.requirement },
    effects,
  );
}

/**
 * `OnElectionEndVote` -> `IElection::EndVote`: the candidates are sorted by votes, the captions and the chat line tell the result, the lord is set (`NULL_ID` = none), the skill cooldowns
 * reset and the next election prepared (state Ready, no candidates, the id one higher, the begin moved by `days`; the original computed a zero begin from the PC clock, the next `Lord`
 * message carries the server's value). A character that is not the lord loses the lord shortcuts and windows (`DoEndVoteComplete`).
 */
export function reduceEndVote(slice: LordSlice, m: ElectionEndVote, env: LordEnv): Reduced {
  const winner = m.idLord === LORD_NULL_ID ? null : m.idLord;
  const effects: LordEffect[] = [
    { kind: 'caption', tid: TID_GAME_ELECTION_CAPTION_END_VOTE_01, args: [], small: true },
  ];
  if (winner !== null) {
    const name = env.name(winner);
    effects.push(
      { kind: 'need', id: winner },
      { kind: 'caption', tid: TID_GAME_ELECTION_CAPTION_END_VOTE_02, args: [name], small: true },
      { kind: 'chat', tid: TID_GAME_ELECTION_END_VOTE_S000, args: [name] },
    );
  } else {
    effects.push({ kind: 'chat', tid: TID_GAME_ELECTION_END_VOTE_S001, args: [] });
  }
  if (winner !== env.selfCharId) {
    effects.push(
      { kind: 'cleanShortcuts' },
      { kind: 'close', app: APP_LORD_SKILL },
      { kind: 'close', app: APP_LORD_EVENT },
    );
  }
  const e = slice.election;
  return {
    slice: {
      ...slice,
      lord: winner,
      skillTicks: Array.from({ length: LORD_SKILL_COUNT }, () => 0),
      election: {
        ...e,
        state: ELECTION_STATE.Ready,
        candidates: [],
        beginAt: e.beginAt !== 0 ? e.beginAt + e.property.days * 86_400 : 0,
        id: e.id + 1,
      },
    },
    effects,
  };
}

// ---- the lord events ---------------------------------------------------------------------------------------------------------------

/**
 * `OnLEventCreate` -> `ILordEvent::AddComponent`: the event is appended; a big caption and a system chat line announce it (`__LORDNAME_FIX`: the name is the CREATOR's, V15 printed the lord's;
 * each factor times 100 is printed as `%2.0f`).
 */
export function reduceEventCreate(slice: LordSlice, m: LEventCreateNtf, env: LordEnv): Reduced {
  const c = m.component;
  if (!c) return same(slice);
  const comp: LordEventComponent = {
    creator: c.idPlayer,
    tick: c.tick,
    expFactor: c.expFactor,
    itemFactor: c.itemFactor,
  };
  const name = env.name(c.idPlayer);
  const e = printfF20(percentOf(c.expFactor));
  const i = printfF20(percentOf(c.itemFactor));
  let captionTid: number;
  let lineTid: number;
  let args: readonly string[];
  if (c.expFactor > 0 && c.itemFactor > 0) {
    captionTid = TID_GAME_L_EVENT_CREATE_S001;
    lineTid = TID_GAME_L_EVENT_CREATE_S004;
    args = [name, e, i];
  } else if (c.expFactor > 0) {
    captionTid = TID_GAME_L_EVENT_CREATE_S002;
    lineTid = TID_GAME_L_EVENT_CREATE_S005;
    args = [name, e];
  } else if (c.itemFactor > 0) {
    captionTid = TID_GAME_L_EVENT_CREATE_S003;
    lineTid = TID_GAME_L_EVENT_CREATE_S006;
    args = [name, i];
  } else {
    // neither factor: the original printed the buffer it had not filled; the server never creates such an event
    return {
      slice: { ...slice, events: [...slice.events, comp] },
      effects: [{ kind: 'need', id: c.idPlayer }],
    };
  }
  return {
    slice: { ...slice, events: [...slice.events, comp] },
    effects: [
      { kind: 'need', id: c.idPlayer },
      { kind: 'caption', tid: captionTid, args: [], small: false },
      { kind: 'chat', tid: lineTid, args, colorTid: TID_ADMIN_ANNOUNCE, style: CHATSTY.SYSTEM },
    ],
  };
}

/** `OnLEventInitialize` -> `ILordEvent::Initialize`: every event is gone. */
export function reduceEventInitialize(slice: LordSlice): Reduced {
  return slice.events.length === 0 ? same(slice) : same({ ...slice, events: [] });
}

/** `OnLEventTick` -> `SerializeTick` + `EraseExpiredComponents`: the listed creators get their minutes, the events at 0 are removed. */
export function reduceEventTick(slice: LordSlice, m: LEventTick): Reduced {
  const ticks = new Map<number, number>();
  for (const t of m.ticks) ticks.set(t.idPlayer, t.tick);
  const events = slice.events
    .map((e) => (ticks.has(e.creator) ? { ...e, tick: ticks.get(e.creator) ?? e.tick } : e))
    .filter((e) => e.tick !== 0);
  return same({ ...slice, events });
}

// ---- the lord skills --------------------------------------------------------------------------------------------------------------

/** `OnLordSkillTick` -> `CLordSkill::SerializeTick`: the listed skills get their minutes. */
export function reduceSkillTick(slice: LordSlice, m: LordSkillTick, env: LordEnv): Reduced {
  const skillTicks = [...slice.skillTicks];
  for (const t of m.ticks) {
    if (t.skill >= 0 && t.skill < skillTicks.length) skillTicks[t.skill] = t.tick;
  }
  return same({ ...slice, skillTicks, skillTicksAt: env.now });
}

/**
 * `OnLordSkillUse( objid )`: the header object is the lord. A skill the table lacks does nothing. `Execute` starts the cooldown (`Use`) on every client. The destination effect goes on the
 * mover whose character id is `target`; the source effect on the lord when he is in view - and only then, and only when the lord is the LOCAL player, the swing motion and the turn toward the
 * target (PL-124 FIX). A skill without a source effect (SHOUT, PARTY) animates nobody.
 */
export function reduceSkillUse(
  slice: LordSlice,
  m: LordSkillUseNtf,
  env: LordEnv,
  header: number,
): Reduced {
  const skill = env.skill(m.skill);
  if (!skill) return same(slice);
  const skillTicks = [...slice.skillTicks];
  if (m.skill >= 0 && m.skill < skillTicks.length) skillTicks[m.skill] = skill.cooltime;
  const effects: LordEffect[] = [];
  if (skill.srcSfx !== LORD_NULL_ID && env.hasObject(header)) {
    if (header === env.selfObjId) {
      const target = env.moverOfCharacter(m.target);
      effects.push({ kind: 'localMotion', faceObjId: target });
    }
    effects.push({ kind: 'sfx', xi: skill.srcSfx, objId: header });
  }
  if (skill.dstSfx !== LORD_NULL_ID) {
    const target = env.moverOfCharacter(m.target);
    if (target !== null) effects.push({ kind: 'sfx', xi: skill.dstSfx, objId: target });
  }
  return { slice: { ...slice, skillTicks, skillTicksAt: env.now }, effects };
}
