// `questStore` (Plan/45 §2.3, §28.1; WP-CFEAT-42): the quest book of the own character (`book.ts`) as a registry store. Registry name `quest`, scope `session` (the lists are replaced by the
// `SpawnSelf` of every world; the memory of the windows is reset when the session leaves the world and by `RemoveQuestNtf{1}`), owner `45`.

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';
import { initialBook, type QuestBook } from './book';

export type QuestStore = StoreApi<QuestBook>;

/** The store of a registry (created on first use). */
export function defineQuestStore(registry: StoreRegistry): QuestStore {
  const existing = registry.get('quest');
  if (existing) return existing as unknown as QuestStore;
  return registry.defineStore<QuestBook>({
    name: 'quest',
    scope: 'session',
    owner: '45',
    initial: initialBook,
  });
}
