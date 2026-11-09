// The public face of the quest feature (`questOf( ctx )`), in a module of its own so that the features that only READ it (the texts' join line, the render layer's NPC marks, the navigator's arrow)
// import it without the module.

import type { ClientContext } from '../../app/context';
import type { QuestClientDef } from '../../gen/bundles';
import type { QuestRecord } from './book';
import type { QuestSenders } from './senders';
import type { QuestStore } from './store';

export interface QuestFeature {
  readonly store: QuestStore;
  readonly senders: QuestSenders;
  /**
   * PL-148: the quest data is loaded and not empty. Decided once per session (one executable: the data cannot change at run time); `false` before the `quests` bundle arrived and on the shipped,
   * quest-less data. While `false` nothing of the quest UI exists: no applet, no quick info, no mark, no join line, no dialog hook.
   */
  available(): boolean;
  /** The definition of a quest (`undefined` while unavailable or for an unknown id). */
  def(questId: number): QuestClientDef | undefined;
  /** The active record of a quest (`FindQuest`). */
  record(questId: number): QuestRecord | undefined;
  /** `m_nQuestEmoticonIndex` of an NPC object: 1 new, 2 in progress, 3 completable, 4 near level, -1 none (the literal V15 loop of `markers.ts`). */
  emoticonOf(objId: number): number;
  /** The quest can be completed now (`__IsEndQuestCondition`, display only): the yellow entry of the dialog list. */
  completable(questId: number): boolean;
  /** The titles of the active defined quests that are not finished, in book order (the join lines; ids `0xffff` and `0xff` are skipped). */
  joinTitles(): string[];
  /** Calls `fn` whenever the book or a fact the marks read changed; returns the unsubscribe. */
  onChange(fn: () => void): () => void;
}

const features = new WeakMap<ClientContext, QuestFeature>();

/** The quest feature of a context; `undefined` before the module registered. */
export function questOf(ctx: ClientContext): QuestFeature | undefined {
  return features.get(ctx);
}

/** Called by the module once it exists. */
export function setQuestOf(ctx: ClientContext, feature: QuestFeature): void {
  features.set(ctx, feature);
}
