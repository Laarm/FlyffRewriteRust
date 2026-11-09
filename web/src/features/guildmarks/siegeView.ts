// The siege facts the overlay needs (WP-REND-26): from ui-siege's `eventStore.siege` - the siege state and war state, the winning guild, the best
// player and the defenders (the keys of the C++ roster map: a roster guild with no listed players has none). `null` without the events feature (no
// defender colour, no marks). The object is memoised on the slice, the defender set on the roster, so a frame costs two comparisons.

import type { ClientContext } from '../../app/context';
import type { ViewerSiege } from '../../render/overlay/types';
import type { EventStore, RosterGuild, SiegeSlice } from '../events/store';

interface Memo {
  slice: SiegeSlice | null;
  view: ViewerSiege | null;
  roster: readonly RosterGuild[] | null;
  defenders: ReadonlySet<number>;
}

const memos = new WeakMap<ClientContext, Memo>();

/** `GetGCStatusDefender` keys: the defender character ids of the roster guilds that list at least one player. */
export function defendersOf(roster: readonly RosterGuild[]): ReadonlySet<number> {
  const out = new Set<number>();
  for (const g of roster) if (g.players.length > 0) out.add(g.defender);
  return out;
}

/** The facts of one slice (pure; `defenders` is passed in so the caller can reuse it). */
export function siegeMarks(s: SiegeSlice, defenders: ReadonlySet<number>): ViewerSiege {
  return {
    state: s.state,
    warState: s.warState,
    winGuildId: s.winGuild?.guildId ?? 0,
    bestPlayerId: s.bestPlayer ?? 0,
    defenders,
  };
}

/** The siege facts of a context, `null` when the events store does not exist. */
export function siegeMarksOf(ctx: ClientContext): ViewerSiege | null {
  const store = ctx.stores.registry.get('events') as unknown as EventStore | undefined;
  if (!store) return null;
  const slice = store.getState().siege;
  let memo = memos.get(ctx);
  if (!memo) {
    memo = { slice: null, view: null, roster: null, defenders: new Set() };
    memos.set(ctx, memo);
  }
  if (memo.slice === slice && memo.view) return memo.view;
  if (memo.roster !== slice.roster) {
    memo.roster = slice.roster;
    memo.defenders = defendersOf(slice.roster);
  }
  memo.slice = slice;
  memo.view = siegeMarks(slice, memo.defenders);
  return memo.view;
}
