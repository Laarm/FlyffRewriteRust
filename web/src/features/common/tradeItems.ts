// What the trade and the personal-shop features share about an item (Plan/45 §14, §15; WP-CFEAT-21, -22): the display fields of an item that is
// not in the own bag (the partner's offer, the goods of a visited shop) and the client half of the tradeable rule.

import type { ClientContext } from '../../app/context';
import type { ClientDataIndex } from '../../data';
import { rulesRuntime } from '../../game/rules';
import { wallNow } from '../../platform/time';
import { isExpired, partsOf } from '../inventory/info';
import { ITEM_FLAG, type InvItem } from '../inventory/items';
import { itemIconPath } from './itemIcon';

/** The fields every item cell of the trade and shop windows carries (`ItemView` of the tooltip host plus the grid's own). */
export interface ForeignItemFields {
  readonly name: string;
  readonly kind2: number;
  readonly parts: number;
  readonly serial: string;
  readonly objId: number;
  readonly itemId: number;
  readonly icon: string | null;
  readonly showCount: boolean;
  readonly expired: boolean;
  readonly item: InvItem;
}

/**
 * The display fields of `it`. `key` names the place of the item (`trade:you:3`, `vendor:12:5`): the tooltip host caches a composition per
 * `(serial, flag, version)`, and two different items must not share a serial (a stack that was split carries the serial of the original in the
 * partner's list, an item of a shop another one).
 */
export function foreignItemFields(
  data: Pick<ClientDataIndex, 'item'> | null | undefined,
  it: InvItem,
  key: string,
): ForeignItemFields {
  const prop = data?.item(it.itemId);
  return {
    name: prop?.name ?? `Item ${String(it.itemId)}`,
    kind2: prop?.item_kind2 ?? 0,
    parts: partsOf(prop),
    serial: `${key}:${String(it.serial)}:${String(it.num)}`,
    objId: it.objId,
    itemId: it.itemId,
    icon: itemIconPath(prop, it),
    showCount: (prop?.pack_max ?? 1) > 1,
    expired: isExpired(it),
    item: it,
  };
}

/** What the client knows of an item that it may offer. */
export interface TradeableFacts {
  /** The item is worn (`container.isEquip`). */
  readonly equipped: boolean;
}

/**
 * `flyff_rules::item::tradeable` for an item of the own bag: `0` when it may be put into a trade or a stall, else the `TID_*` of the server's
 * refusal. The rule is the Rust one (`RulesFacade.tradeable`); without the rules context (the wasm module is not up) the answer is `0` and the
 * server decides. The random-option part of `IsBinds` needs data tables the client does not hold: the server says no for those.
 */
export function tradeRefusal(ctx: ClientContext, it: InvItem, facts: TradeableFacts): number {
  const rules = rulesRuntime(ctx).rules();
  if (!rules) return 0;
  return rules.tradeable({
    itemId: it.itemId,
    reserved: it.extra !== 0,
    flags: it.flag,
    keepUntil: it.keepTimeAt,
    equipped: facts.equipped,
    inUse: (it.flag & ITEM_FLAG.ISUSING) !== 0,
    guildId: it.idGuild,
    now: Math.floor(wallNow() / 1000),
  });
}
