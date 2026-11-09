// The editing rules of the skill tree (`CWndSkillTreeEx`, `src:_Interface/WndField.cpp:6541-8750`; Plan/45 §19.1, ref/99 §4.6, WP-CFEAT-31):
// the window edits a LOCAL COPY of the 45 job-skill slots and a local skill-point count; the server state is only read. Everything
// here is a pure function of the two tables, so the rules are table-tested without a window:
//
//   `checkSkill`   the learnable test (`CheckSkill`, :7004-7058): the skill is not flagged `nLog == 1`, the character level reaches
//                  `dwReqDisLV` (masters and heroes are exempt), both prerequisites are met in the LOCAL copy;
//   `addPoint`     the `+` button (:8402-8416): one level for `GetSkillPoint(prop)` points while `level < dwExpertMax`;
//   `subPoint`     the `-` button (`IsDownPoint`, :6886-6900): only above the level the server knows; refunds the cost, then
//   `subDependents` (`SubSkillPointDown`, :6902-6946) recursively zeroes every dependent skill whose prerequisite fell below its
//                  required level and refunds `cost * level` for each;
//   `reset`        the reset button: back to the server table;
//   `planWords`    the 90 words `[skillId, level] x 45` that `rules.skillPlanCheck` and `DoUseSkillPointReq` carry.

import { JTYPE_HERO, JTYPE_MASTER } from '../../../gen/defs/job';

/** `NULL_ID`: an empty skill slot / an absent prerequisite. */
export const NULL_ID = 0xffff_ffff;

/** `MAX_SKILL_JOB`: vagrant 0..3, expert 3..23, pro 23..43, master 43, hero 44. */
export const MAX_SKILL_JOB = 45;
export const SLOT_VAGRANT = 0;
export const SLOT_EXPERT = 3;
export const SLOT_PRO = 23;
export const SLOT_MASTER = 43;
export const SLOT_HERO = 44;

/** One slot of `m_aJobSkill`. */
export interface SkillSlot {
  readonly skillId: number;
  readonly level: number;
}

export const EMPTY_SLOT: SkillSlot = { skillId: NULL_ID, level: 0 };

export type SkillTable = readonly SkillSlot[];

/** 45 empty slots. */
export function emptyTable(): SkillSlot[] {
  return Array.from({ length: MAX_SKILL_JOB }, () => EMPTY_SLOT);
}

/** Exactly 45 slots from any list (shorter lists are padded with empty slots, longer ones cut). */
export function normalizeTable(slots: readonly SkillSlot[]): SkillSlot[] {
  const out = emptyTable();
  for (let i = 0; i < MAX_SKILL_JOB; i += 1) {
    const s = slots[i];
    if (s) out[i] = s;
  }
  return out;
}

export function isEmptySlot(s: SkillSlot | undefined): boolean {
  return s === undefined || s.skillId === NULL_ID;
}

/** The fields of a skill row (`propSkill.txt`) the tree rules read. */
export interface SkillRule {
  /** `JTYPE_*`: the tier of the skill (`dwItemKind1`). */
  readonly kind1: number;
  /** `dwExpertMax`. */
  readonly expertMax: number;
  /** `dwReqDisLV`: the character level the skill needs (`NULL_ID` / negative: none). */
  readonly reqDisLv: number;
  readonly reSkill1: number;
  readonly reSkillLevel1: number;
  readonly reSkill2: number;
  readonly reSkillLevel2: number;
  /** `nLog`: 1 = the skill cannot be learned. */
  readonly log: number;
}

/** What the rules ask the data side. */
export interface TreeEnv {
  /** The row of a skill, `undefined` for a skill the data does not know. */
  rule(skillId: number): SkillRule | undefined;
  /** `GetSkillPoint(prop)`: the points one level costs (1 / 2 / 3 by tier). */
  cost(skillId: number): number;
}

/** The character facts `CheckSkill` reads. */
export interface TreePlayer {
  readonly level: number;
  readonly master: boolean;
  readonly hero: boolean;
}

/** The editable state of the window. */
export interface TreeState {
  /** The table the server knows (`g_pPlayer->m_aJobSkill`). */
  readonly server: SkillTable;
  /** `g_pPlayer->m_nSkillPoint`. */
  readonly serverSp: number;
  /** The local copy (`m_apSkill`). */
  readonly local: SkillTable;
  /** `m_nCurrSkillPoint`. */
  readonly localSp: number;
  /** `g_nSkillCurSelect`: the selected slot, `-1` = none (`m_pFocusItem == NULL`). */
  readonly selected: number;
}

/** `InitItem`: the local copy starts as the server table, nothing is selected (`__IMPROVE_SYSTEM_VER15`). */
export function initState(server: SkillTable, sp: number): TreeState {
  const t = normalizeTable(server);
  return { server: t, serverSp: sp, local: t, localSp: sp, selected: -1 };
}

/** `GetdwSkill`: the slot index that holds `skillId`, `-1` for none. */
export function slotOf(table: SkillTable, skillId: number): number {
  if (skillId === NULL_ID) return -1;
  return table.findIndex((s) => s.skillId === skillId);
}

/** The tree has unspent edits (`GetCurrSkillPoint() != m_nSkillPoint`). */
export function hasEdits(s: TreeState): boolean {
  return s.localSp !== s.serverSp;
}

/** `CheckSkill(i)`: may the skill in slot `i` be learned (shown lit, selected, dragged and levelled)? */
export function checkSkill(
  table: SkillTable,
  i: number,
  env: TreeEnv,
  player: TreePlayer,
): boolean {
  const slot = table[i];
  if (!slot || slot.skillId === NULL_ID) return false;
  const prop = env.rule(slot.skillId);
  if (!prop || prop.log === 1) return false;
  // `(int)dwReqDisLV`: the NULL value is negative as an `int`
  if (player.level < (prop.reqDisLv | 0) && !player.master && !player.hero) return false;
  for (const [need, needLevel] of [
    [prop.reSkill1, prop.reSkillLevel1],
    [prop.reSkill2, prop.reSkillLevel2],
  ] as const) {
    if (need === NULL_ID) continue;
    const at = slotOf(table, need);
    const have = at < 0 ? undefined : table[at];
    // a prerequisite that is not in the table is ignored (the original tests only a skill it finds)
    if (have && have.level < needLevel) return false;
  }
  return true;
}

/** Is the master / hero tier (`JTYPE_MASTER`, `JTYPE_HERO`): levelled through an NPC, never here. */
export function isLegendSkill(rule: SkillRule | undefined): boolean {
  return rule !== undefined && (rule.kind1 === JTYPE_MASTER || rule.kind1 === JTYPE_HERO);
}

/** What the four buttons are allowed to do right now (`CWndSkillTreeEx::Process`, :7820-7891). */
export interface ButtonState {
  readonly add: boolean;
  readonly sub: boolean;
  readonly reset: boolean;
  readonly finish: boolean;
}

const NO_BUTTONS: ButtonState = { add: false, sub: false, reset: false, finish: false };

/**
 * The enable states. Nothing is enabled without a selected skill or without server-side skill points (the original tests
 * `m_pFocusItem && 0 < g_pPlayer->m_nSkillPoint`); `+` needs room below `dwExpertMax` and enough local points, `-` a level above
 * the server's, reset and finish unspent edits; a master / hero skill disables all four.
 */
export function buttonState(s: TreeState, env: TreeEnv): ButtonState {
  const focus = s.local[s.selected];
  if (!focus || focus.skillId === NULL_ID || s.serverSp <= 0) return NO_BUTTONS;
  const prop = env.rule(focus.skillId);
  if (!prop) return NO_BUTTONS;
  if (isLegendSkill(prop)) return NO_BUTTONS;
  const serverLevel = s.server[s.selected]?.level ?? 0;
  return {
    add: focus.level < prop.expertMax && env.cost(focus.skillId) <= s.localSp,
    sub: focus.level !== serverLevel,
    reset: s.localSp !== s.serverSp,
    finish: s.localSp !== s.serverSp,
  };
}

/** Selects a slot if it can be learned (`OnLButtonDown`); selecting a non-learnable skill changes nothing. */
export function select(s: TreeState, i: number, env: TreeEnv, player: TreePlayer): TreeState {
  if (!checkSkill(s.local, i, env, player)) return s;
  return s.selected === i ? s : { ...s, selected: i };
}

function withLevel(table: SkillTable, i: number, level: number): SkillSlot[] {
  const out = table.slice();
  const cur = table[i];
  if (cur) out[i] = { skillId: cur.skillId, level };
  return out;
}

/**
 * The `+` button: one level of the selected skill for `GetSkillPoint` points. The original trusts that only a learnable skill
 * is ever selected; the plan adds `checkSkill` to the test, so a skill that became unlearnable after an edit cannot grow.
 */
export function addPoint(s: TreeState, env: TreeEnv, player: TreePlayer): TreeState {
  const i = s.selected;
  const focus = s.local[i];
  if (!focus || focus.skillId === NULL_ID || s.serverSp <= 0) return s;
  const prop = env.rule(focus.skillId);
  if (!prop || isLegendSkill(prop)) return s;
  const cost = env.cost(focus.skillId);
  if (cost > s.localSp || focus.level >= prop.expertMax) return s;
  if (!checkSkill(s.local, i, env, player)) return s;
  return {
    ...s,
    local: withLevel(s.local, i, focus.level + 1),
    localSp: s.localSp - cost,
  };
}

/** `IsDownPoint`: the local level is above the level the server knows. */
export function isDownPoint(s: TreeState, i: number): boolean {
  const l = s.local[i];
  const v = s.server[i];
  if (!l || l.skillId === NULL_ID || !v) return false;
  return l.level > v.level;
}

/**
 * `SubSkillPointDown(lpSkill)`: every skill that names `lpSkill` as prerequisite 1 or 2, whose required level is non-zero and above
 * the level `lpSkill` now has, loses all its levels (the points come back, `cost * level`) and its own dependents follow.
 */
function subDependents(table: SkillSlot[], skillId: number, env: TreeEnv): number {
  let refund = 0;
  const source = table[slotOf(table, skillId)];
  if (!source) return 0;
  for (let j = 0; j < table.length; j += 1) {
    const dep = table[j];
    if (!dep || dep.skillId === NULL_ID) continue;
    const prop = env.rule(dep.skillId);
    if (!prop) continue;
    for (const [need, needLevel] of [
      [prop.reSkill1, prop.reSkillLevel1],
      [prop.reSkill2, prop.reSkillLevel2],
    ] as const) {
      const now = table[j];
      if (need !== source.skillId || !now) continue;
      if (needLevel !== 0 && source.level < needLevel && now.level > 0) {
        refund += env.cost(dep.skillId) * now.level;
        table[j] = { skillId: now.skillId, level: 0 };
        refund += subDependents(table, now.skillId, env);
      }
    }
  }
  return refund;
}

/** The `-` button: only above the server level; refunds the cost and zeroes the dependents. */
export function subPoint(s: TreeState, env: TreeEnv): TreeState {
  const i = s.selected;
  const focus = s.local[i];
  if (!focus || focus.skillId === NULL_ID || s.serverSp <= 0) return s;
  const prop = env.rule(focus.skillId);
  if (!prop || isLegendSkill(prop)) return s;
  if (!isDownPoint(s, i)) return s;
  const table = withLevel(s.local, i, focus.level - 1);
  const refund = env.cost(focus.skillId) + subDependents(table, focus.skillId, env);
  return { ...s, local: table, localSp: s.localSp + refund };
}

/** The reset button: back to the server table, nothing selected (`m_pFocusItem = NULL`). */
export function reset(s: TreeState): TreeState {
  return initState(s.server, s.serverSp);
}

/** The server table / points changed (`InitItem`): the local copy is dropped, nothing is selected. */
export function reload(server: SkillTable, sp: number): TreeState {
  return initState(server, sp);
}

/** A table as the `[skillId, level] x 45` words of `skillPlanCheck`. */
export function tableWords(table: SkillTable): Uint32Array {
  const out = new Uint32Array(MAX_SKILL_JOB * 2);
  for (let i = 0; i < MAX_SKILL_JOB; i += 1) {
    const s = table[i] ?? EMPTY_SLOT;
    out[i * 2] = s.skillId >>> 0;
    out[i * 2 + 1] = s.level >>> 0;
  }
  return out;
}

/** The skill-point cost of a plan done by hand (what `skillPlanCheck` returns for a legal plan); used when the rules module is absent. */
export function planCost(server: SkillTable, local: SkillTable, env: TreeEnv): number {
  let cost = 0;
  for (let i = 0; i < MAX_SKILL_JOB; i += 1) {
    const a = server[i];
    const b = local[i];
    if (!a || !b || a.skillId === NULL_ID) continue;
    cost += (b.level - a.level) * env.cost(a.skillId);
  }
  return cost;
}
