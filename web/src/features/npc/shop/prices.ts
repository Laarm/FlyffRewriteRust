// The prices the shop windows show (Plan/45 §12.2-§12.4, Plan/19 §7): the numbers come from the wasm rules (`RulesFacade`:
// `buyUnitPrice`, `sellUnitPrice`, `quickSellUnitPrice`, `chipCost`), never from a formula in the windows. While the rules
// context is not built (the bundles are still loading) the cost of the item table is the stand-in. The tax is the one of the
// PLAYER's tax area (`taxOf( ctx )`, `CTax::GetContinent( g_pPlayer )`, WP-CFEAT-52): the f32 fraction when the area is East or West,
// `null` (not taxable) elsewhere; the rules exempt the Perin, the Red Chip and the awakening scrolls themselves. A quick sale is taxed
// only for a player of authority GENERAL (`econ/quick_sell.rs`); the shop-rate and event factors are the server's runtime values the
// client does not know (1.0).

import type { ClientContext } from '../../../app/context';
import { rulesRuntime } from '../../../game/rules';
import { taxOf } from '../../events/tax/taxModule';
import type { InvItem } from '../../inventory/items';
import { ITEM_FLAG } from '../../inventory/items';

/** The price of one piece in a penya shop: the unit before the tax and the purchase tax. */
export interface BuyPrice {
  readonly unit: number;
  readonly tax: number;
}

export interface ShopPrices {
  /** One piece in a penya shop (`unit + tax` is what the shop window shows and the purchase costs). */
  buy(itemId: number): BuyPrice;
  /** One piece in a Red Chip shop, `-1` when the item has no chip price. */
  chip(itemId: number): number;
  /** What one piece of an inventory item pays when sold to an NPC (after the tax). */
  sell(item: InvItem): number;
  /** What one piece of a quick sale pays (a bound item 1 penya, PL-168). */
  quick(item: InvItem): number;
}

/** `AUTH_GENERAL` ('F'): the authority of an ordinary player. */
const AUTH_GENERAL = 70;

export function shopPricesOf(ctx: ClientContext, cost: (itemId: number) => number): ShopPrices {
  const rules = () => rulesRuntime(ctx).rules();
  // the tax of the area the player stands in: `null` outside East and West
  const purchaseTax = (): number | null => {
    const t = taxOf(ctx);
    return t?.applies() === true ? t.purchaseRate() : null;
  };
  const salesTax = (): number | null => {
    const t = taxOf(ctx);
    return t?.applies() === true ? t.salesRate() : null;
  };
  return {
    buy(itemId) {
      const r = rules();
      if (r) return r.buyUnitPrice({ itemId, abilityOption: 0, taxRate: purchaseTax() });
      return { unit: Math.max(1, cost(itemId)), tax: 0 };
    },
    chip(itemId) {
      return rules()?.chipCost(itemId) ?? -1;
    },
    sell(item) {
      const r = rules();
      if (r)
        return r.sellUnitPrice({
          itemId: item.itemId,
          abilityOption: item.abilityOption,
          taxRate: salesTax(),
        }).unit;
      return Math.max(1, Math.trunc(cost(item.itemId) / 4));
    },
    quick(item) {
      const r = rules();
      const binds = (item.flag & ITEM_FLAG.BINDS) !== 0;
      if (r) {
        return r.quickSellUnitPrice({
          itemId: item.itemId,
          abilityOption: item.abilityOption,
          binds,
          // the server taxes a quick sale only for a player of authority GENERAL (a GM's quick sale pays no tax)
          taxRate: ctx.stores.selfStore.getState().authority === AUTH_GENERAL ? salesTax() : null,
        }).unit;
      }
      return binds ? 1 : Math.max(1, Math.trunc(cost(item.itemId) / 4));
    },
  };
}
