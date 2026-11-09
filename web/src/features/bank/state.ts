// `bankStore` (Plan/45 §2.3, §13; WP-CFEAT-20): the three bank compartments of the account as the server's `ItemContainer`s (42 slots each, no
// equipment), their penya and the character ids, the account slot of the logged-in character. The reducers are pure functions over an immutable
// state value (they clone the container they change); the handlers of `bankModule.ts` call them with the decoded messages.
//
//   SpawnSelf.self        `banks`, `gold_bank`, `id_player_bank`, `slot`          (`applySelf`)
//   PutItemBank           the client's own `Add` on the compartment                (`applyPutItem`)
//   UpdateBankItem        `UI_NUM`: the remaining count, 0 removes the entry       (`applyUpdateItem`)
//   PutGoldBank           the balance of one compartment (the wallet is the self store's) (`applyPutGold`)
//
// `GetItemBank` changes nothing here: the bag side of a withdrawal is a `CreateItem` and the compartment side an `UpdateBankItem`.

import type { StoreApi } from 'zustand/vanilla';
import type { ItemContainer as WireContainer } from '../../gen/proto/flyff/v1/item_types_pb';
import type { PlayerSelf } from '../../gen/proto/flyff/v1/mover_types_pb';
import type { StoreRegistry } from '../../stores/registry';
import { ItemContainer } from '../inventory/container';
import { itemFromWire, type InvItem, UI } from '../inventory/items';

/** The account has three character slots, and so the bank three compartments of 42 items. */
export const BANK_SLOTS = 3;
export const BANK_CAPACITY = 42;

export interface BankCompartmentState {
  readonly container: ItemContainer<InvItem>;
  /** `m_dwGoldBank[i]`. */
  readonly gold: number;
  /** `m_idPlayerBank[i]`: the character in that slot of the account, `0` = none. */
  readonly characterId: number;
}

export interface BankState {
  /** The compartments were received (`SpawnSelf`). */
  readonly ready: boolean;
  /** `m_nSlot`: the account slot of the logged-in character (its own compartment). */
  readonly slot: number;
  readonly compartments: readonly BankCompartmentState[];
  /** Bumps with every change. */
  readonly version: number;
}

const emptyCompartment = (): BankCompartmentState => ({
  container: new ItemContainer<InvItem>(BANK_CAPACITY, 0),
  gold: 0,
  characterId: 0,
});

export function initialBankState(): BankState {
  return {
    ready: false,
    slot: 0,
    compartments: Array.from({ length: BANK_SLOTS }, emptyCompartment),
    version: 0,
  };
}

export type BankStore = StoreApi<BankState>;

/** Scope `world`: the join batch of every world brings the banks again. */
export function defineBankStore(registry: StoreRegistry): BankStore {
  const existing = registry.get('bank');
  if (existing) return existing as unknown as BankStore;
  return registry.defineStore<BankState>({
    name: 'bank',
    scope: 'world',
    owner: '45',
    initial: initialBankState,
  });
}

function containerOf(wire: WireContainer | undefined): ItemContainer<InvItem> {
  if (!wire || wire.max <= 0) return new ItemContainer<InvItem>(BANK_CAPACITY, 0);
  return ItemContainer.fromWire(
    {
      max: wire.max,
      index: wire.index,
      objIndex: wire.objIndex,
      items: wire.items.map(itemFromWire),
    },
    0,
  );
}

/** `SpawnSelf.self`: the whole bank. */
export function applySelf(s: BankState, self: PlayerSelf | undefined): BankState {
  if (!self) return { ...initialBankState(), version: s.version + 1 };
  return {
    ready: true,
    slot: self.slot,
    compartments: Array.from({ length: BANK_SLOTS }, (_, i) => ({
      container: containerOf(self.banks[i]),
      gold: self.goldBank[i] ?? 0,
      characterId: self.idPlayerBank[i] ?? 0,
    })),
    version: s.version + 1,
  };
}

const inRange = (slot: number): boolean => Number.isInteger(slot) && slot >= 0 && slot < BANK_SLOTS;

function withCompartment(s: BankState, slot: number, next: BankCompartmentState): BankState {
  const compartments = s.compartments.slice();
  compartments[slot] = next;
  return { ...s, compartments, version: s.version + 1 };
}

/**
 * `OnPutItemBank`: `m_Bank[slot].Add(&itemElem)`. The item of the message carries the count that was put and, as `obj_id`, the first entry the
 * server touched; re-running `Add` on the mirror of the compartment places it the same way. `packMax` is the item's `dwPackMax`.
 */
export function applyPutItem(
  s: BankState,
  slot: number,
  item: InvItem,
  packMax: number,
): BankState {
  if (!inRange(slot)) return s;
  const comp = s.compartments[slot];
  if (!comp) return s;
  const c = comp.container.clone();
  if (c.add(item, packMax) === null) return s;
  return withCompartment(s, slot, { ...comp, container: c });
}

/**
 * `OnUpdateBankItem` -> `CMover::UpdateItemBank`: `UI_NUM` sets the count of the entry, `0` removes it. Other parameters are never sent and are
 * ignored.
 */
export function applyUpdateItem(
  s: BankState,
  slot: number,
  objId: number,
  param: number,
  value: number,
): BankState {
  if (!inRange(slot) || param !== UI.NUM) return s;
  const comp = s.compartments[slot];
  const it = comp?.container.getAtId(objId);
  if (!comp || !it) return s;
  const c = comp.container.clone();
  if ((value | 0) <= 0) c.removeAtId(objId);
  else c.replaceAtId(objId, { ...it, num: value & 0xffff });
  return withCompartment(s, slot, { ...comp, container: c });
}

/** `OnPutGoldBank`: `m_dwGoldBank[slot] = bank_gold` (the wallet `gold` is applied to the self store by the module). */
export function applyPutGold(s: BankState, slot: number, bankGold: number): BankState {
  if (!inRange(slot)) return s;
  const comp = s.compartments[slot];
  if (!comp || comp.gold === bankGold) return s;
  return withCompartment(s, slot, { ...comp, gold: bankGold });
}
