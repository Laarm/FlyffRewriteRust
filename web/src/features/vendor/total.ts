// The total-gold rule of the personal shop (`CWndRegVend::OnChildNotify` and `CWndVendor::OnChildNotify`, ref/99 §4.7; Plan/45 §15.1, 19 §9.2 VR-019):
// the penya of the wallet plus everything the shop would earn must stay below 2 100 000 000 (the wallet of the seller must hold the payments). The server
// enforces the same bound at registration (`TID_GAME_VENDOR_MAX_ALL_GOLD`); the client refuses first so that the window can say it in place.

/** The limit of the sum: it must be BELOW this. */
export const VENDOR_GOLD_LIMIT = 2_100_000_000;
/** `INT_MAX`: a unit price is clamped to it. */
export const PRICE_CLAMP = 0x7fff_ffff;

/** One registered good (`m_nCost` and the reserved count). */
export interface Listed {
  readonly price: number;
  readonly count: number;
}

/** The penya a shop would hold when everything sold: the wallet plus the sum of `price x count`. */
export function shopTotal(wallet: number, goods: readonly (Listed | null)[]): number {
  let sum = wallet;
  for (const g of goods) if (g) sum += g.price * g.count;
  return sum;
}

/**
 * `CWndRegVend` `WIDC_OK`: `false` (refuse with `TID_GAME_VENDOR_MAX_ALL_GOLD`) unless `0 < sum + price x quantity < 2 100 000 000` AND the sum grew
 * (`nOldGold >= n64Sum` refuses; with price and quantity of at least 1 that cannot happen, but the original tests it, so the port does too).
 */
export function registrationAllowed(
  wallet: number,
  goods: readonly (Listed | null)[],
  price: number,
  quantity: number,
): boolean {
  const old = shopTotal(wallet, goods);
  const sum = old + price * quantity;
  return !(sum <= 0 || sum >= VENDOR_GOLD_LIMIT || old >= sum);
}

/** `CWndVendor` `WIDC_OK`: the shop may open unless `nGold <= 0 || 2 100 000 000 <= nGold`. */
export function openingAllowed(wallet: number, goods: readonly (Listed | null)[]): boolean {
  const sum = shopTotal(wallet, goods);
  return !(sum <= 0 || sum >= VENDOR_GOLD_LIMIT);
}

/** The unit price of the typed digits: an `int64` clamped to `INT_MAX` (`_atoi64`, 0 for no digits). */
export function parsePrice(text: string): number {
  const m = /^\s*(\d+)/.exec(text);
  if (!m) return 0;
  const n = Number(m[1]);
  return Number.isFinite(n) ? Math.min(n, PRICE_CLAMP) : PRICE_CLAMP;
}
