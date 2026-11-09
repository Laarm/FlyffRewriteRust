// `tradeStore` (Plan/45 §2.3, §14; WP-CFEAT-21): the client mirror of a player-to-player trade (`m_vtInfo` of the two movers: the other mover, the 25
// offered items and the offered penya of each side) plus the dialog state of the legacy windows (who pressed OK, the final confirmation).
// The server keeps the offers as reservations in the owners' inventories and sends each put with the full item (PL-063), so the store needs no
// copy of the partner's inventory. Scope `world`: a world change ends a trade (the server cancels it, the client closes its windows).

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';
import type { InvItem } from '../inventory/items';

/** `MAX_TRADE`. */
export const MAX_TRADE = 25;

/** One offered item (`TradeSetItem`). */
export interface TradeOffer {
  /** The object id of the item in the OFFERER's inventory (`nId`). */
  readonly objId: number;
  /** The offered count (`nItemNum`). */
  readonly num: number;
  /** The item with its full data (`TradePutNtf.item`, whose `num` is the offered count). */
  readonly item: InvItem;
}

/** One side of the trade. */
export interface TradeSide {
  /** The offers by slot. */
  readonly slots: readonly (TradeOffer | null)[];
  /** The penya offered so far (the running total of `TradePutGoldNtf`). */
  readonly gold: number;
  /** This side pressed OK (or both did: `TradeLastConfirm`). */
  readonly ok: boolean;
}

/** The final confirmation window: not open, open (Yes / No), or answered (the wait text). */
export type FinalConfirm = 'none' | 'open' | 'waiting';

export interface TradeState {
  /** Bumps with every change (the read model keys on it). */
  readonly version: number;
  /** A trade session exists (`g_pPlayer->m_vtInfo.GetOther() != NULL` with the link back). */
  readonly active: boolean;
  /** The object id of the partner (the header of `TradeNtf`). */
  readonly partnerObj: number;
  /** The partner's name when the trade opened (the entity may disappear before the trade ends). */
  readonly partnerName: string;
  readonly me: TradeSide;
  readonly you: TradeSide;
  readonly final: FinalConfirm;
}

export const emptySide = (): TradeSide => ({
  slots: Array.from({ length: MAX_TRADE }, () => null),
  gold: 0,
  ok: false,
});

export function initialTradeState(): TradeState {
  return {
    version: 0,
    active: false,
    partnerObj: 0,
    partnerName: '',
    me: emptySide(),
    you: emptySide(),
    final: 'none',
  };
}

export type TradeStore = StoreApi<TradeState>;

/** The store of a registry (created on first use). */
export function defineTradeStore(registry: StoreRegistry): TradeStore {
  const existing = registry.get('trade');
  if (existing) return existing as unknown as TradeStore;
  return registry.defineStore<TradeState>({
    name: 'trade',
    scope: 'world',
    owner: '45',
    initial: initialTradeState,
  });
}
