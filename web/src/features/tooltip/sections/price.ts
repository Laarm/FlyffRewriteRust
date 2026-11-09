// Section 15 of the item tooltip: the price (Plan/45 §10.2; `PutItemGold`, `src:_Interface/WndManager.cpp:5800-5948`). The numbers
// are the server rule's (`numbers.prices`, Plan/19 §7: shop rate, event factor and Perin fixed before the tax, the tax per piece
// beside): in a penya shop window the line prints `buy_unit + buy_tax` and the tax line (the price before tax); with a shop open the
// inventory prints what a piece sells for (`sell_unit`, at least 1); a personal vendor prints its listed price.

import { APP_INVENTORY, APP_SHOPEX, APP_VENDOR_REVISION } from '../../../gen/defs/resdata';
import {
  TID_GAME_REDCHIP,
  TID_GAME_TAX_RATE,
  TID_GAME_TOOLTIP_COST2,
  TID_GAME_TOOLTIP_SHELLCOST2,
} from '../../../gen/defs/text';
import { DEFAULT_TEXT_COLOR } from '../../../ui/text/markup';
import { isTaxableItem } from '../../events/tax/rates';
import { fmtNumberComma } from '../../common/numbers';
import type { ComposeCx } from '../model';
import { sprintf } from '../printf';

/** The tax line: `TID_GAME_TAX_RATE(price before tax)` in the colour of its text. */
function taxLine(cx: ComposeCx, before: number): void {
  const { env, b } = cx;
  b.add('\n', DEFAULT_TEXT_COLOR);
  b.add(
    sprintf(env.text(TID_GAME_TAX_RATE), [fmtNumberComma(before)]),
    env.textColor(TID_GAME_TAX_RATE),
  );
}

export function putItemGold(cx: ComposeCx): void {
  const { flag, extra, numbers, env, b, colors } = cx;
  if (flag === APP_SHOPEX) {
    const prices = numbers?.prices;
    const shop = extra.shop;
    if (!prices || !shop || prices.cost === -1) return;
    let cost: number;
    let before = 0;
    if (shop.vendorType === 0) {
      before = prices.buyUnit;
      cost = prices.buyUnit + prices.buyTax;
    } else {
      cost = prices.chipCost;
    }
    if (cost <= 0) {
      cost = 1;
      before = 1;
    }
    let line = sprintf(env.text(TID_GAME_TOOLTIP_COST2), [fmtNumberComma(cost)]);
    if (shop.vendorType === 1) line += ` ${env.text(TID_GAME_REDCHIP)}`;
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(line, colors.gold);
    // `CTax::IsApplyTaxRate` is per item: no tax line for the Perin, the Red Chip and the awakening scrolls
    if (shop.taxApplies && shop.vendorType === 0 && isTaxableItem(cx.item.itemId))
      taxLine(cx, before);
    return;
  }
  if (flag === APP_INVENTORY) {
    // the sell price shows only while a shop window is open
    const prices = numbers?.prices;
    const shop = extra.shop;
    if (!prices || !shop || prices.cost === -1) return;
    let after = prices.sellUnit;
    let before = prices.sellUnit + prices.sellTax;
    if (after < 1) {
      after = 1;
      before = 1;
    }
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(sprintf(env.text(TID_GAME_TOOLTIP_SHELLCOST2), [fmtNumberComma(after)]), colors.gold);
    if (shop.taxApplies && isTaxableItem(cx.item.itemId)) taxLine(cx, before);
    return;
  }
  if (flag === APP_VENDOR_REVISION) {
    const price = extra.vendorPrice;
    if (price === null || price === undefined || price === 0xffff_ffff) return;
    b.add('\n', DEFAULT_TEXT_COLOR);
    b.add(sprintf(env.text(TID_GAME_TOOLTIP_SHELLCOST2), [fmtNumberComma(price)]), colors.gold);
  }
}
