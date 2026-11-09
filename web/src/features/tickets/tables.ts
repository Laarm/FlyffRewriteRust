// The client ticket tables (`CTicketProperty` + `CLayerProperty`, Plan/45 §9.5 R24; WP-CFEAT-45 / the open part of WP-CFEAT-15): `bundle('item-ext').tickets` indexed by the ticket item. The server's
// `ticket.inc` (5 tickets on the shipped data: Kebaras 26529, 26530, 26558 in world 2, Cisland 26681, 26682 in world 3) and `layer.inc` (both worlds expand 1) are projected by
// `crates/flyff-data/src/bundle/project/items.rs`; the original client read the same two files itself.

import type { TicketRow } from '../../gen/bundles';

/** What the item dispatch (`DispatchEnv.ticketOf`) asks: the world the ticket opens and the number of EXPANDED layers (`-expand ..= 0` may be entered). */
export interface TicketInfo {
  readonly world: number;
  readonly expandedLayers: number;
}

/** The index of a bundle's rows by ticket item (a repeated item keeps the first row, as `map::insert`). */
export function ticketIndex(
  rows: readonly TicketRow[] | undefined,
): ReadonlyMap<number, TicketInfo> {
  const out = new Map<number, TicketInfo>();
  for (const r of rows ?? []) {
    if (!out.has(r.item))
      out.set(r.item, { world: r.world, expandedLayers: Math.max(0, r.expand) });
  }
  return out;
}

/** Caches the index per rows array: the bundle's `tickets` array is the key, so a reloaded bundle rebuilds it. */
export function cachedTicketIndex(): (
  rows: readonly TicketRow[] | undefined,
) => ReadonlyMap<number, TicketInfo> {
  let key: readonly TicketRow[] | undefined;
  let index: ReadonlyMap<number, TicketInfo> = new Map();
  return (rows) => {
    if (rows !== key) {
      key = rows;
      index = ticketIndex(rows);
    }
    return index;
  };
}
