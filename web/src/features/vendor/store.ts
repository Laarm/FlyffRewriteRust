// `vendorStore` (Plan/45 §2.3, §15; WP-CFEAT-22): the client mirror of a personal shop - the own one (`m_vtInfo` of the own mover: the title, the
// registered goods, whether it is open), the one that is visited (`CMover::m_vtInfo.VendorCopyItems`), the list of goods the window re-registers
// next time (`g_Neuz.m_aSavedInven`) and the chat room of the shop (`g_Chatting`, the lines of `CWndVendorMessage`). Scope `world`: the shop closes
// with a world change (the server ends it).

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';
import type { InvItem } from '../inventory/items';

/** `MAX_VENDITEM`: the slots the server keeps; the window shows the first `MAX_VENDOR_REVISION`. */
export const MAX_VENDITEM = 30;
export const MAX_VENDOR_REVISION = 20;

/** One good of the own shop (`VendorSetItem`): the inventory object, the reserved count and the unit price. */
export interface OwnGood {
  readonly objId: number;
  readonly count: number;
  readonly price: number;
}

/** The good the window registers again next time (`m_aSavedInven`: object, item, count, price). */
export interface SavedGood {
  readonly objId: number;
  readonly itemId: number;
  readonly count: number;
  readonly price: number;
}

/** One good of a visited shop (`PVendorItem.entries`): the full item, the count for sale and the price. */
export interface BrowseGood {
  readonly item: InvItem;
  readonly count: number;
  readonly price: number;
}

/** The shop that is visited. */
export interface Browse {
  /** The vendor's object id. */
  readonly vendor: number;
  /** The vendor's character id (the key of the chat room). */
  readonly vendorId: number;
  readonly title: string;
  readonly goods: readonly (BrowseGood | null)[];
}

export interface ChatMember {
  readonly id: number;
  readonly name: string;
}

/** The chat room of the shop the player is in (`g_Chatting`). */
export interface ChatRoom {
  readonly members: readonly ChatMember[];
  /** The talk page: markup lines. */
  readonly talk: readonly string[];
  /** The info page of the owner: markup lines. */
  readonly info: readonly string[];
  /** `g_Chatting.m_bState`: the room is open to talk. */
  readonly open: boolean;
}

export interface VendorState {
  /** Bumps with every change. */
  readonly version: number;
  /** The title of the own shop (the one the server broadcast, or the one restored after a stop). */
  readonly title: string;
  /** `IsVendorOpen()`. */
  readonly open: boolean;
  /** The own goods by slot. */
  readonly own: readonly (OwnGood | null)[];
  /** The list the window registers again (`ReloadItemList`). */
  readonly saved: readonly (SavedGood | null)[];
  /** The owner confirmed "stop selling": the window comes back in set-up mode when the close arrives. */
  readonly stopping: boolean;
  /** The visited shop, `null` when none (`GetOther()` is a vendor). */
  readonly browse: Browse | null;
  readonly chat: ChatRoom;
}

const empty = <T>(n: number): (T | null)[] => Array.from({ length: n }, () => null);

export const emptyChat = (): ChatRoom => ({ members: [], talk: [], info: [], open: false });

export function initialVendorState(): VendorState {
  return {
    version: 0,
    title: '',
    open: false,
    own: empty<OwnGood>(MAX_VENDITEM),
    saved: empty<SavedGood>(MAX_VENDITEM),
    stopping: false,
    browse: null,
    chat: emptyChat(),
  };
}

export type VendorStore = StoreApi<VendorState>;

/** The store of a registry (created on first use). */
export function defineVendorStore(registry: StoreRegistry): VendorStore {
  const existing = registry.get('vendor');
  if (existing) return existing as unknown as VendorStore;
  return registry.defineStore<VendorState>({
    name: 'vendor',
    scope: 'world',
    owner: '45',
    initial: initialVendorState,
  });
}
