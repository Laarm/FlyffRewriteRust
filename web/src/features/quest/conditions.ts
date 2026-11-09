// The condition and reward lines of the quest windows (`MakeQuestConditionItems`, `MakeString`, `MakeTextColor`, `CWndQuestDetail::UpdateQuestDetailText`; `WndQuest.cpp:1742-1885, 1887-2417`; Plan/45
// §28.1; WP-CFEAT-42): the lines of the detail window's condition tree, of the quick-info window, the description and the reward text. Pure functions over the definition, the record and a
// small environment, so the golden strings are plain unit tests. All lines are display only: the server decides what completes a quest.
//
// The lines appear only when the record is at the last defined state below 14 (the first missing of the states 0..13, minus one) or at 14 (KEEP), in this order: time, the two kills, the end items,
// the patrol zone, the disguise, the skill, the party and guild lines, the one-of items (ONE line), the dialog, the end character or the first multi character whose item is held.
//
// KEEP-QUIRKS of the original: the guild lines test the PARTY (`bGuild = g_Party.IsMember`, the size of the party); the leader of a party line is a constant `FALSE`; `MakeTextColor` answers 0 for a
// zero goal; the one-of line is grey as soon as one entry is reached. V15 has no chaotic lines (they sit in the `__VER < 8` branch).

import type { QuestClientDef, QuestItemCond } from '../../gen/bundles';
import {
  TID_GAME_QUEST_COMPLETE_TEXT,
  TID_QUEST_COMPLETED,
  TID_QUEST_COND_ITEM,
  TID_QUEST_DESTINATION,
  TID_QUEST_DIALOG,
  TID_QUEST_DISQUISE,
  TID_QUEST_EXP1,
  TID_QUEST_GOLD1,
  TID_QUEST_GOLD2,
  TID_QUEST_GUILD,
  TID_QUEST_GUILD_LEADER,
  TID_QUEST_GUILD_LEADER_NUM,
  TID_QUEST_GUILD_MEMBER,
  TID_QUEST_GUILD_MEMBER_NUM,
  TID_QUEST_GUILD_NUM,
  TID_QUEST_GUILD_SOLO,
  TID_QUEST_ITEM,
  TID_QUEST_ITEM1,
  TID_QUEST_KILL_NPC,
  TID_QUEST_LIMIT_TIME,
  TID_QUEST_LIMIT_TIMEOUT,
  TID_QUEST_NODISQUISE,
  TID_QUEST_PARTY,
  TID_QUEST_PARTY_LEADER,
  TID_QUEST_PARTY_LEADER_NUM,
  TID_QUEST_PARTY_MEMBER,
  TID_QUEST_PARTY_MEMBER_NUM,
  TID_QUEST_PARTY_NUM,
  TID_QUEST_PARTY_SOLO,
  TID_QUEST_PATROL,
  TID_QUEST_PK_REWARD_PKVALUE,
  TID_QUEST_SKILL_LVL,
  TID_QUEST_SKILLPOINT,
} from '../../gen/defs/text';
import { makeTextColor } from '../../ui/windows/quest/rows';
import { QS_END, timeWord, type QuestRecord } from './book';

export { makeTextColor };

/** The colours of a condition tree: untouched lines, reached lines, the selected line (`D3DCOLOR_ARGB`). */
export interface CondColors {
  readonly start: number;
  readonly end: number;
  readonly select: number;
}

/** The detail window's colours (`MakeQuestConditionItems` defaults: black, black, blue). */
export const DETAIL_COLORS: CondColors = { start: 0xff000000, end: 0xff000000, select: 0xff0000ff };

/** One line of a condition tree: the markup text, the goal index of the click (`m_dwData`), the colours. */
export interface CondLine {
  readonly text: string;
  readonly data: number;
  readonly color: number;
  readonly selectColor: number;
  /** The "go to the NPC" line of the end character (a click also asks the server for the NPC's position). */
  readonly role?: 'character';
}

/** What the lines read of the client. */
export interface CondEnv {
  /** The template of a text id with its arguments applied (`GETTEXT` + `Format`). */
  fmt(tid: number, args: readonly (string | number)[]): string;
  sex(): number;
  job(): number;
  itemNum(item: number): number;
  isDisguised(): boolean;
  skillLevel(skill: number): number | undefined;
  checkSkill(skill: number): boolean;
  /** `g_Party.IsMember( own )` and `g_Party.GetSizeofMember()` (the guild lines read these too: KEEP-QUIRK). */
  inParty(): boolean;
  partySize(): number;
  moverName(id: number): string;
  itemName(id: number): string;
  skillName(id: number): string;
  /** `CHARACTER.m_strName` of an NPC key; `undefined` for an unknown key. */
  npcName(key: string): string | undefined;
  /** `prj.GetPatrolDestination( id )`. */
  patrolName(id: number): string;
}

/** `MakeString`: a reached condition is struck out in grey. */
export const makeString = (text: string, reached: boolean): string =>
  reached ? `#cffa0a0a0${text}#nc` : text;

/** The first missing state of 0..13 (14 when none is missing): the lines show at `i - 1` or at 14. */
function firstMissingState(def: QuestClientDef): number {
  const have = new Set(def.states.map((s) => s.state));
  let i = 0;
  while (i < 14 && have.has(i)) i += 1;
  return i;
}

/** Whether an item entry applies to the player (sex, then job or held item) and names an item. */
function itemApplies(e: QuestItemCond, env: CondEnv): boolean {
  if (!(e.sex === -1 || e.sex === env.sex())) return false;
  if (e.type === 0) {
    if (!(e.job_or_item === -1 || e.job_or_item === env.job())) return false;
  } else if (e.type === 1) {
    if (!(e.job_or_item === -1 || env.itemNum(e.job_or_item) !== 0)) return false;
  } else {
    return false;
  }
  return e.item !== 0;
}

/** A party or guild line (`WndQuest.cpp:2036-2200`): the texts of `family` ('party' | 'guild'), the sign of the comparison, `size` and `member` as the original computed them. */
function groupLine(
  env: CondEnv,
  family: 'party' | 'guild',
  mode: number,
  leader: number,
  num: number,
  comp: number,
  member: boolean,
  size: number,
  c: CondColors,
): CondLine | null {
  const T =
    family === 'party'
      ? {
          solo: TID_QUEST_PARTY_SOLO,
          any: TID_QUEST_PARTY,
          member: TID_QUEST_PARTY_MEMBER,
          leader: TID_QUEST_PARTY_LEADER,
          anyNum: TID_QUEST_PARTY_NUM,
          memberNum: TID_QUEST_PARTY_MEMBER_NUM,
          leaderNum: TID_QUEST_PARTY_LEADER_NUM,
        }
      : {
          solo: TID_QUEST_GUILD_SOLO,
          any: TID_QUEST_GUILD,
          member: TID_QUEST_GUILD_MEMBER,
          leader: TID_QUEST_GUILD_LEADER,
          anyNum: TID_QUEST_GUILD_NUM,
          memberNum: TID_QUEST_GUILD_MEMBER_NUM,
          leaderNum: TID_QUEST_GUILD_LEADER_NUM,
        };
  if (mode === 1) {
    return {
      text: makeString(env.fmt(T.solo, []), !member),
      data: 0,
      color: member ? c.start : c.end,
      selectColor: c.select,
    };
  }
  if (mode !== 2) return null;
  // the original's leader flag is the constant FALSE: leader 1 (member) holds, leader 2 (leader) never
  let text = '';
  let result = false;
  if (num === 0) {
    const tid = leader === 0 ? T.any : leader === 1 ? T.member : leader === 2 ? T.leader : 0;
    text = env.fmt(tid, []);
    result = leader === 0 || leader === 1;
  } else {
    const tid =
      leader === 0 ? T.anyNum : leader === 1 ? T.memberNum : leader === 2 ? T.leaderNum : 0;
    const sign = comp === 0 ? '=' : comp === -1 ? '<=' : comp === 1 ? '>=' : null;
    if (sign !== null) {
      text = env.fmt(tid, [size, sign, num]);
      result = comp === 0 ? size === num : comp === -1 ? size <= num : size >= num;
    }
  }
  const reached = result && member;
  return {
    text: makeString(text, reached),
    data: 0,
    color: reached ? c.end : c.start,
    selectColor: c.select,
  };
}

/** The lines of the condition tree of an active record (`MakeQuestConditionItems`). Empty unless the record is at its last defined state or at 14. */
export function conditionLines(
  def: QuestClientDef,
  rec: QuestRecord,
  env: CondEnv,
  c: CondColors = DETAIL_COLORS,
): CondLine[] {
  const out: CondLine[] = [];
  const e = def.end;
  const i = firstMissingState(def);
  if (!(rec.state === i - 1 || rec.state === QS_END)) return out;
  const push = (text: string, data: number, color: number, role?: 'character'): void => {
    out.push({ text, data, color, selectColor: c.select, ...(role ? { role } : {}) });
  };
  // E1: the time limit (the word, so a frozen timer shows its minutes)
  if (e.limit_time_min !== 0) {
    if (timeWord(rec) !== 0) push(env.fmt(TID_QUEST_LIMIT_TIME, [rec.timeLeft]), 0, c.end);
    else push(`#cffff0000${env.fmt(TID_QUEST_LIMIT_TIMEOUT, [])}#ns#nc`, 0, c.start);
  }
  // E3: the kills
  for (const [k, kill] of e.kill.entries()) {
    if (kill.mover === 0) continue;
    const have = rec.kills[k] ?? 0;
    const shown = Math.min(have, kill.num);
    const text = env.fmt(TID_QUEST_KILL_NPC, [env.moverName(kill.mover), have, kill.num]);
    out.push({
      text: makeString(text, shown === kill.num),
      data: kill.goal?.index ?? 0,
      color: makeTextColor(c.start, c.end, have, kill.num),
      selectColor: c.select,
    });
  }
  // E15: the end items
  for (const item of e.items) {
    if (!itemApplies(item, env)) continue;
    const n = env.itemNum(item.item);
    const text = env.fmt(TID_QUEST_ITEM, [env.itemName(item.item), n, item.num]);
    out.push({
      text: makeString(text, n >= item.num),
      data: item.goal?.index ?? 0,
      color: makeTextColor(c.start, c.end, n, item.num),
      selectColor: c.select,
    });
  }
  // E2: the patrol zone
  if (e.patrol) {
    const destination = e.patrol.destination === null ? '' : env.patrolName(e.patrol.destination);
    let text = env.fmt(TID_QUEST_PATROL, [destination]);
    if (rec.patrol) text += ` ${env.fmt(TID_GAME_QUEST_COMPLETE_TEXT, [])}`;
    push(makeString(text, rec.patrol), e.patrol.goal?.index ?? 0, rec.patrol ? c.end : c.start);
  }
  // E9: the disguise
  if (e.disguise !== 0) {
    const disguised = env.isDisguised();
    if (e.disguise === -1) {
      push(
        makeString(env.fmt(TID_QUEST_NODISQUISE, []), !disguised),
        0,
        !disguised ? c.end : c.start,
      );
    } else {
      push(
        makeString(env.fmt(TID_QUEST_DISQUISE, [env.moverName(e.disguise)]), disguised),
        0,
        disguised ? c.end : c.start,
      );
    }
  }
  // E4: the skill
  const [skill, skillLvl] = e.skill;
  if (skill !== 0) {
    const have = env.skillLevel(skill);
    if (env.checkSkill(skill) && have !== undefined) {
      push(
        makeString(
          env.fmt(TID_QUEST_SKILL_LVL, [env.skillName(skill), have, skillLvl]),
          have >= skillLvl,
        ),
        0,
        makeTextColor(c.start, c.end, have, skillLvl),
      );
    } else {
      push(
        env.fmt(TID_QUEST_SKILL_LVL, [env.skillName(skill), 0, skillLvl]),
        0,
        makeTextColor(c.start, c.end, 0, skillLvl),
      );
    }
  }
  // E12: the party; E13: the guild (KEEP-QUIRK: both read the party)
  const party = groupLine(
    env,
    'party',
    e.party.mode,
    e.party.leader,
    e.party.num,
    e.party.comp,
    env.inParty(),
    env.partySize(),
    c,
  );
  if (party) out.push(party);
  const guild = groupLine(
    env,
    'guild',
    e.guild.mode,
    e.guild.leader,
    e.guild.num,
    e.guild.comp,
    env.inParty(),
    env.partySize(),
    c,
  );
  if (guild) out.push(guild);
  // E14: one of the items (ONE line)
  const applicable = e.one_items.filter((it) => itemApplies(it, env));
  if (applicable.length > 0) {
    const reached = applicable.some((it) => env.itemNum(it.item) >= it.num);
    const prefix = reached ? '#s#cffa0a0a0' : '';
    let tail = '';
    for (const it of applicable) {
      tail += `${prefix}-${env.fmt(TID_QUEST_ITEM, [env.itemName(it.item), env.itemNum(it.item), it.num])}#ns#nc`;
    }
    push(`${prefix}${env.fmt(TID_QUEST_COND_ITEM, [])}#ns#nc${tail}`, 0, c.end);
  }
  // the dialog
  if (e.dialog && e.dialog.npc !== '') {
    const name = env.npcName(e.dialog.npc.toLowerCase()) ?? '';
    push(
      makeString(env.fmt(TID_QUEST_DIALOG, [name]), rec.dialog),
      e.dialog.goal?.index ?? 0,
      c.end,
    );
  }
  // the end character, else the first multi character whose item is held
  if (e.character.npc !== '') {
    const name = env.npcName(e.character.npc.toLowerCase()) ?? '';
    push(env.fmt(TID_QUEST_DESTINATION, [name]), e.character.goal?.index ?? 0, c.end, 'character');
  } else {
    for (const [npc, item] of e.multi_characters) {
      const name = env.npcName(npc.toLowerCase());
      if (name === undefined) continue;
      if (item !== 0 && env.itemNum(item) !== 0) {
        push(env.fmt(TID_QUEST_DESTINATION, [name]), 0, c.end);
        break;
      }
    }
  }
  return out;
}

/** The single line of a completed quest (`TID_QUEST_COMPLETED`). */
export function completedLine(env: CondEnv, c: CondColors = DETAIL_COLORS): CondLine {
  return { text: env.fmt(TID_QUEST_COMPLETED, []), data: 0, color: c.start, selectColor: c.select };
}

/**
 * The description of the detail window: the text of the record's state, the highest defined state 14..0 for a completed quest. FIX: empty when the state has no description (the original
 * dereferenced null).
 */
export function descriptionText(
  def: QuestClientDef,
  rec: QuestRecord | null,
  completed: boolean,
): string {
  const byState = new Map(def.states.map((s) => [s.state, s.desc]));
  if (completed) {
    for (let i = QS_END; i >= 0; i -= 1) {
      const d = byState.get(i);
      if (d !== undefined) return d;
    }
    return '';
  }
  return rec ? (byState.get(rec.state) ?? '') : '';
}

/**
 * The reward text of the detail window (`UpdateQuestDetailText`): one line per applicable item, the penya, then the experience, skill point and PK lines. `rewards == null` (the quest hides them)
 * gives an empty text, also after the completion (a limit of the frozen bundle schema; the original showed placeholders before and the real lines after the completion).
 */
export function rewardText(def: QuestClientDef, env: CondEnv): string {
  const r = def.rewards;
  if (!r) return '';
  let out = '';
  for (const it of r.items) {
    if (!(it.sex === -1 || it.sex === env.sex())) continue;
    const ok =
      it.type === 0
        ? it.job_or_item === -1 || it.job_or_item === env.job()
        : it.type === 1
          ? it.job_or_item === -1 || env.itemNum(it.job_or_item) !== 0
          : false;
    if (ok && it.item !== 0)
      out += `${env.fmt(TID_QUEST_ITEM1, [env.itemName(it.item), it.num])}\n`;
  }
  const [gMin, gMax] = r.gold;
  if (gMin !== 0) {
    out += `${gMax - gMin === 0 ? env.fmt(TID_QUEST_GOLD1, [gMin]) : env.fmt(TID_QUEST_GOLD2, [gMin, gMax])}\n`;
  }
  if (r.exp[0] !== 0) out += `${env.fmt(TID_QUEST_EXP1, [])}\n`;
  if (r.skill_point !== 0) out += `${env.fmt(TID_QUEST_SKILLPOINT, [])}\n`;
  if (r.pk_value[0] !== 0) out += `${env.fmt(TID_QUEST_PK_REWARD_PKVALUE, [])}\n`;
  return out;
}
