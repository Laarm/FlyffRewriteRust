// The reducers of `skillTreeStore` (Plan/45 §19.1; `CDPClient::OnSetChangeJob` :3682, `OnCmdSetSkillLevel` :13606,
// `OnInitSkillPoint` :14768, `OnDoUseSkillPoint` :14834, `OnSetExperience` :3435): pure functions from the messages of the own
// character to the next `SkillTreeState`. Every server message that changes the table or the points calls `InitItem` of an open skill
// tree window, i.e. the local copy is dropped and the window shows the server state (`reload`).

import { JTYPE_HERO, JTYPE_MASTER } from '../../../gen/defs/job';
import type { Skill } from '../../../gen/proto/flyff/v1/mover_types_pb';
import {
  initState,
  NULL_ID,
  normalizeTable,
  type SkillRule,
  type SkillSlot,
  type SkillTable,
} from './model';
import type { SkillTreeState } from './store';

/** The slots of a wire list (`Skill{skill_id, level}`): padded or cut to 45, an empty entry is `NULL_ID / 0`. */
export function tableFromWire(skills: readonly Pick<Skill, 'skillId' | 'level'>[]): SkillSlot[] {
  return normalizeTable(skills.map((s) => ({ skillId: s.skillId >>> 0, level: s.level >>> 0 })));
}

/** The server table / points changed: the local copy is dropped (`InitItem`), nothing is selected. */
function reloaded(
  s: SkillTreeState,
  server: SkillTable,
  sp: number,
  extra: Partial<SkillTreeState> = {},
): SkillTreeState {
  return {
    ...s,
    ...extra,
    tree: initState(server, sp),
    serverVersion: s.serverVersion + 1,
  };
}

/** `SpawnSelf`: the table of the join (`PlayerSelf.job_skills`) and the points (`PlayerSelf.skill_point`). */
export function applySpawn(
  s: SkillTreeState,
  skills: readonly Pick<Skill, 'skillId' | 'level'>[],
  skillPoint: number,
): SkillTreeState {
  return reloaded(s, tableFromWire(skills), skillPoint, { ready: true, busy: false });
}

/**
 * `JobChanged` of the own character: the 45 slots replace the table (the skill points stay; `InitLevel` already sent them with the
 * `SetExperience` that precedes it).
 */
export function applyJobChanged(
  s: SkillTreeState,
  skills: readonly Pick<Skill, 'skillId' | 'level'>[],
): SkillTreeState {
  return reloaded(s, tableFromWire(skills), s.tree.serverSp, { ready: true, busy: false });
}

/** `DoUseSkillPointNtf{skills, skill_point}`: the answer of the finish (and of a master growth or a legend skill-up). */
export function applyDoUseSkillPoint(
  s: SkillTreeState,
  skills: readonly Pick<Skill, 'skillId' | 'level'>[],
  skillPoint: number,
): SkillTreeState {
  // `OnDoUseSkillPoint` writes the level of every skill it finds in the own table (a skill the table lacks is skipped); a client
  // that has no table yet takes the one of the message
  const own = s.tree.server;
  const known = own.some((slot) => slot.skillId !== NULL_ID);
  const next = known
    ? own.map((slot) => {
        if (slot.skillId === NULL_ID) return slot;
        const hit = skills.find((w) => w.skillId >>> 0 === slot.skillId);
        return hit ? { skillId: slot.skillId, level: hit.level >>> 0 } : slot;
      })
    : tableFromWire(skills);
  return reloaded(s, next, skillPoint, { ready: true, busy: false });
}

/**
 * `InitSkillPoint{skill_point}`: every skill that is not a master or hero skill goes back to level 0 (the original stops at a skill
 * without a prop; the data has none) and the points are the new total.
 */
export function applyInitSkillPoint(
  s: SkillTreeState,
  skillPoint: number,
  rule: (id: number) => SkillRule | undefined,
): SkillTreeState {
  const next = s.tree.server.map((slot) => {
    if (slot.skillId === NULL_ID || slot.level === 0) return slot;
    const kind = rule(slot.skillId)?.kind1;
    return kind === JTYPE_MASTER || kind === JTYPE_HERO
      ? slot
      : { skillId: slot.skillId, level: 0 };
  });
  return reloaded(s, next, skillPoint, { busy: false });
}

/** `CmdSetSkillLevel{skill, level}`: a script or GM set one level (an unknown skill changes nothing). */
export function applyCmdSetSkillLevel(
  s: SkillTreeState,
  skillId: number,
  level: number,
): SkillTreeState | null {
  const at = s.tree.server.findIndex((x) => x.skillId === skillId);
  if (at < 0) return null;
  const next = s.tree.server.slice();
  next[at] = { skillId, level };
  return reloaded(s, next, s.tree.serverSp);
}

/**
 * `SetExperience.skill_point`: when the number of points differs from the one the window knows, `OnSetExperience` re-initialises the
 * open skill tree (`nSP != nSkillPoint`); the table is the same.
 */
export function applySetExperience(s: SkillTreeState, skillPoint: number): SkillTreeState {
  if (skillPoint === s.tree.serverSp) return s;
  return reloaded(s, s.tree.server, skillPoint);
}
