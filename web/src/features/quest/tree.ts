// The model of the quest list (`CWndQuest::Update`, `InsertQuestItem`, `GetQuestTreeSelf`, `GetRootHeadQuest`, `WndQuest.cpp:145-200, 1352-1514, 2419-2430`; Plan/45 §28.1; WP-CFEAT-42): which quest
// goes under which folder of which tab, as plain data. The window builds its four `QuestTree`s from it; nothing here knows a control.
//
//   * four tabs in the order Scenario 6000, Normal 6001, Request 6002, Event 6003 (`QUEST_KIND_*`); the tab of a quest is the ROOT of its `head_quest` chain (`GetRootHeadQuest`): a kind id 6000-6003,
//     anything else Normal;
//   * the group CURRENT adds the active records, COMPLETE the completed ids, ALL both (active first);
//   * every head of the chain that is not a kind id becomes a folder (its title, no check box) inserted ONCE; the quest is a leaf under the last folder;
//   * FIX: a quest with `head_quest == 0` or a chain that reaches an id without a definition is placed at the ROOT of the tab of the chain's last id (Normal when unknown) - the original never
//     listed head-0 quests and dereferenced null for a missing head;
//   * a leaf is the title, plus `"(state, id)"` for a game master (`g_Option.m_bOperator || IsAuthHigher( AUTH_GAMEMASTER )`: the `sunkist` option is never set in a normal client, so the authority
//     decides); a completed leaf has the colour `0xffc0c0c0` and no check box; an active leaf is checkable and ticked when the id is in `checked`.

import type { QuestClientDef } from '../../gen/bundles';
import {
  QUEST_KIND_EVENT,
  QUEST_KIND_NORMAL,
  QUEST_KIND_REQUEST,
  QUEST_KIND_SCENARIO,
} from '../../gen/defs/quest';
import { EMPTY_QUEST_ID, QUEST_GROUP, QS_END, type QuestBook, type QuestGroup } from './book';

/** The quest kinds in tab order. */
export const QUEST_KINDS: readonly number[] = [
  QUEST_KIND_SCENARIO,
  QUEST_KIND_NORMAL,
  QUEST_KIND_REQUEST,
  QUEST_KIND_EVENT,
];
/** The English fallback of a tab title (the prop of the kind id gives the real one). */
export const TAB_FALLBACK_TITLES: readonly string[] = ['Scenario', 'Normal', 'Request', 'Event'];
/** The index of the Normal tab (`default:` of `GetQuestTreeSelf`). */
export const NORMAL_TAB = 1;
/** The colour of a completed quest (`m_dwColor = 0xffc0c0c0`). */
export const COMPLETED_COLOR = 0xffc0c0c0;

export interface QuestNode {
  /** The quest id (`m_dwData`). */
  readonly id: number;
  label: string;
  /** An active or completed quest (a plain folder is not). */
  isQuest: boolean;
  completed: boolean;
  readonly children: QuestNode[];
}

export interface QuestTreeModel {
  /** The four tabs' root nodes. */
  readonly tabs: readonly QuestNode[][];
}

/** What the model reads of the client. */
export interface TreeEnv {
  def(questId: number): QuestClientDef | undefined;
  /** `IsAuthHigher( AUTH_GAMEMASTER )`. */
  readonly isGm: boolean;
}

/** `QUEST_KIND_*` ids. */
export const isKind = (id: number): boolean => QUEST_KINDS.includes(id);

/** The tab of a kind id, `NORMAL_TAB` for anything else. */
export function kindTab(id: number): number {
  const i = QUEST_KINDS.indexOf(id);
  return i < 0 ? NORMAL_TAB : i;
}

/**
 * `GetRootHeadQuest( id )`: the end of the `head_quest` chain. FIX: an id without a definition ends the chain (the original dereferenced null); a cycle ends at the first repeated id.
 */
export function rootHeadQuest(id: number, def: (id: number) => QuestClientDef | undefined): number {
  const seen = new Set<number>();
  let at = id;
  for (;;) {
    if (seen.has(at)) return at;
    seen.add(at);
    const d = def(at);
    if (!d || d.head_quest <= 0) return at;
    at = d.head_quest;
  }
}

/** `GetQuestTreeSelf( id )`: the tab that holds a quest (`default:` Normal). */
export function tabOfQuest(id: number, def: (id: number) => QuestClientDef | undefined): number {
  return kindTab(rootHeadQuest(id, def));
}

/** The text of a leaf: the title, plus `"(state, id)"` for a game master. */
export function leafLabel(title: string, state: number, id: number, isGm: boolean): string {
  return isGm ? `${title}(${String(state)}, ${String(id)})` : title;
}

/** The quest list of a group (`CWndQuest::Update`). */
export function buildQuestTree(book: QuestBook, group: QuestGroup, env: TreeEnv): QuestTreeModel {
  const tabs: QuestNode[][] = [[], [], [], []];
  /** The nodes inserted so far by quest id (a folder is inserted once). */
  const nodes = new Map<number, { node: QuestNode; tab: number }>();

  /** The tab and the parent node under which the quest `def` goes; the folders of the chain are created on the way. */
  const parentOf = (
    d: QuestClientDef,
    guard: Set<number>,
  ): { tab: number; parent: QuestNode | null } => {
    const h = d.head_quest;
    if (h <= 0) return { tab: NORMAL_TAB, parent: null };
    if (isKind(h)) return { tab: kindTab(h), parent: null };
    const hd = env.def(h);
    if (!hd) return { tab: NORMAL_TAB, parent: null };
    const known = nodes.get(h);
    if (known) return { tab: known.tab, parent: known.node };
    if (guard.has(h)) return { tab: NORMAL_TAB, parent: null };
    guard.add(h);
    const up = parentOf(hd, guard);
    const folder: QuestNode = {
      id: h,
      label: hd.title,
      isQuest: false,
      completed: false,
      children: [],
    };
    (up.parent ? up.parent.children : (tabs[up.tab] ?? [])).push(folder);
    nodes.set(h, { node: folder, tab: up.tab });
    return { tab: up.tab, parent: folder };
  };

  const insert = (id: number, state: number, completed: boolean): void => {
    const d = env.def(id);
    // a quest without a definition has no title: nothing to list (the original skipped a null property)
    if (!d) return;
    const label = leafLabel(d.title, state, id, env.isGm);
    const existing = nodes.get(id);
    if (existing) {
      // the quest was inserted as the folder of another quest before: it is that quest's node now
      existing.node.label = label;
      existing.node.isQuest = true;
      existing.node.completed = completed;
      return;
    }
    const { tab, parent } = parentOf(d, new Set([id]));
    const node: QuestNode = { id, label, isQuest: true, completed, children: [] };
    (parent ? parent.children : (tabs[tab] ?? [])).push(node);
    nodes.set(id, { node, tab });
  };

  if (group !== QUEST_GROUP.Complete) {
    for (const r of book.active) {
      if (r.id === EMPTY_QUEST_ID) continue;
      insert(r.id, r.state, false);
    }
  }
  if (group !== QUEST_GROUP.Current) {
    for (const id of book.completed) {
      if (id === EMPTY_QUEST_ID) continue;
      insert(id, QS_END, true);
    }
  }
  return { tabs };
}

/** Every node of a model, depth first. */
export function allNodes(nodes: readonly QuestNode[]): QuestNode[] {
  const out: QuestNode[] = [];
  const walk = (list: readonly QuestNode[]): void => {
    for (const n of list) {
      out.push(n);
      walk(n.children);
    }
  };
  walk(nodes);
  return out;
}

/** The tab (0..3) whose tree holds the node with this id, -1 when none. */
export function tabHolding(model: QuestTreeModel, id: number): number {
  return model.tabs.findIndex((t) => allNodes(t).some((n) => n.id === id));
}
