// The `tickets` feature (Plan/45 §9.5 R24, §9.6; WP-CFEAT-45; the open ticket-tables part of WP-CFEAT-15): the client ticket rows for the item dispatch and the active ticket for the buff tooltip.
//
//   ticketOf( item )     `bundle('item-ext').tickets` -> `{ world, expandedLayers }`, handed to the item dispatch (`inventoryOf( ctx ).registerDispatchEnv( { ticketOf } )`). R24 of the router then
//                        asks the `TID_GAME_TICKET_DESC` confirm in the ticket's own world (Yes sends `DoUseItemInput { data, "0" }`) and opens `APP_SELECT_CHANNEL` elsewhere (`expandedLayers + 1`
//                        rows; a choice sends `DoUseItemInput { data, "0" | "-1" | ... }` and NOTHING else: PL-124 FIX)
//   activeTicketEnd()    `CMover::GetTicket()` for `DurationEnv.ticketEnd` of the item duration list (`features/buffs`)
//
// The bundle is a boot bundle, so the rows are read at call time: nothing needs re-registering when it arrives.

import type { ClientContext } from '../../app/context';
import { APP_SELECT_CHANNEL } from '../../gen/defs/resdata';
import type { WndSelectChannel } from '../../ui/windows/itemuse/WndSelectChannel';
import { type FeatureContext, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { isExpired } from '../inventory/info';
import { inventoryOf } from '../inventory';
import { buffsOf } from '../buffs/buffsModule';
import { activeTicketEnd, type TicketItem } from './active';
import { cachedTicketIndex, type TicketInfo } from './tables';

/** The public face of the tickets feature of a context. */
export interface TicketsFeature {
  /** The world and the expanded layers of a ticket item, `null` when it is none. */
  ticketOf(itemId: number): TicketInfo | null;
  /** The number of ticket rows now loaded (0 before the bundle arrived). */
  count(): number;
  /** `GetTicket()`'s keep end (unix seconds), `null` for none. */
  activeEnd(): number | null;
}

const features = new WeakMap<ClientContext, TicketsFeature>();

/** The tickets feature of a context; `undefined` before the module registered. */
export function ticketsOf(ctx: ClientContext): TicketsFeature | undefined {
  return features.get(ctx);
}

/** Registers the tickets (a handler module of its own, `features/minigamesM7.ts`, on the `itemuse` feature context). */
export function registerTickets(fc: FeatureContext): TicketsFeature {
  const ctx = fc.app;
  const index = cachedTicketIndex();
  const rows = () => ctx.data?.bundle('item-ext')?.tickets;
  const feature: TicketsFeature = {
    ticketOf: (itemId) => index(rows()).get(itemId) ?? null,
    count: () => index(rows()).size,
    activeEnd: () => activeEnd(ctx),
  };
  features.set(ctx, feature);
  inventoryOf(ctx)?.registerDispatchEnv({ ticketOf: (id) => feature.ticketOf(id) });
  addProbeMember(ctx, 'tickets', () => {
    const sel = fullUi(ctx)?.wm.get(APP_SELECT_CHANNEL) as WndSelectChannel | null | undefined;
    const list = sel?.list;
    return {
      rows: feature.count(),
      activeEnd: feature.activeEnd(),
      worldId: ctx.stores.selfStore.getState().worldId,
      // the layer list of `APP_SELECT_CHANNEL`: the texts of its rows, `null` while it is closed
      selector: list
        ? Array.from({ length: list.getCount() }, (_, i) => list.getPlainText(i))
        : null,
    };
  });
  return feature;
}

/** `CMover::GetTicket()` over the live buffs and bag of a context. */
export function activeEnd(ctx: ClientContext): number | null {
  const inv = inventoryOf(ctx);
  const buffs = buffsOf(ctx);
  if (!inv || !buffs) return null;
  const kind3Of = (id: number): number | undefined => ctx.data?.item(id)?.item_kind3;
  const items = function* (): Iterable<TicketItem> {
    const c = inv.state().container;
    for (let slot = 0; slot < c.indexNum; slot += 1) {
      const it = c.getAtSlot(slot);
      if (it) yield { itemId: it.itemId, keepTimeAt: it.keepTimeAt, expired: isExpired(it) };
    }
  };
  return activeTicketEnd({
    buffs: () => buffs.own().list(),
    kind3Of,
    items,
  });
}
