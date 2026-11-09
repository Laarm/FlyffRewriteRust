// The visit ports of the messenger (Plan/45 §28.10, §24.1; `WndMessengerCtrl.cpp:795-811`): the friend menu offers "allow my house" / "forbid my house" (`MMI` popup ids 8 and 7) and the friend rows turn green for the
// friends who may visit. `messengerOf( ctx ).setVisitPorts( visitPorts( ... ) )`.

import type { VisitPorts } from '../../messenger/api';
import type { HousingSenders } from './senders';
import type { HousingSlice } from './slice';

export interface VisitListeners {
  /** Tells the subscribers (`CWndMessengerEx::UpdateFriendList`). */
  notify(): void;
  subscribe(listener: () => void): () => void;
}

export function visitListeners(): VisitListeners {
  const fns = new Set<() => void>();
  return {
    notify: () => {
      for (const fn of [...fns]) fn();
    },
    subscribe: (fn) => {
      fns.add(fn);
      return () => {
        fns.delete(fn);
      };
    },
  };
}

/** `{ allowed, set, subscribe }` over the slice and the sender. */
export function visitPorts(
  state: () => HousingSlice,
  senders: Pick<HousingSenders, 'reqSetVisitAllow'>,
  listeners: VisitListeners,
): VisitPorts {
  return {
    allowed: (id) => state().visitors.has(id),
    set: (id, allow) => {
      senders.reqSetVisitAllow(id, allow);
    },
    subscribe: (fn) => listeners.subscribe(fn),
  };
}
