// The reducers of `selfStore` (Plan/40 section 6.2.2, ST3; WP-CLIENT-29): pure functions from the protocol messages of the
// own character to the next `SelfState`. High-frequency values (HP, MP, FP, EXP) are NOT part of the store state while a
// drain runs (ST5): the handlers write them into the {@link SelfLive} record, the publishers of `game/world/self.ts` copy
// them at 10 Hz. Everything else is written by the handlers at once, one `setState` per message.
//
// Source of the semantics: ref/09a section 5.3 and 5.6 (`OnSetPointParam`, `OnSetExperience`, `OnSetStatLevel`,
// `OnSetGrowthLearningPoint`, `OnSetChangeJob`, `OnSetFxp`), ref/09b section 3.4 (`SetState`, `SetFame`, `ModifyMode`,
// `SmMode`) and Plan/16 section 17 (the message catalogue: which messages carry absolute values).

import type {
  JobChanged,
  ModifyMode,
  SetExperience,
  SetFame,
  SetFxp,
  SetGrowthLearningPoint,
  SetState,
  SetStatLevel,
  SmMode,
  SmModeAll,
} from '../gen/proto/flyff/v1/character_pb';
import type { PkRelation } from '../gen/proto/flyff/v1/pvp_pb';
import type { SpawnSelf } from '../gen/proto/flyff/v1/world_pb';
import { n64 } from '../net/int64';
import type { SelfState } from './self';

/** `OBJSTAF_FLY` of the state-flag word (`flyff_sim::types`). */
const OBJSTAF_FLY = 0x08;

/** `DST_*` ids of the point pools and the purse (`defineAttribute.h`), the numbers `SetPointParam` carries. */
export const POINT_DST = { HP: 38, MP: 39, FP: 40, GOLD: 10000 } as const;

/** `SET_STAT_LEVEL.chID`: 100 STR, 101 STA, 102 DEX, 103 INT. */
export const STAT_ID = { STR: 100, STA: 101, DEX: 102, INT: 103 } as const;

/** The live values of the own character; written by the handlers, published at 10 Hz (ST5). */
export interface SelfLive {
  hp: number;
  mp: number;
  fp: number;
  exp: number;
  /** A value changed since the last publish. */
  dirty: boolean;
  /** The own mover id (0 = none). */
  objId: number;
}

export function newSelfLive(): SelfLive {
  return { hp: 0, mp: 0, fp: 0, exp: 0, dirty: false, objId: 0 };
}

/** The `SelfState` a `SpawnSelf` describes; fields the message cannot know keep the reset value. */
export function applySpawnSelf(m: SpawnSelf, objId: number, worldId: number): SelfState | null {
  const p = m.player;
  if (!p || !m.obj || !m.mover) return null;
  const s = m.self;
  const pos = m.obj.pos;
  const smTimers: { kind: number; endUnix: number }[] = [];
  p.smTime.forEach((end, kind) => {
    const endUnix = n64(end);
    if (endUnix > 0) smTimers.push({ kind, endUnix });
  });
  return {
    objId,
    idPlayer: p.idPlayer,
    name: p.name,
    sex: p.sex === 1 ? 1 : 0,
    job: p.job,
    level: p.level,
    exp: s ? n64(s.exp) : 0,
    // the percentage and the maxima come from the rules context (`derived_stats`, `exp_percent_x100`): 0 until it ran
    expPercentX100: 0,
    deathExp: s ? n64(s.deathExp) : 0,
    deathLevel: s?.deathLevel ?? 0,
    hp: m.mover.hp,
    mp: s?.mp ?? 0,
    fp: s?.fp ?? 0,
    maxHp: 0,
    maxMp: 0,
    maxFp: 0,
    base: { str: p.str, sta: p.sta, dex: p.dex, int: p.int },
    statPoints: s?.remainGp ?? 0,
    skillPoints: s?.skillPoint ?? 0,
    gold: s?.gold ?? 0,
    authority: p.authorization,
    modeFlags: p.mode,
    worldId,
    pos: [pos?.x ?? 0, pos?.y ?? 0, pos?.z ?? 0],
    isFlying: (m.mover.stateFlag & OBJSTAF_FLY) !== 0,
    guildId: p.idGuild,
    partyId: p.idParty,
    fame: p.fame,
    smTimers,
    pk: { pink: p.pkTimeMs > 0, propensity: p.pkPropensity, value: p.pkValue },
    derived: null,
    fxp: s?.fxp ?? 0,
    flightLevel: 1,
    dead: m.mover.hp <= 0,
  };
}

/** The live record after a `SpawnSelf` (`state` is the result of {@link applySpawnSelf}). */
export function liveFromSpawn(live: SelfLive, state: SelfState): void {
  live.objId = state.objId;
  live.hp = state.hp;
  live.mp = state.mp;
  live.fp = state.fp;
  live.exp = state.exp;
  live.dirty = false;
}

/**
 * `SetPointParam{dst, value}` of the own character: HP, MP and FP are absolute values written into the live record
 * (`OnSetPointParam`, ref/09a 5.3; the server sends them after heals, regeneration, revival, level-up and restat, PL-083),
 * the purse is a store field (`DST_GOLD`, 11a). Returns the store patch (`null`: the live record or nothing changed).
 * A positive HP ends the `dead` state.
 */
export function applySetPointParam(
  live: SelfLive,
  state: Pick<SelfState, 'dead'>,
  dst: number,
  value: number,
): Partial<SelfState> | null {
  switch (dst) {
    case POINT_DST.HP:
      live.hp = value;
      live.dirty = true;
      return value > 0 && state.dead ? { dead: false } : null;
    case POINT_DST.MP:
      live.mp = value;
      live.dirty = true;
      return null;
    case POINT_DST.FP:
      live.fp = value;
      live.dirty = true;
      return null;
    case POINT_DST.GOLD:
      return { gold: value };
    default:
      return null;
  }
}

/**
 * `SetExperience`: the real values always (the legacy -1 / 0xFFFF sentinels are not produced). EXP goes into the live
 * record, the rest is a store patch (level, skill points, the death penalty memory).
 */
export function applySetExperience(live: SelfLive, m: SetExperience): Partial<SelfState> {
  live.exp = n64(m.exp);
  live.dirty = true;
  return {
    level: m.level,
    skillPoints: m.skillPoint,
    deathExp: n64(m.deathExp),
    deathLevel: m.deathLevel,
  };
}

/** `SetLevel` of the own mover (the server sends it to the others; an own copy is harmless). */
export function applySetLevel(level: number): Partial<SelfState> {
  return { level };
}

/** `SetStatLevel{stat_id, value, remain_gp}`: one stat point was spent. */
export function applySetStatLevel(state: SelfState, m: SetStatLevel): Partial<SelfState> {
  const base = { ...state.base };
  switch (m.statId) {
    case STAT_ID.STR:
      base.str = m.value;
      break;
    case STAT_ID.STA:
      base.sta = m.value;
      break;
    case STAT_ID.DEX:
      base.dex = m.value;
      break;
    case STAT_ID.INT:
      base.int = m.value;
      break;
    default:
      return { statPoints: m.remainGp };
  }
  return { base, statPoints: m.remainGp };
}

/** `SetState{str, sta, dex, int, remain_gp}`: the full stat refresh (the pools follow as `SetPointParam`, PL-083). */
export function applySetState(m: SetState): Partial<SelfState> {
  return {
    base: { str: m.str, sta: m.sta, dex: m.dex, int: m.int },
    statPoints: m.remainGp,
  };
}

/** `SetGrowthLearningPoint{remain_gp}`: the remaining stat points (LP is ignored, `OnSetGrowthLearningPoint`). */
export function applySetGrowthLearningPoint(m: SetGrowthLearningPoint): Partial<SelfState> {
  return { statPoints: m.remainGp };
}

/** `JobChanged{job, skills}`: the job; the skill list belongs to the skill tree (M5). */
export function applyJobChanged(m: JobChanged): Partial<SelfState> {
  return { job: m.job };
}

/** `ModifyMode{mode}`: the whole mode word. */
export function applyModifyMode(m: ModifyMode): Partial<SelfState> {
  return { modeFlags: m.mode };
}

/** `SetFame{fame}`. */
export function applySetFame(m: SetFame): Partial<SelfState> {
  return { fame: m.fame };
}

/** `SetFxp{fxp, flight_level}`. */
export function applySetFxp(m: SetFxp): Partial<SelfState> {
  return { fxp: m.fxp, flightLevel: m.flightLevel };
}

/** `PkRelation`: pink flag and propensity (near, including self), the PK value (self). */
export function applyPkRelation(state: SelfState, m: PkRelation): Partial<SelfState> | null {
  const sub = m.sub;
  switch (sub.case) {
    case 'pink':
      return { pk: { ...state.pk, pink: sub.value.pink !== 0 } };
    case 'propensity':
      return { pk: { ...state.pk, propensity: sub.value.propensity } };
    case 'pkValue':
      return { pk: { ...state.pk, value: sub.value.pkValue } };
    case undefined:
      return null;
  }
}

/** `SmMode{sm_type, value}`: one premium timer (an absolute expiry in unix seconds or a raw counter; 0 = off). */
export function applySmMode(state: SelfState, m: SmMode): Partial<SelfState> {
  const value = n64(m.value);
  const rest = state.smTimers.filter((t) => t.kind !== m.smType);
  if (value > 0) rest.push({ kind: m.smType, endUnix: value });
  rest.sort((a, b) => a.kind - b.kind);
  return { smTimers: rest };
}

/** `SmModeAll{entries}`: every active timer, in SM index order (the join batch and the world change). */
export function applySmModeAll(m: SmModeAll): Partial<SelfState> {
  const smTimers: { kind: number; endUnix: number }[] = [];
  for (const e of m.entries) {
    const endUnix = n64(e.value);
    if (endUnix > 0) smTimers.push({ kind: e.smType, endUnix });
  }
  smTimers.sort((a, b) => a.kind - b.kind);
  return { smTimers };
}
