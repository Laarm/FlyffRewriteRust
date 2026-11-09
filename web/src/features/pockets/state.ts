// `pocketStore` (Plan/45 §9.7, Plan/19 §3.5; WP-CFEAT-29): the three pockets of the character - the basic pocket and the two extra bags - as the server's
// `ItemContainer`s (no equipment), the availability flags and the expiry. The reducers are pure functions over an immutable state value (they clone the
// container they change); the handlers of `pocketsModule.ts` call them with the decoded messages.
//
//   SpawnSelf.self.pockets   `CPocketController::Serialize`: three slots           (`applySelf`)
//   PocketAttribute          `SetAttribute(attribute, pocket, data)`: 1 = `Avail` for `data` seconds, 0 = `SetExpired(data)`   (`applyAttribute`)
//   PocketAddItem            the client's own `Add` on the pocket                    (`applyAdd`)
//   PocketRemoveItem         `RemoveAtId(pocket, objId, num)`                        (`applyRemove`)

import type { StoreApi } from 'zustand/vanilla';
import type { PocketSet } from '../../gen/proto/flyff/v1/item_types_pb';
import type { StoreRegistry } from '../../stores/registry';
import { ItemContainer } from '../inventory/container';
import { type InvItem, itemFromWire } from '../inventory/items';

/** `MAX_POCKET`. */
export const POCKET_COUNT = 3;
/** `CPocketController::expired` and `avail` of `PocketAttribute.attribute`. */
export const ATTR_EXPIRED = 0;
export const ATTR_AVAIL = 1;

/** The slots of pocket `index`: 18 / 42 / 42 with `features.bigger_pocket`, else 6 / 24 / 24 (`flyff_rules::item::buffitem::pocket_capacity`). */
export function pocketCapacity(index: number, biggerPocket: boolean): number {
  if (index === 0) return biggerPocket ? 18 : 6;
  if (index === 1 || index === 2) return biggerPocket ? 42 : 24;
  return 0;
}

/**
 * `CPocketController::IsAvailable(index)`: with `features.perm_extrabags` every pocket always; else pocket 0 always, pockets 1 and 2 while they exist and
 * have not expired (`flyff_rules::item::buffitem::pocket_available`).
 */
export function pocketAvailable(
  permExtrabags: boolean,
  index: number,
  exists: boolean,
  expired: boolean,
): boolean {
  if (index < 0 || index >= POCKET_COUNT) return false;
  return permExtrabags || index === 0 || (exists && !expired);
}

export interface PocketState {
  /** `m_apPocket[i] != NULL` (the server's `exists`). */
  readonly exists: boolean;
  /** `m_bExpired`. */
  readonly expired: boolean;
  /** `m_tExpirationDate` (unix seconds), `0` = never. */
  readonly expireAt: number;
  readonly container: ItemContainer<InvItem>;
}

export interface PocketsState {
  /** The pockets were received (`SpawnSelf`). */
  readonly ready: boolean;
  readonly pockets: readonly PocketState[];
  /** Bumps with every change. */
  readonly version: number;
}

/** The capacity of pocket `i` for a container without a size on the wire. */
export type CapacityOf = (index: number) => number;

const emptyPocket = (capacity: number, exists: boolean): PocketState => ({
  exists,
  expired: !exists,
  expireAt: 0,
  container: new ItemContainer<InvItem>(capacity, 0),
});

export function initialPocketsState(
  capacityOf: CapacityOf = (i) => pocketCapacity(i, true),
): PocketsState {
  return {
    ready: false,
    pockets: Array.from({ length: POCKET_COUNT }, (_, i) => emptyPocket(capacityOf(i), i === 0)),
    version: 0,
  };
}

export type PocketsStore = StoreApi<PocketsState>;

/** Scope `world`: the join batch of every world brings the pockets again. */
export function definePocketsStore(registry: StoreRegistry): PocketsStore {
  const existing = registry.get('pockets');
  if (existing) return existing as unknown as PocketsStore;
  return registry.defineStore<PocketsState>({
    name: 'pockets',
    scope: 'world',
    owner: '45',
    initial: () => initialPocketsState(),
  });
}

/** `SpawnSelf.self.pockets`: the whole set. */
export function applySelf(
  s: PocketsState,
  set: PocketSet | undefined,
  capacityOf: CapacityOf,
): PocketsState {
  const pockets = Array.from({ length: POCKET_COUNT }, (_, i): PocketState => {
    const slot = set?.slots[i];
    const wire = slot?.items;
    const capacity = wire && wire.max > 0 ? wire.max : capacityOf(i);
    const container = wire
      ? ItemContainer.fromWire(
          {
            max: capacity,
            index: wire.index,
            objIndex: wire.objIndex,
            items: wire.items.map(itemFromWire),
          },
          0,
        )
      : new ItemContainer<InvItem>(capacity, 0);
    return {
      // pocket 0 is always there (`Avail( 0 )` in the constructor)
      exists: (slot?.exists ?? false) || i === 0,
      expired: slot?.expired ?? false,
      expireAt: Number(slot?.expireAt ?? 0n),
      container,
    };
  });
  return { ready: true, pockets, version: s.version + 1 };
}

const inRange = (pocket: number): boolean =>
  Number.isInteger(pocket) && pocket >= 0 && pocket < POCKET_COUNT;

function withPocket(s: PocketsState, index: number, next: PocketState): PocketsState {
  const pockets = s.pockets.slice();
  pockets[index] = next;
  return { ...s, pockets, version: s.version + 1 };
}

/**
 * `OnPocketAttribute` -> `CPocketController::SetAttribute`: `avail` adds `data` seconds (`Avail( pocket, t )`: an expired or new bag counts from now, a
 * running one from its end; `0` seconds = no expiry) and clears the expired flag; `expired` sets the flag of a bag that exists. Other attributes are ignored.
 */
export function applyAttribute(
  s: PocketsState,
  attribute: number,
  pocket: number,
  data: number,
  nowSec: number,
  capacityOf: CapacityOf,
): PocketsState {
  if (!inRange(pocket)) return s;
  const cur = s.pockets[pocket];
  if (!cur) return s;
  if (attribute === ATTR_AVAIL) {
    let expireAt: number;
    if (cur.exists) expireAt = cur.expired ? nowSec + data : cur.expireAt + data;
    else expireAt = nowSec + data;
    if (data === 0) expireAt = 0;
    const container = cur.exists
      ? cur.container
      : new ItemContainer<InvItem>(capacityOf(pocket), 0);
    return withPocket(s, pocket, { exists: true, expired: false, expireAt, container });
  }
  if (attribute === ATTR_EXPIRED) {
    if (!cur.exists) return s;
    const expired = data !== 0;
    if (cur.expired === expired) return s;
    return withPocket(s, pocket, { ...cur, expired });
  }
  return s;
}

/**
 * `OnPocketAddItem`: `m_Pocket.Add( pocket, &item )`. The item of the message carries the count that was put and, as `obj_id`, the first entry the server
 * touched; re-running `Add` on the mirror of the pocket places it the same way. `packMax` is the item's `dwPackMax`. A pocket that cannot be used (not
 * available) ignores the item, as `IsAvailable( pocket )` does.
 */
export function applyAdd(
  s: PocketsState,
  pocket: number,
  item: InvItem,
  packMax: number,
  usable: boolean,
): PocketsState {
  if (!inRange(pocket) || !usable) return s;
  const cur = s.pockets[pocket];
  if (!cur) return s;
  const c = cur.container.clone();
  if (c.add(item, packMax) === null) return s;
  return withPocket(s, pocket, { ...cur, container: c });
}

/**
 * `OnPocketRemoveItem` -> `RemoveAtId( pocket, objId, num )`: the entry loses `num` pieces, or goes when it has no more than that.
 */
export function applyRemove(
  s: PocketsState,
  pocket: number,
  objId: number,
  num: number,
  usable: boolean,
): PocketsState {
  if (!inRange(pocket) || !usable) return s;
  const cur = s.pockets[pocket];
  const it = cur?.container.getAtId(objId);
  if (!cur || !it) return s;
  const c = cur.container.clone();
  if (it.num > num) c.replaceAtId(objId, { ...it, num: it.num - num });
  else c.removeAtId(objId);
  return withPocket(s, pocket, { ...cur, container: c });
}
