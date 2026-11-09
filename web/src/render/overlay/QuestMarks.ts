// The quest emoticons over the NPCs (Plan/41 section 19.6, WP-REND-27; `CMover::RenderQuestEmoticon`, `MoverRender.cpp:1217-1268`, `ProcessQuestEmoticon`, `WndWorld.cpp:8194-8210`): a small
// picture (a "!" for a new quest, a "?" for one in progress or ready to hand in, ...) above the head of an NPC that has something to say about the player's quests.
//
//   index    1 new, 2 in progress, 3 completable, 4 near level (`questOf(ctx).emoticonOf(objId)`: ui-quest's port of the `__IMPROVE_QUEST_INTERFACE` loop of `Mover.cpp:1108-1157`); <= 0: none
//   anchor   the head anchor of section 19.1: `(0, bb.max.y, 0)` through the NPC's scale, rotation and translation, projected to the screen
//   place    `x = trunc(even(trunc(anchor.x)) - 16 s)`, `y = trunc(trunc(anchor.y) - 50 - 16 s)`: the 32 px sprite is drawn at scale `s` from its top-left corner, so it grows about the
//            point above the head; alpha 255, linear filter
//   pulse    ONE global scale `s`: 1.0 <-> 1.2 by 0.01 per 60 Hz tick, starting upward (KEEP: not per NPC; it ticks per frame in the original, PL-134: per tick here)
//   hidden   for players, while the NPC has a speech balloon, and (PL-148) while the quest data is empty: `emoticonOf` then answers -1 for every NPC
//
// Pure: the layout takes the anchor and the scale and returns a sprite item.

import type { SpriteItem } from './types';

/** `g_Neuz.m_texQuestEmoticon` (`Icon/icon_QuestEmoticon.inc`), lower case like every atlas path. */
export const QUEST_EMOTICON_ATLAS = 'icon/icon_questemoticon.inc';
/** The scale limits and the step of `ProcessQuestEmoticon`. */
export const QUEST_PULSE_MIN = 1.0;
export const QUEST_PULSE_MAX = 1.2;
export const QUEST_PULSE_STEP = 0.01;
/** Half the width of the 32 px picture and the lift above the head anchor (`point.y -= 50`). */
const HALF = 16;
const LIFT = 50;

/** `CMover::m_fQuestEmoticonScale` / `m_bQuestEmoticonAdd`: the one global pulse. */
export class QuestPulse {
  scale = 1;
  up = true;

  /** One `ProcessQuestEmoticon` (a 60 Hz tick). */
  tick(): void {
    if (this.up) {
      this.scale = Math.fround(this.scale + QUEST_PULSE_STEP);
      if (this.scale >= QUEST_PULSE_MAX) this.up = false;
    } else {
      this.scale = Math.fround(this.scale - QUEST_PULSE_STEP);
      if (this.scale <= QUEST_PULSE_MIN) this.up = true;
    }
  }
}

/** `MakeEven( x )`: clears the lowest bit of an integer (two's complement, like the C macro). */
export const makeEven = (x: number): number => Math.trunc(x) & ~1;

/** The sprite of an emoticon above the head anchor `(x, y)` (screen pixels) at pulse scale `scale`; `null` for an index of 0 or below. */
export function layoutQuestMark(
  anchor: { readonly x: number; readonly y: number },
  index: number,
  scale: number,
): SpriteItem | null {
  if (!(index > 0)) return null;
  const x = Math.trunc(makeEven(anchor.x) - HALF * scale);
  const y = Math.trunc(Math.trunc(anchor.y) - LIFT - HALF * scale);
  return { kind: 'sprite', atlas: QUEST_EMOTICON_ATLAS, index, x, y, alpha: 255, scale };
}

/** Whether a subject gets a mark: an NPC (never a player) with a positive index and no speech balloon. */
export function wantsQuestMark(
  s: { readonly kind: string; readonly questEmoticon?: number },
  hasBalloon: boolean,
): boolean {
  return s.kind === 'npc' && (s.questEmoticon ?? 0) > 0 && !hasBalloon;
}
