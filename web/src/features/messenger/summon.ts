// The friend-summon rules of the client (`CWndSummonFriend::OnChildNotify`, `OnSummonFriend`, `OnSummonFriendConfirm`; `src:_Interface/WndSummonFriend.cpp:150-250`,
// `Neuz/DPClient.cpp:16141-16200`; Plan/21 §4.11; WP-CFEAT-39). Pure, so the table of outcomes is tested without windows.

import { FRS, type MessengerRow } from '../../ui/windows/messenger/ports';

/** `MAX_NAME`. */
export const MAX_NAME = 64;
/** The shortest name the window takes (`3 <= string.GetLength()`). */
export const MIN_SUMMON_NAME = 3;

const bytes = (s: string): number => new TextEncoder().encode(s).length;

export type SummonOutcome =
  /** Fewer than 3 or 64 and more bytes: the window stays open (the original did nothing). */
  | { readonly kind: 'keep' }
  /** The own name: the line `TID_GAME_SUMMON_FRIEND_MY_NOUSE`; the window closes. */
  | { readonly kind: 'self' }
  /** The scroll is not in the bag any more: the line `TID_ERROR_SUMMONFRIEND_NOITEM( scroll name, name )`; the window closes. */
  | { readonly kind: 'noItem' }
  /** `SummonFriend{ data, name }`; the window closes. */
  | { readonly kind: 'send'; readonly name: string };

/** What OK of the name window does with the typed text. */
export function summonOutcome(
  typed: string,
  ownName: string,
  scrollStillThere: boolean,
): SummonOutcome {
  const n = bytes(typed);
  if (n < MIN_SUMMON_NAME || n >= MAX_NAME) return { kind: 'keep' };
  if (typed === ownName) return { kind: 'self' };
  if (!scrollStillThere) return { kind: 'noItem' };
  return { kind: 'send', name: typed };
}

/** The names of the combo box: the online, unblocked friends in list order (`dwState != FRS_OFFLINE && !bBlock`). */
export function summonCandidates(friends: readonly MessengerRow[]): string[] {
  return friends
    .filter((f) => f.state !== FRS.OFFLINE && !f.blocked && f.name !== '')
    .map((f) => f.name);
}
