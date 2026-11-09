// The read model of the lord windows (Plan/45 §28.2, WP-CFEAT-43): `LordView` from the slice, the player-data cache, the tables bundle and the feature switches. Pure: the module gathers the facts.

import type { LordSkill } from '../../../gen/bundles';
import type {
  LordCandidateRow,
  LordRateRow,
  LordSkillRow,
  LordView,
} from '../../../ui/windows/lord/ports';
import type { LordSlice } from './slice';

/** What the view reads besides the slice. */
export interface LordViewFacts {
  /** `g_pPlayer->m_idPlayer`. */
  readonly selfId: number;
  /** The level and the job id of a character the cache knows; `undefined` for an unknown one (the placeholder has level 1). */
  player(id: number): { readonly level: number; readonly job: number } | undefined;
  /** `GetPlayerString( id )`, `''` while unknown. */
  nameOf(id: number): string;
  /** The name of a job id, `''` while the tables are missing. */
  jobName(job: number): string;
  /** `lordskill.txt` (the `tables` bundle). */
  readonly skills: readonly LordSkill[];
  /** `lordevent.inc` (the `tables` bundle). */
  readonly expRates: readonly LordRateRow[];
  readonly itemRates: readonly LordRateRow[];
  /** `features.select_lord`. */
  readonly selectLord: boolean;
  /** `features.player_lordevent`. */
  readonly playerLordEvent: boolean;
  /** `gameplay.max_lord_event`. */
  readonly maxLordEvent: number;
}

/** `IElection::GetVote()`: the sum of the votes. */
export function totalVotes(slice: LordSlice): number {
  return slice.election.candidates.reduce((n, c) => n + c.votes, 0);
}

/** `IElection::IsValid()`: the votes reach the requirement. */
export function requirementOk(slice: LordSlice): boolean {
  return totalVotes(slice) >= slice.election.requirement;
}

/** `GetBegin() + tCandidacy + tVote` (`clord.h:39`), 0 when the election is not scheduled. */
export function endVoteAt(slice: LordSlice): number {
  const e = slice.election;
  return e.beginAt === 0 ? 0 : e.beginAt + e.property.candidacyS + e.property.voteS;
}

/** The icon path of a lord skill: `icon/<file>`, lower case; `null` for none. */
export function skillIconPath(icon: string): string | null {
  return icon === '' ? null : `icon/${icon.toLowerCase()}`;
}

export function lordView(slice: LordSlice, f: LordViewFacts): LordView {
  const e = slice.election;
  const candidates: LordCandidateRow[] = e.candidates.map((c, i) => {
    const p = f.player(c.id);
    return {
      rank: i + 1,
      id: c.id,
      name: f.nameOf(c.id),
      level: p?.level ?? 1,
      job: p ? f.jobName(p.job) : '',
      deposit: c.deposit,
      votes: c.votes,
      pledge: c.pledge,
    };
  });
  const own = e.candidates.find((c) => c.id === f.selfId);
  const skills: LordSkillRow[] = f.skills.map((s) => ({
    id: s.id,
    name: s.name,
    desc: s.desc,
    icon: skillIconPath(s.icon),
    tick: slice.skillTicks[s.id] ?? 0,
    cooltime: s.cooltime,
  }));
  return {
    lordId: slice.lord,
    lordName: slice.lord === null ? '' : f.nameOf(slice.lord),
    state: e.state,
    requirementOk: requirementOk(slice),
    endVoteAt: endVoteAt(slice),
    candidates,
    selfId: f.selfId,
    ownDeposit: own?.deposit ?? 0,
    isLord: slice.lord !== null && slice.lord === f.selfId,
    selectLord: f.selectLord,
    playerLordEvent: f.playerLordEvent,
    maxLordEvent: f.maxLordEvent,
    eventCount: slice.events.length,
    ownEvent: slice.events.some((x) => x.creator === f.selfId),
    expRates: f.expRates,
    itemRates: f.itemRates,
    skills,
  };
}
