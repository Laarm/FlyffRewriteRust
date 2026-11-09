// The destination arrow (`CWndWorld::m_vDestinationArrow`, `m_bSetQuestNPCDest`; Plan/45 §7.6, §7.7, §28.1; WP-CFEAT-42): where the player is told to go - a quest goal that was clicked in a condition
// line, the position of a quest NPC that the helper answered, or an NPC picked in the minimap's popup (V15, `WndField.cpp:10467-10469`, also without quest data).
//
//   * the state is a point `{x, z, fromNpc}` or nothing; the original's "nothing" is the vector `(-1, 0, -1)`, so a goal written at `(-1, -1)` leaves the destination unset;
//   * it is cleared (ANY destination) at the completion of a quest (state 14), by `RemoveQuestNtf`-free paths of the windows (a click on a condition line, a click on a title of the quick info),
//     on a world change (`DPClient.cpp:2467-2471`) and by a new pick of the minimap; V15 clears only the NPC flag and its distance label at Esc / `setFocus` (`WndManager.cpp:3458-3459`,
//     `World.cpp:296-297`): `clearNpcFlag`, the destination itself stays;
//   * the minimap draws `ButtDestination.bmp` scaled 0.4 at the point minus 3 (`WndField.cpp:9916-9935`); the world map in destination mode draws it pulsing; the 3-D arrow (`etc_arrow.o3d`) is
//     render-audio-m7's (`navigatorOf( ctx ).destination`).
//
// `goalSearch` is `SetQuestDestinationInformation` (`WndQuest.cpp:2425-2501`): the goal list searched in the order of the original, with its KEEP-QUIRK - the end-character, the patrol and the dialog goal
// of index 0 END the search with 0, so a later goal is found only when the earlier ones exist.

import type { QuestClientDef, QuestGoalData } from '../../gen/bundles';

/** A destination. */
export interface DestinationPoint {
  readonly x: number;
  readonly z: number;
  /** The point is the answer of the quest helper (`m_bSetQuestNPCDest`). */
  readonly fromNpc: boolean;
}

export class DestinationState {
  #point: DestinationPoint | null = null;
  readonly #listeners = new Set<() => void>();

  /** The destination, `null` for none. */
  get(): DestinationPoint | null {
    return this.#point;
  }

  /** `m_vDestinationArrow = (x, 0, z)`; the sentinel `(-1, -1)` is "none". */
  set(x: number, z: number, fromNpc = false): void {
    const next = x === -1 && z === -1 ? null : { x, z, fromNpc };
    this.#replace(next);
  }

  /** `m_vDestinationArrow = (-1, 0, -1)`: ANY destination goes. */
  clear(): void {
    this.#replace(null);
  }

  /** Drops only the NPC flag (`m_bSetQuestNPCDest = FALSE`); the destination itself stays. */
  clearNpcFlag(): void {
    const p = this.#point;
    if (p?.fromNpc === true) this.#replace({ ...p, fromNpc: false });
  }

  /** The destination came from the quest helper. */
  get fromNpc(): boolean {
    return this.#point?.fromNpc === true;
  }

  subscribe(fn: () => void): () => void {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  }

  #replace(next: DestinationPoint | null): void {
    const p = this.#point;
    if (p === next) return;
    if (p?.x === next?.x && p?.z === next?.z && p?.fromNpc === next?.fromNpc) return;
    this.#point = next;
    for (const fn of [...this.#listeners]) fn();
  }
}

/** The result of the goal path of one click. */
export interface GoalResult {
  /** The goal's position when the search found a goal that has one. */
  readonly point: { readonly x: number; readonly z: number } | null;
  /** `m_dwGoalTextID` of the found goal (the key of `quest_destinations`), 0 for none. */
  readonly textId: number;
}

const NONE: GoalResult = { point: null, textId: 0 };

const found = (g: QuestGoalData): GoalResult => ({ point: { x: g.x, z: g.z }, textId: g.text_id });

/**
 * `SetQuestDestinationInformation( id, goalIndex )` after the destination was cleared: `goalIndex == 0` finds nothing; then the end items, the two kills, the end character, the patrol zone and the
 * dialog character are searched in that order for the goal with that index. An end-character, patrol or dialog goal of index 0 ENDS the search with 0 (KEEP-QUIRK).
 */
export function goalSearch(def: QuestClientDef, goalIndex: number): GoalResult {
  if (goalIndex === 0) return NONE;
  for (const item of def.end.items) {
    const g = item.goal;
    if (!g || g.index === 0) continue;
    if (g.index === goalIndex) return found(g);
  }
  for (const kill of def.end.kill) {
    const g = kill.goal;
    if (!g || g.index === 0) continue;
    if (g.index === goalIndex) return found(g);
  }
  for (const g of [def.end.character.goal, def.end.patrol?.goal, def.end.dialog?.goal]) {
    if (!g || g.index === 0) return NONE;
    if (g.index === goalIndex) return found(g);
  }
  return NONE;
}

/** What `ProcessQuestDestinationWorldMap` does with the result: the world map in destination mode, a box with the goal text, or nothing. */
export type GoalAction =
  | { readonly kind: 'worldMap' }
  | { readonly kind: 'box'; readonly textId: number }
  | { readonly kind: 'none' };

/** A set destination opens the world map; else a goal with a text id opens the box; else nothing. */
export function goalAction(destination: DestinationPoint | null, textId: number): GoalAction {
  if (destination) return { kind: 'worldMap' };
  if (textId !== 0) return { kind: 'box', textId };
  return { kind: 'none' };
}
