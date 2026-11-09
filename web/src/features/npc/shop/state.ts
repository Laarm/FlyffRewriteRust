// The shop store (Plan/45 §12, §2.3 `shopStore`; WP-CFEAT-19): the four stock tabs of the vendor the player talks to. The
// stock is infinite (the server never decrements it, Plan/19 §7.1); `Vendor` and `UpdateVendor` exist in the protocol but the
// server never sends them. A tab container is `max` slots whose `index` names the storage entry shown in each slot.

import type { StoreApi } from 'zustand';
import type { ItemContainer as WireContainer } from '../../../gen/proto/flyff/v1/item_types_pb';
import type { StoreRegistry } from '../../../stores/registry';
import { itemFromWire, type InvItem } from '../../inventory/items';

/** One stock entry of a tab. */
export interface ShopStockEntry {
  /** The storage id inside the tab container (what `BuyItem.obj_id` carries). */
  readonly objId: number;
  readonly item: InvItem;
}

export interface ShopState {
  readonly open: boolean;
  readonly npcId: number;
  /** The `character.inc` key of the vendor. */
  readonly npcKey: string;
  /** The four tabs; the entries in slot order (`CWndItemCtrl::OnDraw` walks the container this way). */
  readonly tabs: readonly (readonly ShopStockEntry[])[];
  readonly version: number;
}

export const initialShopState = (): ShopState => ({
  open: false,
  npcId: 0,
  npcKey: '',
  tabs: [[], [], [], []],
  version: 0,
});

/**
 * `CWndShop::OnDestroy`: the shop is gone (closed and empty). The version goes UP, never back to 0: a read model keyed on
 * it must not meet an old number again with the next shop (bug #012).
 */
export const closedShopState = (s: ShopState): ShopState => ({
  ...initialShopState(),
  version: s.version + 1,
});

export type ShopStore = StoreApi<ShopState>;

/** Scope `world`: a shop does not survive a world change. */
export function defineShopStore(registry: StoreRegistry): ShopStore {
  const existing = registry.get('shop');
  if (existing) return existing as unknown as ShopStore;
  return registry.defineStore<ShopState>({
    name: 'shop',
    scope: 'world',
    owner: '45',
    initial: initialShopState,
  });
}

/** The entries of a tab container in slot order: `index[slot]` names the storage entry shown in `slot` (`-1` = empty). */
export function parseTab(c: Pick<WireContainer, 'max' | 'index' | 'items'>): ShopStockEntry[] {
  const byId = new Map<number, InvItem>();
  for (const w of c.items) byId.set(w.objId, itemFromWire(w));
  const out: ShopStockEntry[] = [];
  const seen = new Set<number>();
  for (let slot = 0; slot < c.max; slot += 1) {
    const id = c.index[slot];
    if (id === undefined || id < 0 || seen.has(id)) continue;
    const item = byId.get(id);
    if (!item) continue;
    seen.add(id);
    out.push({ objId: id, item });
  }
  return out;
}

/** `OnOpenShopWnd`: the four tabs of the vendor; fewer than four containers leave the rest empty. */
export function applyOpenShop(
  s: ShopState,
  npcId: number,
  npcKey: string,
  containers: readonly WireContainer[],
): ShopState {
  const tabs = [0, 1, 2, 3].map((i) => {
    const c = containers[i];
    return c ? parseTab(c) : [];
  });
  return { open: true, npcId, npcKey, tabs, version: s.version + 1 };
}

/** `Vendor`: a restock replaces the tabs of the open shop of that NPC. */
export function applyVendor(
  s: ShopState,
  npcId: number,
  containers: readonly WireContainer[],
): ShopState {
  if (!s.open || s.npcId !== npcId) return s;
  return applyOpenShop(s, npcId, s.npcKey, containers);
}

/** `UpdateVendor`: one entry changed; a count of 0 removes it. */
export function applyUpdateVendor(
  s: ShopState,
  npcId: number,
  tab: number,
  objId: number,
  num: number,
): ShopState {
  if (!s.open || s.npcId !== npcId) return s;
  const entries = s.tabs[tab];
  if (!entries) return s;
  const next =
    num <= 0
      ? entries.filter((e) => e.objId !== objId)
      : entries.map((e) => (e.objId === objId ? { ...e, item: { ...e.item, num } } : e));
  const tabs = s.tabs.map((t, i) => (i === tab ? next : t));
  return { ...s, tabs, version: s.version + 1 };
}
