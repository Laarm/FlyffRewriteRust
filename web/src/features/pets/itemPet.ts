// Edits of one inventory item by the pet messages (`Pet`, `PetState`, `PetSetExp`, `PetFeed`, `SetPetName` change the `CPet` inside the item; a level change also its
// item id): the inventory store is replaced with the item replaced, like its own reducers do (`applyExtra` ...). The container is cloned so that the view builders
// see a new version; a worn item is not touched (a pet is never worn).

import type { InventoryStore } from '../inventory';
import type { InvItem } from '../inventory';

/**
 * Replaces the item `objId` by `edit(item)`; `null` from `edit` (or an unknown item) changes nothing. Returns whether the item was replaced.
 */
export function patchInvItem(
  store: InventoryStore,
  objId: number,
  edit: (item: InvItem) => InvItem | null,
): boolean {
  const s = store.getState();
  const cur = s.container.getAtId(objId);
  if (!cur) return false;
  const next = edit(cur);
  if (!next) return false;
  const container = s.container.clone();
  container.replaceAtId(objId, next);
  store.setState({ ...s, container, version: s.version + 1 });
  return true;
}
