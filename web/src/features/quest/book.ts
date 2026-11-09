// The client mirror of the quest book (Plan/45 §28.1, §2.3 `questStore`; Plan/20 §9.12; WP-CFEAT-42): pure reducers over the lists of the own character - the active records, the completed
// ids, the ticked ids (the quick-info list), the guild quests, the timed text and the memory of the quest windows (`CTreeInformationManager`). Nothing here knows a window, a store or the
// network: the module applies the messages with these functions and tells the windows.
//
//   SpawnSelf           `bookFromSelf`: replaces all lists (the join batch of every world)
//   SetQuest            `applySetQuest`: `CMover::SetQuest` / `__SetQuest` (`MoverParam.cpp:3540-3645`): an id that is completed and not active is IGNORED; an unknown id is appended (also with
//                       state 14: it then stays an active record, the original); an active id is overwritten, and with state 14 it leaves `active` and `checked`, joins `completed` unless
//                       the definition says `repeat` (no definition: not added, client KEEP), its quick-tree entry is deleted and the destination is cleared (`ended`)
//   RemoveQuestNtf      `applyRemove`: -1 / 0 one quest (`CMover::RemoveQuest`: active, completed and checked), 1 all (`RemoveAllQuest` + `DeleteAllTreeInformation`), 2 the completed list
//   QuestChecked        `applyChecked`: the ticked list replaced
//   SetGuildQuest / RemoveGuildQuest / QuestTextTime   `applyGuildQuest`, `applyGuildQuestRemoved`, `applyTextTime`
//
// The timer word is kept as sent (`time & 0x7fff` minutes, bit 15 = frozen): the server's first `SetQuest` of a timed quest carries the limit since PL-160, so the original's overwrite of a
// new record's timer by the property's limit is gone. There is no 300 cap (PL-160) and no 100 cap (the server's).

import type { PlayerSelf, Quest } from '../../gen/proto/flyff/v1/mover_types_pb';

/** `QS_END`: the quest is finished. */
export const QS_END = 14;
/** `MAX_CHECKED_QUEST` (`CWndQuest::MAX_CHECK_NUMBER`). */
export const MAX_CHECKED = 5;
/** The value of an empty slot of the wire (`0xffff`). */
export const EMPTY_QUEST_ID = 0xffff;

/** `CTreeInformationManager::QuestListGroup`: which records the list shows. */
export const QUEST_GROUP = { All: 0, Current: 1, Complete: 2 } as const;
export type QuestGroup = (typeof QUEST_GROUP)[keyof typeof QUEST_GROUP];

/** One active record (`QUEST`). */
export interface QuestRecord {
  readonly id: number;
  readonly state: number;
  /** `m_wTime & 0x7fff`: the minutes left. */
  readonly timeLeft: number;
  /** `m_wTime & 0x8000`: the timer is frozen (the end conditions held at the last check). */
  readonly frozen: boolean;
  readonly kills: readonly [number, number];
  readonly patrol: boolean;
  readonly dialog: boolean;
}

/** `QuestTextTime`: the timed text of a state; `endMs` is in the clock of `ctx.now()`. */
export interface TextTime {
  readonly flag: boolean;
  readonly state: number;
  readonly endMs: number;
}

/** The memory of the quest windows (`CTreeInformationManager`). */
export interface TreeMemory {
  /** The open state of the quick-info node of a quest (`CTreeInformation`; an absent id is open: `m_bOpen( TRUE )`). */
  readonly open: ReadonlyMap<number, boolean>;
  /** `m_nSelectedTabNumber`. */
  readonly tab: number;
  /** `m_eQuestListGroup`. */
  readonly group: QuestGroup;
}

export interface QuestBook {
  /** The active records in arrival order. */
  readonly active: readonly QuestRecord[];
  readonly completed: readonly number[];
  /** The ticked quests (`m_aCheckedQuest`), at most `MAX_CHECKED`. */
  readonly checked: readonly number[];
  readonly guildQuests: ReadonlyMap<number, number>;
  readonly textTime: TextTime | null;
  readonly tree: TreeMemory;
  /** Bumps whenever a list changed (the windows re-render on it). */
  readonly version: number;
}

export function initialTree(): TreeMemory {
  return { open: new Map(), tab: 0, group: QUEST_GROUP.Current };
}

export function initialBook(): QuestBook {
  return {
    active: [],
    completed: [],
    checked: [],
    guildQuests: new Map(),
    textTime: null,
    tree: initialTree(),
    version: 0,
  };
}

/** `QUEST` of the wire. */
export function recordOf(
  q: Pick<Quest, 'questId' | 'state' | 'time' | 'killNpcNum' | 'patrol' | 'dialog'>,
): QuestRecord {
  return {
    id: q.questId,
    state: q.state,
    timeLeft: q.time & 0x7fff,
    frozen: (q.time & 0x8000) !== 0,
    kills: [q.killNpcNum[0] ?? 0, q.killNpcNum[1] ?? 0],
    patrol: q.patrol,
    dialog: q.dialog,
  };
}

/** The timer word of a record (`m_wTime`). */
export function timeWord(r: Pick<QuestRecord, 'timeLeft' | 'frozen'>): number {
  return (r.timeLeft & 0x7fff) | (r.frozen ? 0x8000 : 0);
}

const keep = (ids: readonly number[]): number[] => ids.filter((id) => id !== EMPTY_QUEST_ID);

/** `SpawnSelf` (own): all lists replaced; the memory of the windows and the guild quests stay. */
export function bookFromSelf(
  book: QuestBook,
  self: Pick<PlayerSelf, 'quests' | 'completeQuests' | 'checkedQuests'> | undefined,
): QuestBook {
  if (!self) return book;
  return {
    ...book,
    active: self.quests.filter((q) => q.questId !== EMPTY_QUEST_ID).map(recordOf),
    completed: keep(self.completeQuests),
    checked: keep(self.checkedQuests).slice(0, MAX_CHECKED),
    version: book.version + 1,
  };
}

export type SetQuestOutcome =
  /** A completed quest that is not active: `SetQuest` returns FALSE. */
  | 'ignored'
  /** An unknown id was appended. */
  | 'added'
  /** An active record was overwritten. */
  | 'updated'
  /** An active record reached state 14: it left `active` and `checked`; the destination is cleared. */
  | 'ended';

export interface SetQuestResult {
  readonly book: QuestBook;
  readonly outcome: SetQuestOutcome;
}

function without<T>(list: readonly T[], x: T): T[] {
  return list.filter((v) => v !== x);
}

function withoutOpen(tree: TreeMemory, id: number): TreeMemory {
  if (!tree.open.has(id)) return tree;
  const open = new Map(tree.open);
  open.delete(id);
  return { ...tree, open };
}

/** `CMover::SetQuest( &quest )`; `repeat` is the definition's `SetRepeat` (`undefined` = no definition). */
export function applySetQuest(
  book: QuestBook,
  quest: QuestRecord,
  repeat: boolean | undefined,
): SetQuestResult {
  const at = book.active.findIndex((r) => r.id === quest.id);
  if (at < 0) {
    if (book.completed.includes(quest.id)) return { book, outcome: 'ignored' };
    return {
      book: { ...book, active: [...book.active, quest], version: book.version + 1 },
      outcome: 'added',
    };
  }
  if (quest.state === QS_END) {
    const active = book.active.filter((r) => r.id !== quest.id);
    // a repeatable quest never joins the completed list; neither does a quest without a definition (client KEEP)
    const completed =
      repeat === false && !book.completed.includes(quest.id)
        ? [...book.completed, quest.id]
        : book.completed;
    return {
      book: {
        ...book,
        active,
        completed,
        checked: without(book.checked, quest.id),
        tree: withoutOpen(book.tree, quest.id),
        version: book.version + 1,
      },
      outcome: 'ended',
    };
  }
  const active = book.active.slice();
  active[at] = quest;
  return { book: { ...book, active, version: book.version + 1 }, outcome: 'updated' };
}

/** `RemoveQuestNtf`: mode -1 / 0 one quest, 1 everything, 2 the completed list (the tree memory stays). */
export function applyRemove(book: QuestBook, mode: number, questId: number): QuestBook {
  switch (mode) {
    case -1:
    case 0:
      return {
        ...book,
        active: book.active.filter((r) => r.id !== questId),
        completed: without(book.completed, questId),
        checked: without(book.checked, questId),
        tree: withoutOpen(book.tree, questId),
        version: book.version + 1,
      };
    case 1:
      // `RemoveAllQuest` + `DeleteAllTreeInformation`: the selected tab back to 0, the group back to CURRENT
      return {
        ...book,
        active: [],
        completed: [],
        checked: [],
        tree: initialTree(),
        version: book.version + 1,
      };
    case 2:
      return { ...book, completed: [], version: book.version + 1 };
    default:
      return book;
  }
}

/** `QuestChecked`: the ticked list replaced. */
export function applyChecked(book: QuestBook, ids: readonly number[]): QuestBook {
  return { ...book, checked: keep(ids).slice(0, MAX_CHECKED), version: book.version + 1 };
}

/** `SetGuildQuest`: no display (the original stored it in the guild). */
export function applyGuildQuest(book: QuestBook, questId: number, state: number): QuestBook {
  const guildQuests = new Map(book.guildQuests);
  guildQuests.set(questId, state);
  return { ...book, guildQuests, version: book.version + 1 };
}

/** `RemoveGuildQuest`. */
export function applyGuildQuestRemoved(book: QuestBook, questId: number): QuestBook {
  if (!book.guildQuests.has(questId)) return book;
  const guildQuests = new Map(book.guildQuests);
  guildQuests.delete(questId);
  return { ...book, guildQuests, version: book.version + 1 };
}

/** `QuestTextTime`: `endMs = now + time`; a false flag clears it. Nothing is drawn (the release client drew it only under `__INTERNALSERVER`). */
export function applyTextTime(
  book: QuestBook,
  flag: boolean,
  state: number,
  timeMs: number,
  nowMs: number,
): QuestBook {
  return {
    ...book,
    textTime: flag ? { flag, state, endMs: nowMs + timeMs } : null,
    version: book.version + 1,
  };
}

// ---- the memory of the windows ------------------------------------------------------------------------------------------------------

/** `CTreeInformation::GetOpen` of a quest (absent = open). */
export function quickOpen(tree: TreeMemory, questId: number): boolean {
  return tree.open.get(questId) ?? true;
}

/** `InsertTreeInformation( id, TRUE )` when the quest has none (a listed active quest, a ticked quest). */
export function rememberQuest(book: QuestBook, questId: number): QuestBook {
  if (book.tree.open.has(questId)) return book;
  const open = new Map(book.tree.open);
  open.set(questId, true);
  return { ...book, tree: { ...book.tree, open }, version: book.version + 1 };
}

/** `CTreeInformation::SetOpen` (an entry is created when the quest has none). */
export function setQuickOpen(book: QuestBook, questId: number, open: boolean): QuestBook {
  if (book.tree.open.get(questId) === open) return book;
  const map = new Map(book.tree.open);
  map.set(questId, open);
  return { ...book, tree: { ...book.tree, open: map }, version: book.version + 1 };
}

/** `DeleteTreeInformation( id )` (an untick). */
export function forgetQuest(book: QuestBook, questId: number): QuestBook {
  if (!book.tree.open.has(questId)) return book;
  return { ...book, tree: withoutOpen(book.tree, questId), version: book.version + 1 };
}

/** `m_nSelectedTabNumber` and `m_eQuestListGroup`. */
export function setListMemory(book: QuestBook, tab: number, group: QuestGroup): QuestBook {
  if (book.tree.tab === tab && book.tree.group === group) return book;
  return { ...book, tree: { ...book.tree, tab, group } };
}

/** A fresh book for the next session (the character left the world). */
export function resetBook(): QuestBook {
  return initialBook();
}

/** `FindQuest`. */
export function findRecord(book: QuestBook, questId: number): QuestRecord | undefined {
  return book.active.find((r) => r.id === questId);
}
