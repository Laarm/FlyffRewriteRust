// `CMover::GetTicket()` (`src:_Common/Mover.cpp:12036-12056`) for the duration tooltip of the ticket buff (`DurationEnv.ticketEnd` of `features/buffs/durations.ts`): the own item buff of kind
// `IK3_TICKET` names the ticket item; the first INVENTORY item with that id that has a keep time and has not expired is the active ticket and its keep end is the answer; anything else is `null`.

import { IK3_TICKET } from '../../gen/defs/itemkind';
import { BUFF_SKILL } from '../buffs/types';

/** One buff of the own set. */
export interface TicketBuff {
  readonly type: number;
  readonly id: number;
}

/** One bag item. */
export interface TicketItem {
  readonly itemId: number;
  /** `m_dwKeepTime` as the unix end (`InvItem.keepTimeAt`), 0 = no keep time. */
  readonly keepTimeAt: number;
  readonly expired: boolean;
}

/** What `activeTicketEnd` reads (the module wires the buff set, the item props and the bag). */
export interface ActiveTicketFacts {
  /** The own buffs, in the order of the set. */
  buffs(): readonly TicketBuff[];
  /** `ItemProp::dwItemKind3` of an item (`undefined` for an unknown one). */
  kind3Of(itemId: number): number | undefined;
  /** The bag in slot order. */
  items(): Iterable<TicketItem>;
}

/** The unix end of the active ticket, `null` for none. */
export function activeTicketEnd(f: ActiveTicketFacts): number | null {
  // `m_buffs.GetBuffByIk3( IK3_TICKET )`: an item buff whose prop is a ticket
  const buff = f.buffs().find((b) => b.type !== BUFF_SKILL && f.kind3Of(b.id) === IK3_TICKET);
  if (!buff) return null;
  for (const it of f.items()) {
    if (
      it.keepTimeAt > 0 &&
      it.itemId === buff.id &&
      !it.expired &&
      f.kind3Of(it.itemId) === IK3_TICKET
    ) {
      return it.keepTimeAt;
    }
  }
  return null;
}
