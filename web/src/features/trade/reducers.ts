// The reducers of the trade messages (Plan/45 §14.2; `CDPClient::OnTrade*`, `src:Neuz/DPClient.cpp:2527-2860`): pure functions of the state and
// the message. The module turns the difference between two states into windows, reservations and lines.

import type { InvItem } from '../inventory/items';
import {
  emptySide,
  initialTradeState,
  MAX_TRADE,
  type TradeOffer,
  type TradeSide,
  type TradeState,
} from './store';

const bump = (s: TradeState, patch: Partial<TradeState>): TradeState => ({
  ...s,
  ...patch,
  version: s.version + 1,
});

const withSlot = (side: TradeSide, slot: number, offer: TradeOffer | null): TradeSide => {
  const slots = side.slots.slice();
  slots[slot] = offer;
  return { ...side, slots };
};

/** `OnTrade`: a session opens with the partner (both sides empty). */
export function onTrade(s: TradeState, partnerObj: number, partnerName: string): TradeState {
  return { ...initialTradeState(), version: s.version + 1, active: true, partnerObj, partnerName };
}

/**
 * `OnTradePut`: `isMe` = the header object is the own mover. A put outside the 25 slots, or without a session, is ignored (the original needs the
 * mutual `GetOther()` link).
 */
export function onTradePut(
  s: TradeState,
  isMe: boolean,
  slot: number,
  offer: { objId: number; num: number; item: InvItem },
): TradeState {
  if (!s.active || slot < 0 || slot >= MAX_TRADE) return s;
  const side = isMe ? s.me : s.you;
  const next = withSlot(side, slot, offer);
  return bump(s, isMe ? { me: next } : { you: next });
}

/** `OnTradePull`: the slot is cleared. */
export function onTradePull(s: TradeState, isMe: boolean, slot: number): TradeState {
  if (!s.active || slot < 0 || slot >= MAX_TRADE) return s;
  const side = isMe ? s.me : s.you;
  if (!side.slots[slot]) return s;
  const next = withSlot(side, slot, null);
  return bump(s, isMe ? { me: next } : { you: next });
}

/** `OnTradePutGold`: the TOTAL offered so far (PL-062; the legacy sent the last amount). */
export function onTradePutGold(s: TradeState, isMe: boolean, gold: number): TradeState {
  if (!s.active) return s;
  return bump(s, isMe ? { me: { ...s.me, gold } } : { you: { ...s.you, gold } });
}

/** `OnTradeOk`: that side's OK is checked and disabled. */
export function onTradeOk(s: TradeState, isMe: boolean): TradeState {
  if (!s.active) return s;
  return bump(s, isMe ? { me: { ...s.me, ok: true } } : { you: { ...s.you, ok: true } });
}

/** `OnTradelastConfirm`: both OK buttons are checked and disabled and the final confirmation opens. */
export function onTradeLastConfirm(s: TradeState): TradeState {
  if (!s.active) return s;
  return bump(s, { me: { ...s.me, ok: true }, you: { ...s.you, ok: true }, final: 'open' });
}

/** `OnTradelastConfirmOk` of the own player: the confirmation shows the wait text. */
export function onTradeLastConfirmOk(s: TradeState): TradeState {
  if (!s.active || s.final === 'none') return s;
  return bump(s, { final: 'waiting' });
}

/** The session is over (consent, cancel, a window that closed, the partner left): everything is empty again. */
export function endTrade(s: TradeState): TradeState {
  if (!s.active && s.final === 'none') return s;
  return { ...initialTradeState(), version: s.version + 1 };
}

/** The object ids of the own offers (the items whose reservation the end of the trade lifts). */
export function offeredObjIds(s: TradeState): number[] {
  const out: number[] = [];
  for (const o of s.me.slots) if (o) out.push(o.objId);
  return out;
}

/** The slot of the own side that is still free, `-1` when all 25 are taken. */
export function freeSlot(side: TradeSide): number {
  return side.slots.findIndex((o) => o === null);
}

export { emptySide };
