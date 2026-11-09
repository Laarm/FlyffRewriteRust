// `petStore` (Plan/45 §18.7, §2.3; WP-CFEAT-27): the handles of the own pets. The data of a pet lives in its inventory item (`InvItem.pet`, the `Pet`, `PetState`,
// `PetSetExp` and `PetFeed` messages edit it); this store holds which items are the summoned ones (`m_dwPetId`, `m_dwEatPetId`, `m_dwVisPetItem` of the own mover).
// Scope `world`: the join batch of every world brings them again (`SpawnSelf.pet_id`, the `PetCall` after the join, `VisPetActivate`).

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';

export interface PetState {
  /** `m_dwPetId`: the inventory object id of the summoned SYSTEM pet item (an egg or a hatched pet), `null` = none. */
  readonly summonedItem: number | null;
  /** `m_dwEatPetId`: the object id of the summoned EAT pet's mover (a vis pet's model); `null` = none. */
  readonly eatPetId: number | null;
  /** `m_dwVisPetItem`: the inventory object id of the activated vis-pet item, `null` = none. */
  readonly visPetItem: number | null;
  /** The `PF_*` flag of the last `Pet` message (`m_pet.GetLevelup()`). */
  readonly lastFlag: number;
  /** Bumps on every change (the views poll it). */
  readonly version: number;
}

export type PetStore = StoreApi<PetState>;

export function initialPetState(): PetState {
  return { summonedItem: null, eatPetId: null, visPetItem: null, lastFlag: 0, version: 0 };
}

/** Defines (or returns) the `pets` store of a registry. */
export function definePetStore(registry: StoreRegistry): PetStore {
  const existing = registry.get('pets');
  if (existing) return existing as unknown as PetStore;
  return registry.defineStore<PetState>({
    name: 'pets',
    scope: 'world',
    owner: '45',
    initial: initialPetState,
  });
}

/** Sets fields and bumps the version (nothing happens when nothing changes). */
export function patchPets(store: PetStore, patch: Partial<Omit<PetState, 'version'>>): void {
  const s = store.getState();
  let changed = false;
  for (const [k, v] of Object.entries(patch)) {
    if ((s as unknown as Record<string, unknown>)[k] !== v) changed = true;
  }
  if (changed) store.setState({ ...s, ...patch, version: s.version + 1 });
}
