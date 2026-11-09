// The quest marks of the NPCs (`CMover::ProcessQuest`, the client branch `Mover.cpp:1108-1160`, V15 priority "finished > new > in progress > near level"; Plan/45 §7.6, §28.1; WP-CFEAT-42): the index
// `m_nQuestEmoticonIndex` of an NPC key, ported LITERALLY with the order quirks of the loop:
//
//   -1 none   1 a new quest (yellow "!")   2 in progress   3 completable   4 near level (red)
//
// for each `quest_src` quest in file order, with the giver item `nItem` of the quest: not active and not completed -> `canBegin && (nItem == 0 || holds nItem)` -> 1, else if the index is not 1 and
// `isNextLevel` and the item rule -> 4; active, defined, not completed, state != 14 -> index != 1 and the item rule -> 2. Then each `quest_dst` quest (item = this NPC's entry of
// `end.multi_characters`, else 0): active, defined, not completed, state != 14 -> `canEnd` and the item rule -> 3, else index not 3 and not 1 and the item rule -> 2. A later near-level quest
// overwrites a 2, a completable one overwrites a 1 (KEEP).
//
// `Theme/navigator.inc` has 24 frames (20-23 = indices 0-3), so index 4 has NO minimap frame: the original's `MakeVertex( 24 )` read past the frame table; the navigator draws nothing for it.

import type { QuestClientDef } from '../../gen/bundles';
import { QS_END } from './book';
import { canBegin, canEnd, isNextLevel, type QuestSubject } from './predicates';

/** The facts of the quests an NPC mark reads. */
export interface MarkerFacts {
  def(questId: number): QuestClientDef | undefined;
  /** `quest_src[ key ]` (the key in lower case): the quests the NPC gives, file order. */
  src(key: string): readonly number[];
  /** `quest_dst[ key ]`: the quests the NPC completes. */
  dst(key: string): readonly number[];
  readonly subject: QuestSubject;
  /** `fidelity.quest_condition_fixes`. */
  readonly fixes: boolean;
}

/** The giver item of a quest for an NPC key (`m_anSrcQuestItem[ i ]`): the first giver entry of that NPC, 0 without one. */
export function giverItem(def: QuestClientDef, key: string): number {
  for (const [npc, item] of def.givers) if (npc.toLowerCase() === key) return item;
  return 0;
}

/** The item of the end NPC (`m_anDstQuestItem[ i ]`): this NPC's entry of `end.multi_characters`, else 0. */
export function destinationItem(def: QuestClientDef, key: string): number {
  for (const [npc, item] of def.end.multi_characters) if (npc.toLowerCase() === key) return item;
  return 0;
}

/** `m_nQuestEmoticonIndex` of the NPC with this key (`''` = no character: -1). */
export function markerIndex(npcKey: string, f: MarkerFacts): number {
  const key = npcKey.toLowerCase();
  if (key === '') return -1;
  const s = f.subject;
  const itemRule = (item: number): boolean => item === 0 || s.itemNum(item) !== 0;
  let index = -1;
  for (const questId of f.src(key)) {
    const def = f.def(questId);
    if (!def) continue;
    const item = giverItem(def, key);
    const record = s.record(questId);
    // a new quest
    if (!record && !s.isComplete(questId)) {
      if (canBegin(def, s, f.fixes) && itemRule(item)) index = 1;
      else if (index !== 1 && isNextLevel(def, s, f.fixes) && itemRule(item)) index = 4;
    }
    // a quest in progress
    if (record && !s.isComplete(questId) && record.state !== QS_END) {
      if (index !== 1 && itemRule(item)) index = 2;
    }
  }
  for (const questId of f.dst(key)) {
    const def = f.def(questId);
    if (!def) continue;
    const item = destinationItem(def, key);
    const record = s.record(questId);
    if (record && !s.isComplete(questId) && record.state !== QS_END) {
      if (canEnd(def, s, f.fixes) && itemRule(item)) index = 3;
      else if (index !== 3 && index !== 1 && itemRule(item)) index = 2;
    }
  }
  return index;
}

/** The minimap frame of an index: `20 + index` for 0..3, `null` for 4 (no frame in `navigator.inc`) and for none. */
export function minimapFrame(index: number): number | null {
  return index >= 0 && index <= 3 ? 20 + index : null;
}
