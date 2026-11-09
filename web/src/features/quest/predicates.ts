// The display ports of the quest condition predicates (Plan/20 §9.4-§9.5 rows B0-B15, N*, E1-E16; `Mover.cpp:9550-10678`; WP-CFEAT-42): which NPC marks, dialog colours and condition
// lines the client shows. Display only - the server decides every real accept and every completion. They are transcriptions of `crates/flyff-game/src/quest/predicates.rs`
// (quest-core, WP-NPC-13), over `QuestClientDef` instead of the server's `QuestProp`, and `predicates.test.ts` replays its three goldens (`tests/golden/quest_predicates_{begin,next_level,end}.json`)
// through them with both values of `fidelity.quest_condition_fixes` (PL-159: the client display follows the switch).
//
// Every row is the `if` chain of the original ported literally, so an out-of-range mode, comparison, type or operator fails its row just as the original's count fell short. B0a (the free
// slots for the start items) keeps its pass/fail and drops its message. `canEnd` is pure: the freeze of a running timer (a server side effect) is not ported.

import type { QuestClientDef, QuestItemCond, QuestPartyCond } from '../../gen/bundles';
import { QS_END, type QuestRecord } from './book';

/** What the predicates read of a player (`QuestSubject` of `quest/subject.rs`). */
export interface QuestSubject {
  level(): number;
  job(): number;
  sex(): number;
  gold(): number;
  /** `GetExpPercent()` in units of 1/100 percent (0..9999). */
  expPercent(): number;
  pkValue(): number;
  /** `GetSkill( s )`: the level of a learned skill, `undefined` without it. */
  skillLevel(skill: number): number | undefined;
  /** `CMover::CheckSkill( s )`. */
  checkSkill(skill: number): boolean;
  /** `GetItemNum( item )`. */
  itemNum(item: number): number;
  /** `m_Inventory.GetEmptyCount()`. */
  emptySlots(): number;
  isDisguised(): boolean;
  /** `GetIndex()` (the disguise when there is one). */
  moverIndex(): number;
  pet(): { readonly level: number; readonly expPercent: number } | null;
  tutorialState(): number;
  campusPoints(): number;
  party(): { readonly size: number; readonly isLeader: boolean } | null;
  guild(): { readonly size: number; readonly isMaster: boolean } | null;
  /** The quest book. */
  isActive(questId: number): boolean;
  isComplete(questId: number): boolean;
  record(questId: number): QuestRecord | undefined;
}

/** The size test of the party and guild rows: `num == 0` passes, else `==` (comp 0), `>=` (1) or `<=` (-1); any other comparison fails. */
function sizeTest(size: number, comp: number, num: number): boolean {
  if (num === 0) return true;
  switch (comp) {
    case 0:
      return size === num;
    case 1:
      return size >= num;
    case -1:
      return size <= num;
    default:
      return false;
  }
}

/** B11 / E9: 0 no condition, -1 must not be disguised, else the mover index must equal the value. */
function disguiseOk(idx: number, s: QuestSubject): boolean {
  if (idx === 0 || (idx === -1 && !s.isDisguised())) return true;
  return idx >>> 0 === s.moverIndex() >>> 0;
}

/** B12 / E8: the pet experience percent and level. */
function petOk(exp: number, level: number, s: QuestSubject): boolean {
  const pet = s.pet();
  const expOk = exp === 0 || (pet !== null && pet.expPercent >= exp);
  const levelOk = level === -1 || (pet !== null && pet.level === level);
  return expOk && levelOk;
}

const sexMatches = (e: QuestItemCond, s: QuestSubject): boolean =>
  e.sex === -1 || e.sex === s.sex();

/** Whether an entry applies to the subject: `true` it applies, `false` it does not (counts as a pass), `null` for a type other than 0 and 1 (a failure). */
function applies(e: QuestItemCond, s: QuestSubject): boolean | null {
  switch (e.type) {
    case 0:
      return e.job_or_item === -1 || e.job_or_item === s.job();
    case 1:
      return e.job_or_item === -1 || s.itemNum(e.job_or_item) !== 0;
    default:
      return null;
  }
}

/** One entry of B10 (`enough == false`: fewer than `num`), B14 and E15 (`enough == true`: at least `num`). A sex mismatch fails in the original and is "not applicable" with the fix. */
function itemEntryOk(e: QuestItemCond, s: QuestSubject, fixes: boolean, enough: boolean): boolean {
  if (!sexMatches(e, s)) return fixes;
  const a = applies(e, s);
  if (a === null) return false;
  if (!a) return true;
  if (e.item === 0) return true;
  return enough ? s.itemNum(e.item) >= e.num : s.itemNum(e.item) < e.num;
}

const itemsOk = (
  list: readonly QuestItemCond[],
  s: QuestSubject,
  fixes: boolean,
  enough: boolean,
): boolean => list.slice(0, 64).every((e) => itemEntryOk(e, s, fixes, enough));

/** E14 (any of): a sex-matching applicable entry with `itemNum >= num` passes the row; otherwise only when every entry is not applicable. */
function oneItemOk(list: readonly QuestItemCond[], s: QuestSubject, fixes: boolean): boolean {
  let allNotApplicable = true;
  for (const e of list.slice(0, 64)) {
    if (!sexMatches(e, s)) {
      allNotApplicable = allNotApplicable && fixes;
      continue;
    }
    const a = applies(e, s);
    if (a === null) {
      allNotApplicable = false;
    } else if (a) {
      if (s.itemNum(e.item) >= e.num) return true;
      allNotApplicable = false;
    }
  }
  return allNotApplicable;
}

/** The leader rule of the begin rows: the original compared with a never-computed `false`; the fix is -1 any, 0 not the leader, 1 the leader. */
function beginLeaderOk(leader: number, isLeader: boolean, fixes: boolean): boolean {
  if (fixes) return leader === -1 || leader === (isLeader ? 1 : 0);
  return leader === -1 || leader === 0;
}

/** B5. */
function beginPartyOk(c: QuestPartyCond, s: QuestSubject, fixes: boolean): boolean {
  switch (c.mode) {
    case 0:
      return true;
    case 1:
      return s.party() === null;
    case 2: {
      const v = s.party();
      return (
        v !== null && beginLeaderOk(c.leader, v.isLeader, fixes) && sizeTest(v.size, c.comp, c.num)
      );
    }
    default:
      return false;
  }
}

/** B6: the original never looked the guild up (mode 1 always passes, mode 2 never); the fix reads the real guild. */
function beginGuildOk(c: QuestPartyCond, s: QuestSubject, fixes: boolean): boolean {
  switch (c.mode) {
    case 0:
      return true;
    case 1:
      return !fixes || s.guild() === null;
    case 2: {
      const g = s.guild();
      return (
        fixes &&
        g !== null &&
        beginLeaderOk(c.leader, g.isMaster, true) &&
        sizeTest(g.size, c.comp, c.num)
      );
    }
    default:
      return false;
  }
}

/** B8 and its next-level variant: `GetSkill`, `CheckSkill( level )` (the original passed the LEVEL as a skill id; the fix passes the skill), the level compared unsigned in the begin row, signed in next level. */
function beginSkillOk(
  def: QuestClientDef,
  s: QuestSubject,
  fixes: boolean,
  signed: boolean,
): boolean {
  const [idx, lvl] = def.begin.skill;
  if (idx === 0) return true;
  const have = s.skillLevel(idx);
  if (have === undefined) return false;
  if (!s.checkSkill(fixes ? idx : lvl >>> 0)) return false;
  return signed ? have >= lvl : have >>> 0 >= lvl >>> 0;
}

/** How the level row reads (B4 or its next-level variant). */
type LevelRule = 'begin' | 'nextLevel';

/** B1..B3 and B5..B15: the rows both begin predicates share. */
function beginRowsOk(
  def: QuestClientDef,
  s: QuestSubject,
  fixes: boolean,
  rule: LevelRule,
): boolean {
  const b = def.begin;
  // B1 previous quests: 0 active or completed, 1 completed and not active, 2 active and not completed
  for (const q of b.previous.quests) {
    if (q === 0) continue;
    const a = s.isActive(q);
    const c = s.isComplete(q);
    let ok: boolean;
    switch (b.previous.type) {
      case 0:
        ok = a || c;
        break;
      case 1:
        ok = !a && c;
        break;
      case 2:
        ok = a && !c;
        break;
      default:
        ok = false;
    }
    if (!ok) return false;
  }
  // B2 exclusive quests: not active, and not completed (the original read the PREVIOUS quest of the slot)
  for (const [i, q] of b.exclusive.entries()) {
    if (q === 0) continue;
    const done = fixes ? s.isComplete(q) : s.isComplete(b.previous.quests[i] ?? 0);
    if (s.isActive(q) || done) return false;
  }
  // B3 jobs
  if (b.jobs.length > 0 && !b.jobs.includes(s.job())) return false;
  // B4 level
  const [min, max] = b.level;
  const lv = s.level();
  const levelOk =
    rule === 'begin' ? min === 0 || (lv >= min && lv <= max) : lv < min && lv + 5 >= min;
  if (!levelOk) return false;
  // B5 party, B6 guild
  if (!beginPartyOk(b.party, s, fixes) || !beginGuildOk(b.guild, s, fixes)) return false;
  // B7 sex
  if (!(b.sex === -1 || b.sex === s.sex())) return false;
  // B8 skill
  if (!beginSkillOk(def, s, fixes, rule === 'nextLevel')) return false;
  // B9 PK value
  if (!(b.pk_value === 0 || b.pk_value <= s.pkValue())) return false;
  // B10 items that must not be owned
  if (!itemsOk(b.not_items, s, fixes, false)) return false;
  // B11 disguise, B12 pet, B13 tutorial
  if (
    !disguiseOk(b.disguise, s) ||
    !petOk(b.pet_exp, b.pet_level, s) ||
    !(b.tutorial_state === -1 || s.tutorialState() >= b.tutorial_state)
  ) {
    return false;
  }
  // B14 items that must be owned
  if (!itemsOk(b.items, s, fixes, true)) return false;
  // B15 campus points
  return b.tsp === 0 || s.campusPoints() >= b.tsp;
}

/** B0: the quest is neither active nor completed. */
const b0Ok = (def: QuestClientDef, s: QuestSubject): boolean =>
  !s.isActive(def.id) && !s.isComplete(def.id);

/** `__IsBeginQuestCondition` (B0, B0a, B1-B15). */
export function canBegin(def: QuestClientDef, s: QuestSubject, fixes: boolean): boolean {
  if (!b0Ok(def, s)) return false;
  const used = def.begin.add_items.filter(([item]) => item !== 0).length;
  if (s.emptySlots() < used) return false;
  return beginRowsOk(def, s, fixes, 'begin');
}

/** `__IsNextLevelQuest`: B0 without B0a, B1-B3, B5-B15, and B4 replaced by `level < min && level + 5 >= min` (the quest is displayed red and never accepted). */
export function isNextLevel(def: QuestClientDef, s: QuestSubject, fixes: boolean): boolean {
  return b0Ok(def, s) && beginRowsOk(def, s, fixes, 'nextLevel');
}

/** E13 / E12 leader rule: 0 any, 1 not the leader, 2 the leader. */
const endLeaderOk = (leader: number, isLeader: boolean): boolean =>
  leader === 0 || leader - 1 === (isLeader ? 1 : 0);

/**
 * The state `SetEndCondState( 0 )` demands (E10): the highest state of the consecutive run `1, 2, ...` that is defined (0 when state 1 is missing), at most 13
 * (`QuestProp::last_defined_state`).
 */
export function lastDefinedState(def: Pick<QuestClientDef, 'states'>): number {
  const have = new Set(def.states.map((st) => st.state));
  let last = 0;
  for (let i = 1; i < 14; i += 1) {
    if (!have.has(i)) break;
    last = i;
  }
  return last;
}

/** E11. */
function completeQuestsOk(def: QuestClientDef, s: QuestSubject): boolean {
  const c = def.end.complete_quest;
  switch (c.oper) {
    case 0:
      return true;
    // `IsCompleteQuest` on every slot, literally: a 0 slot counts only if 0 is in the completed list
    case 1:
      return c.quests.some((q) => s.isComplete(q));
    case 2:
      return c.quests.every((q) => q === 0 || s.isComplete(q));
    default:
      return false;
  }
}

/** E12. */
function endPartyOk(c: QuestPartyCond, s: QuestSubject): boolean {
  switch (c.mode) {
    case 0:
      return true;
    case 1:
      return s.party() === null;
    case 2: {
      const v = s.party();
      return v !== null && endLeaderOk(c.leader, v.isLeader) && sizeTest(v.size, c.comp, c.num);
    }
    default:
      return false;
  }
}

/** E13: the original looked the guild up by the PLAYER id (no guild: mode 1 passes, mode 2 never); the fix reads the real guild. */
function endGuildOk(c: QuestPartyCond, s: QuestSubject, fixes: boolean): boolean {
  switch (c.mode) {
    case 0:
      return true;
    case 1:
      return !fixes || s.guild() === null;
    case 2: {
      const g = s.guild();
      return (
        fixes && g !== null && endLeaderOk(c.leader, g.isMaster) && sizeTest(g.size, c.comp, c.num)
      );
    }
    default:
      return false;
  }
}

/** `__IsEndQuestCondition` (E1-E16) for an active record that is not at state 14. */
export function canEnd(def: QuestClientDef, s: QuestSubject, fixes: boolean): boolean {
  const r = s.record(def.id);
  if (!r || r.state === QS_END) return false;
  const e = def.end;
  // E1 time limit: the stored word (the frozen bit counts as non-zero)
  if (!(e.limit_time_min === 0 || r.timeLeft !== 0 || r.frozen)) return false;
  // E2 patrol
  if (!(e.patrol === null || r.patrol)) return false;
  // E3 kills
  for (const [i, k] of e.kill.entries()) {
    if (!(k.mover === 0 || (r.kills[i] ?? 0) >= k.num)) return false;
  }
  // E4 skill
  const [skill, skillLevel] = e.skill;
  if (skill !== 0) {
    const have = s.skillLevel(skill);
    if (have === undefined || !s.checkSkill(skill) || !(have >= skillLevel >>> 0)) return false;
  }
  // E5 gold
  if (!(e.gold === 0 || s.gold() >= e.gold)) return false;
  // E6 level, E7 experience percent
  const lv = s.level();
  if (!(e.level[0] === 0 || (lv >= e.level[0] && lv <= e.level[1]))) return false;
  const pct = s.expPercent();
  if (!(e.exp_percent[0] === 0 || (pct >= e.exp_percent[0] && pct <= e.exp_percent[1])))
    return false;
  // E8 pet, E9 disguise, E10 state, E11 completed quests
  if (!petOk(e.pet_exp, e.pet_level, s) || !disguiseOk(e.disguise, s)) return false;
  if (!(e.state === 0 ? lastDefinedState(def) === r.state : e.state === r.state)) return false;
  if (!completeQuestsOk(def, s)) return false;
  // E12 party, E13 guild
  if (!endPartyOk(e.party, s) || !endGuildOk(e.guild, s, fixes)) return false;
  // E14 any of the items, E15 all of the items, E16 campus points
  return (
    oneItemOk(e.one_items, s, fixes) &&
    itemsOk(e.items, s, fixes, true) &&
    (e.tsp === 0 || s.campusPoints() >= e.tsp)
  );
}
